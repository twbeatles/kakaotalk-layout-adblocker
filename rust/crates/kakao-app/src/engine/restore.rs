use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;

use kakao_core::WindowIdentity;
use kakao_win32::api::{Win32Api, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SW_SHOW};
use tracing::warn;

use super::apply::identity_matches;
use super::caches::EngineCaches;
use super::flags::SharedFlags;
use super::model::{
    RestoreSnapshot, RESTORE_GIVEUP_COOLDOWN_TICKS, RESTORE_MAX_ATTEMPTS, RESTORE_MISS_THRESHOLD,
};

pub fn restore_all(
    api: &dyn Win32Api,
    snapshots: &mut HashMap<WindowIdentity, RestoreSnapshot>,
) -> (u32, String) {
    let mut failures = 0u32;
    let mut last_error = String::new();
    let pending: Vec<_> = snapshots.drain().map(|(_, snap)| snap).collect();
    for snap in pending {
        if !identity_matches(api, &snap.identity) {
            continue;
        }
        if restore_snapshot(api, &snap, &mut last_error) {
            continue;
        }
        failures += 1;
        snapshots.insert(snap.identity.clone(), snap);
    }
    (failures, last_error)
}

fn restore_snapshot(api: &dyn Win32Api, snap: &RestoreSnapshot, last_error: &mut String) -> bool {
    let mut ok = true;
    if let Some(rect) = snap.rect {
        if rect.width() > 0 && rect.height() > 0 {
            let mut flags = SWP_NOZORDER | SWP_NOACTIVATE;
            if !snap.top_level {
                flags |= SWP_NOMOVE;
            }
            if !api.set_window_pos(
                snap.identity.hwnd,
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
                flags,
            ) {
                ok = false;
                *last_error = format!("restore pos failed hwnd={}", snap.identity.hwnd);
            }
        }
    }
    if snap.was_visible {
        let _ = api.show_window(snap.identity.hwnd, SW_SHOW);
        if !api.is_window_visible(snap.identity.hwnd) {
            ok = false;
            *last_error = format!("restore show failed hwnd={}", snap.identity.hwnd);
        }
    }
    ok
}

/// Backoff before the next restore attempt, in reconciliation ticks.
fn retry_cooldown_ticks(attempts: u32) -> u32 {
    if attempts >= RESTORE_MAX_ATTEMPTS {
        return RESTORE_GIVEUP_COOLDOWN_TICKS;
    }
    1u32 << attempts.saturating_sub(1).min(4)
}

/// Restore windows this process hid that no longer look like ads.
///
/// A window whose ancestors are hidden (KakaoTalk closed to tray) can refuse to
/// become visible again for as long as the user leaves it closed. Retrying that
/// every tick would emit a warning about five times a second forever, so
/// failures back off exponentially and are logged once per window.
pub(super) fn restore_stale_hidden(
    api: &dyn Win32Api,
    caches: &mut EngineCaches,
    matched: &HashSet<WindowIdentity>,
) -> (u32, String) {
    let mut last_error = String::new();
    let pending: Vec<WindowIdentity> = caches.snapshots.keys().cloned().collect();
    for identity in pending {
        if matched.contains(&identity) {
            caches.stale.remove(&identity);
            continue;
        }
        let due = {
            let state = caches.stale.entry(identity.clone()).or_default();
            state.miss_streak = state.miss_streak.saturating_add(1);
            if state.miss_streak < RESTORE_MISS_THRESHOLD {
                false
            } else if state.cooldown > 0 {
                state.cooldown -= 1;
                false
            } else {
                true
            }
        };
        if !due {
            continue;
        }
        let Some(snap) = caches.snapshots.remove(&identity) else {
            caches.stale.remove(&identity);
            continue;
        };
        if !identity_matches(api, &snap.identity) {
            caches.stale.remove(&identity);
            continue;
        }
        if restore_snapshot(api, &snap, &mut last_error) {
            caches.stale.remove(&identity);
            continue;
        }
        let hwnd = identity.hwnd;
        caches.snapshots.insert(identity.clone(), snap);
        let state = caches.stale.entry(identity).or_default();
        state.attempts = state.attempts.saturating_add(1);
        state.cooldown = retry_cooldown_ticks(state.attempts);
        if !state.warned {
            state.warned = true;
            warn!(
                hwnd,
                attempts = state.attempts,
                cooldown_ticks = state.cooldown,
                last_error = %last_error,
                "stale hide restore failed; backing off and retrying quietly"
            );
        }
    }
    (caches.restore_failure_count(), last_error)
}

pub(super) fn report_restore(flags: &SharedFlags, failures: u32, err: &str, context: &str) {
    flags.restore_failures.store(failures, Ordering::SeqCst);
    if failures > 0 {
        flags.set_last_error(err);
        warn!(failures, last_error = %err, "{context}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_cooldown_backs_off_then_settles_on_the_long_interval() {
        assert_eq!(retry_cooldown_ticks(1), 1);
        assert_eq!(retry_cooldown_ticks(2), 2);
        assert_eq!(retry_cooldown_ticks(3), 4);
        assert_eq!(retry_cooldown_ticks(4), 8);
        assert_eq!(
            retry_cooldown_ticks(RESTORE_MAX_ATTEMPTS),
            RESTORE_GIVEUP_COOLDOWN_TICKS
        );
        assert_eq!(retry_cooldown_ticks(50), RESTORE_GIVEUP_COOLDOWN_TICKS);
    }
}
