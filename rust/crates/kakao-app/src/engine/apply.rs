use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;

use kakao_core::{Evaluation, WindowGraph, WindowIdentity};
use kakao_win32::api::{Win32Api, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SW_HIDE, WM_CLOSE};
use tracing::debug;

use super::flags::SharedFlags;
use super::model::RestoreSnapshot;

pub fn capture_snapshot(
    api: &dyn Win32Api,
    graph: &WindowGraph,
    hwnd: i64,
) -> Option<RestoreSnapshot> {
    let node = graph.get(hwnd)?;
    Some(RestoreSnapshot {
        identity: node.identity(),
        was_visible: api.is_window_visible(hwnd),
        rect: api.get_window_rect(hwnd).or(node.rect),
        top_level: node.structural_parent.is_none(),
    })
}

/// Keep the first snapshot of a window. Hidden ads are re-hidden every tick,
/// and capturing again (IsWindowVisible + GetWindowRect per window per tick)
/// only to discard the result was pure overhead.
fn remember_snapshot(
    api: &dyn Win32Api,
    graph: &WindowGraph,
    hwnd: i64,
    snapshots: &mut HashMap<WindowIdentity, RestoreSnapshot>,
) {
    let Some(node) = graph.get(hwnd) else {
        return;
    };
    if snapshots.contains_key(&node.identity()) {
        return;
    }
    if let Some(snap) = capture_snapshot(api, graph, hwnd) {
        snapshots.insert(snap.identity.clone(), snap);
    }
}

pub(super) fn identity_matches(api: &dyn Win32Api, identity: &WindowIdentity) -> bool {
    api.is_window(identity.hwnd)
        && api.get_window_thread_process_id(identity.hwnd) == identity.pid
        && api.get_class_name(identity.hwnd) == identity.class_name
}

pub fn apply_evaluation(
    api: &dyn Win32Api,
    graph: &WindowGraph,
    evaluation: &Evaluation,
    snapshots: &mut HashMap<WindowIdentity, RestoreSnapshot>,
    pids: &HashSet<i64>,
    flags: &SharedFlags,
) {
    if flags.stopping.load(Ordering::SeqCst)
        || !flags.enabled.load(Ordering::SeqCst)
        || !flags.apply.load(Ordering::SeqCst)
    {
        return;
    }
    let precheck = |hwnd: i64| -> bool {
        if flags.stopping.load(Ordering::SeqCst) {
            return false;
        }
        let Some(node) = graph.get(hwnd) else {
            return false;
        };
        if !pids.contains(&node.pid) {
            return false;
        }
        // A not-responding KakaoTalk thread would block the synchronous
        // ShowWindow/SetWindowPos below; skip it and re-evaluate next tick.
        identity_matches(api, &node.identity()) && !api.is_hung_app_window(hwnd)
    };

    for hwnd in &evaluation.actions.close {
        if !precheck(*hwnd) {
            continue;
        }
        let (delivered, _) = api.send_message_timeout(*hwnd, WM_CLOSE, 0, 0, 500);
        // A close request is only a request. Record what actually happened so
        // a KakaoTalk UI change that starts refusing WM_CLOSE is diagnosable
        // from the log instead of silently relying on the hide fallback.
        // DEBUG, not WARN: a surviving popup is re-evaluated every tick.
        if !api.is_window(*hwnd) {
            flags.closed_windows.fetch_add(1, Ordering::SeqCst);
            debug!(hwnd = *hwnd, "window destroyed by WM_CLOSE");
        } else if delivered {
            debug!(
                hwnd = *hwnd,
                "window refused WM_CLOSE; hide/zero-size fallback applies"
            );
        } else {
            debug!(
                hwnd = *hwnd,
                error = api.get_last_error(),
                "WM_CLOSE was not delivered; hide/zero-size fallback applies"
            );
        }
    }
    for hwnd in &evaluation.actions.hide {
        if !precheck(*hwnd) {
            continue;
        }
        remember_snapshot(api, graph, *hwnd, snapshots);
        // ShowWindow returns non-zero only when the window was previously
        // visible, so this counts newly hidden windows rather than the
        // per-tick re-application on an already hidden one.
        if api.show_window(*hwnd, SW_HIDE) {
            flags.hidden_windows.fetch_add(1, Ordering::SeqCst);
        }
    }
    for pos in &evaluation.actions.set_pos {
        if pos.len() < 5 {
            continue;
        }
        let hwnd = pos[0];
        if !precheck(hwnd) {
            continue;
        }
        // View resize is size-only (Python SWP_NOMOVE). Zero-size popup
        // fallback must still be allowed to move to 0,0.
        let width = pos[3];
        let height = pos[4];
        let is_view_resize = width > 0 && height > 0;
        // Python stop() restores hidden/zero-sized windows only. Snapshotting
        // OnlineMainView resize and replaying GetWindowRect through
        // SetWindowPos treats screen coordinates as parent-relative, which
        // shoves the main view off-canvas and blacks out KakaoTalk.
        if !is_view_resize {
            remember_snapshot(api, graph, hwnd, snapshots);
        }
        let mut swp_flags = SWP_NOZORDER | SWP_NOACTIVATE;
        if is_view_resize {
            swp_flags |= SWP_NOMOVE;
        }
        let applied = api.set_window_pos(
            hwnd,
            pos[1] as i32,
            pos[2] as i32,
            width as i32,
            height as i32,
            swp_flags,
        );
        if applied && is_view_resize {
            flags.resized_windows.fetch_add(1, Ordering::SeqCst);
        }
    }
}
