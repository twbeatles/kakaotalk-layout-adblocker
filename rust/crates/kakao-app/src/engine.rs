use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use kakao_core::{
    evaluate_graph_for_apply, evaluate_graph_with_states, CandidateState, Evaluation, LayoutRules,
    WindowGraph, WindowIdentity,
};
use kakao_win32::api::{
    Win32Api, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SW_HIDE, SW_SHOW, WM_CLOSE,
};
use tracing::{debug, info, warn};

use crate::config::AppSettings;
use crate::graph_build::build_graph;

/// Consecutive ticks a hidden window must go unmatched before it is restored.
pub const RESTORE_MISS_THRESHOLD: u32 = 2;
/// After this many consecutive restore failures a window stops being retried
/// every tick and falls back to the long cooldown below.
const RESTORE_MAX_ATTEMPTS: u32 = 5;
/// ~60s at the default 200ms reconciliation. A window can become restorable
/// again later (for example when KakaoTalk's main window is reopened), so the
/// engine keeps retrying — just rarely, and without repeating the warning.
const RESTORE_GIVEUP_COOLDOWN_TICKS: u32 = 300;
/// How long after the last WinEvent the engine treats KakaoTalk as "active"
/// and reconciles at `poll_interval_ms` instead of `idle_poll_interval_ms`.
const ACTIVE_WINDOW: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct RestoreSnapshot {
    pub identity: WindowIdentity,
    pub was_visible: bool,
    pub rect: Option<kakao_core::Rect>,
    pub top_level: bool,
}

/// Per-window bookkeeping for the stale-hide restore path.
#[derive(Clone, Debug, Default)]
pub struct StaleState {
    /// Consecutive ticks this hidden window was not matched as an ad.
    pub miss_streak: u32,
    /// Consecutive failed restore attempts. `0` means "not currently failing".
    pub attempts: u32,
    /// Ticks to skip before the next retry (exponential backoff).
    pub cooldown: u32,
    /// Whether the failure was already reported, so it is logged only once.
    pub warned: bool,
}

/// Worker-owned caches. Bundled so the tick signature stays small and so the
/// cache-cleanup clock has a home.
#[derive(Default)]
pub struct EngineCaches {
    pub snapshots: HashMap<WindowIdentity, RestoreSnapshot>,
    pub states: HashMap<WindowIdentity, CandidateState>,
    pub stale: HashMap<WindowIdentity, StaleState>,
    last_cleanup: Option<Instant>,
}

impl EngineCaches {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of windows currently stuck in a failed restore.
    pub fn restore_failure_count(&self) -> u32 {
        u32::try_from(
            self.stale
                .values()
                .filter(|state| state.attempts > 0)
                .count(),
        )
        .unwrap_or(u32::MAX)
    }

    /// Tray "복원 실패 초기화": forget the failure bookkeeping so every pending
    /// snapshot gets a fresh, immediate retry.
    pub fn clear_restore_failures(&mut self) {
        for state in self.stale.values_mut() {
            state.attempts = 0;
            state.cooldown = 0;
            state.warned = false;
        }
    }

    /// Restore everything and drop stale bookkeeping for whatever came back.
    pub fn drain_restore_all(&mut self, api: &dyn Win32Api) -> (u32, String) {
        let result = restore_all(api, &mut self.snapshots);
        let Self {
            snapshots, stale, ..
        } = self;
        stale.retain(|identity, _| snapshots.contains_key(identity));
        result
    }

    pub fn clear_transient(&mut self) {
        self.states.clear();
        self.stale.clear();
        self.last_cleanup = None;
    }

    fn cleanup_due(&mut self, interval: Duration) -> bool {
        match self.last_cleanup {
            Some(at) if at.elapsed() < interval => false,
            _ => {
                self.last_cleanup = Some(Instant::now());
                true
            }
        }
    }
}

pub struct SharedFlags {
    pub enabled: Arc<AtomicBool>,
    pub aggressive: Arc<AtomicBool>,
    pub stopping: Arc<AtomicBool>,
    pub apply: Arc<AtomicBool>,
    pub startup: Arc<AtomicBool>,
    pub reset_restore: Arc<AtomicBool>,
    /// Gauge: windows currently stuck in a failed restore (not a running total).
    pub restore_failures: Arc<AtomicU32>,
    /// Gauge: confirmed KakaoTalk main windows seen by the last evaluation.
    pub main_windows: Arc<AtomicU32>,
    /// Running total of windows this process actually hid.
    pub hidden_windows: Arc<AtomicU32>,
    /// Running total of windows confirmed destroyed after `WM_CLOSE`.
    pub closed_windows: Arc<AtomicU32>,
    /// Running total of applied main-view resizes.
    pub resized_windows: Arc<AtomicU32>,
    /// Most recent engine-level error, surfaced in the tray status.
    pub last_error: Arc<Mutex<String>>,
}

impl SharedFlags {
    pub fn from_settings(settings: &AppSettings, apply: bool) -> Arc<Self> {
        Arc::new(Self {
            enabled: Arc::new(AtomicBool::new(settings.enabled)),
            aggressive: Arc::new(AtomicBool::new(settings.aggressive_mode)),
            stopping: Arc::new(AtomicBool::new(false)),
            apply: Arc::new(AtomicBool::new(apply)),
            startup: Arc::new(AtomicBool::new(settings.run_on_startup)),
            reset_restore: Arc::new(AtomicBool::new(false)),
            restore_failures: Arc::new(AtomicU32::new(0)),
            main_windows: Arc::new(AtomicU32::new(0)),
            hidden_windows: Arc::new(AtomicU32::new(0)),
            closed_windows: Arc::new(AtomicU32::new(0)),
            resized_windows: Arc::new(AtomicU32::new(0)),
            last_error: Arc::new(Mutex::new(String::new())),
        })
    }

    pub fn set_last_error(&self, message: &str) {
        if let Ok(mut guard) = self.last_error.lock() {
            guard.clear();
            guard.push_str(message);
        }
    }

    pub fn last_error_text(&self) -> String {
        self.last_error
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

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

fn identity_matches(api: &dyn Win32Api, identity: &WindowIdentity) -> bool {
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
        identity_matches(api, &node.identity())
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
        if let Some(snap) = capture_snapshot(api, graph, *hwnd) {
            snapshots.entry(snap.identity.clone()).or_insert(snap);
        }
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
            if let Some(snap) = capture_snapshot(api, graph, hwnd) {
                snapshots.entry(snap.identity.clone()).or_insert(snap);
            }
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
fn restore_stale_hidden(
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

pub fn tick(
    api: &dyn Win32Api,
    pids: &[i64],
    settings: &AppSettings,
    rules: &LayoutRules,
    caches: &mut EngineCaches,
    flags: &SharedFlags,
) -> Evaluation {
    let mut core = settings.to_core();
    core.enabled = flags.enabled.load(Ordering::SeqCst);
    core.aggressive_mode = flags.aggressive.load(Ordering::SeqCst);
    if pids.is_empty() && caches.snapshots.is_empty() {
        return Evaluation::default();
    }
    let graph = build_graph(api, pids);
    let apply_mode = flags.apply.load(Ordering::SeqCst) && core.enabled;
    // The diagnostic `candidates` payload costs a second full traversal and is
    // discarded on the apply path, so only build it when something reads it.
    let evaluation = if apply_mode {
        evaluate_graph_for_apply(&graph, &core, rules, &mut caches.states)
    } else {
        evaluate_graph_with_states(&graph, &core, rules, &mut caches.states)
    };
    flags.main_windows.store(
        u32::try_from(evaluation.state.main_window_count).unwrap_or(0),
        Ordering::SeqCst,
    );

    let cleanup_interval =
        Duration::from_millis(u64::from(settings.cache_cleanup_interval_ms.max(100)));
    if caches.cleanup_due(cleanup_interval) {
        prune_gone_identities(&graph, caches);
    }

    if apply_mode {
        let pid_set: HashSet<i64> = pids.iter().copied().collect();
        apply_evaluation(
            api,
            &graph,
            &evaluation,
            &mut caches.snapshots,
            &pid_set,
            flags,
        );
        if !caches.snapshots.is_empty() {
            let matched = matched_identities(&graph, &evaluation);
            let (failures, err) = restore_stale_hidden(api, caches, &matched);
            flags.restore_failures.store(failures, Ordering::SeqCst);
            if failures > 0 && !err.is_empty() {
                flags.set_last_error(&err);
            }
        }
    } else {
        for candidate in &evaluation.candidates {
            info!(
                hwnd = candidate.hwnd,
                pid = candidate.pid,
                class = %candidate.class,
                decision = %candidate.decision,
                action = %candidate.action,
                "shadow"
            );
        }
    }
    evaluation
}

fn prune_gone_identities(graph: &WindowGraph, caches: &mut EngineCaches) {
    let live: HashSet<WindowIdentity> = graph.nodes.values().map(|node| node.identity()).collect();
    let EngineCaches {
        snapshots,
        states,
        stale,
        ..
    } = caches;
    states.retain(|id, _| live.contains(id) || snapshots.contains_key(id));
    stale.retain(|id, _| live.contains(id) || snapshots.contains_key(id));
}

fn matched_identities(graph: &WindowGraph, evaluation: &Evaluation) -> HashSet<WindowIdentity> {
    let mut matched = HashSet::new();
    let mut push = |hwnd: i64| {
        if let Some(node) = graph.get(hwnd) {
            matched.insert(node.identity());
        }
    };
    for hwnd in &evaluation.actions.hide {
        push(*hwnd);
    }
    for hwnd in &evaluation.actions.close {
        push(*hwnd);
    }
    for pos in &evaluation.actions.set_pos {
        if pos.len() >= 5 && (pos[3] <= 0 || pos[4] <= 0) {
            push(pos[0]);
        }
    }
    matched
}

fn report_restore(flags: &SharedFlags, failures: u32, err: &str, context: &str) {
    flags.restore_failures.store(failures, Ordering::SeqCst);
    if failures > 0 {
        flags.set_last_error(err);
        warn!(failures, last_error = %err, "{context}");
    }
}

pub fn spawn_worker(
    api: Arc<dyn Win32Api>,
    flags: Arc<SharedFlags>,
    settings: AppSettings,
    rules: LayoutRules,
) -> thread::JoinHandle<EngineCaches> {
    thread::spawn(move || {
        info!("engine worker thread started");
        let mut caches = EngineCaches::new();
        let mut last_pids: HashSet<i64> = HashSet::new();
        let mut cached_pids: Vec<i64> = Vec::new();
        let mut last_pid_scan = Instant::now() - Duration::from_secs(60);
        let mut burst_left = 0u32;
        let mut last_full = Instant::now() - Duration::from_secs(10);
        let mut was_enabled = flags.enabled.load(Ordering::SeqCst);
        let mut last_event_at: Option<Instant> = None;
        #[cfg(windows)]
        let mut hook: Option<kakao_win32::event_hook::EventHook> = None;
        #[cfg(windows)]
        let mut hooked_pids: HashSet<i64> = HashSet::new();
        while !flags.stopping.load(Ordering::SeqCst) {
            if flags.reset_restore.swap(false, Ordering::SeqCst) {
                caches.clear_restore_failures();
                flags.restore_failures.store(0, Ordering::SeqCst);
                flags.set_last_error("");
            }
            let enabled_now = flags.enabled.load(Ordering::SeqCst);
            if was_enabled && !enabled_now && flags.apply.load(Ordering::SeqCst) {
                let (failures, err) = caches.drain_restore_all(api.as_ref());
                report_restore(&flags, failures, &err, "restore on disable had failures");
            }
            was_enabled = enabled_now;
            if !enabled_now {
                thread::sleep(Duration::from_millis(1000));
                continue;
            }

            // Interval-throttled PID scan with fast liveness check:
            // When KakaoTalk PIDs are known and alive, use ultra-fast liveness checks (0.001ms)
            // and perform full process snapshot (9ms) only every 5 seconds or when a PID dies.
            let pid_scan_interval =
                Duration::from_millis(u64::from(settings.pid_scan_interval_ms.max(200)));
            let full_sync_interval = Duration::from_secs(5);
            #[cfg(windows)]
            {
                let pids_alive = !cached_pids.is_empty()
                    && cached_pids
                        .iter()
                        .all(|&pid| kakao_win32::process::is_process_alive(pid));
                let need_scan = if pids_alive {
                    last_pid_scan.elapsed() >= full_sync_interval
                } else {
                    last_pid_scan.elapsed() >= pid_scan_interval
                };
                if need_scan {
                    cached_pids = kakao_win32::process::kakaotalk_pids().into_iter().collect();
                    last_pid_scan = Instant::now();
                }
            }

            let pid_set: HashSet<i64> = cached_pids.iter().copied().collect();
            if !pid_set.is_empty() && pid_set != last_pids {
                burst_left = settings.burst_scan_iterations.max(1);
                last_pids = pid_set.clone();
            }

            // Scope the WinEvent hook to KakaoTalk. A session-wide hook also
            // receives every EVENT_OBJECT_LOCATIONCHANGE from every other
            // process, which can fill the bounded channel and drop the
            // KakaoTalk events this engine actually needs.
            #[cfg(windows)]
            if pid_set != hooked_pids {
                hook = None;
                if !cached_pids.is_empty() {
                    hook = kakao_win32::event_hook::EventHook::install_for_pids(&cached_pids);
                    if hook.is_none() {
                        warn!("failed to install WinEvent hook; falling back to polling");
                    }
                }
                hooked_pids = pid_set.clone();
            }

            let mut events = Vec::new();
            #[cfg(windows)]
            if let Some(hook) = hook.as_ref() {
                events = hook.drain();
                // The hook is PID-scoped already; this is defence in depth for
                // the window between a KakaoTalk restart and the re-install.
                if !pid_set.is_empty() {
                    events.retain(|ev| {
                        let pid = api.get_window_thread_process_id(ev.hwnd);
                        pid_set.contains(&pid)
                    });
                } else {
                    events.clear();
                }
            }
            if !events.is_empty() {
                last_event_at = Some(Instant::now());
            }

            let idle_ms = u64::from(settings.idle_poll_interval_ms.max(200));
            // `poll_interval_ms` is the active cadence: while KakaoTalk is
            // producing window events the engine reconciles this often, and it
            // relaxes back to `idle_poll_interval_ms` once things go quiet.
            let active_ms = u64::from(settings.poll_interval_ms.max(50)).min(idle_ms);
            let active = last_event_at.is_some_and(|at| at.elapsed() < ACTIVE_WINDOW);
            let recon_ms = if active { active_ms } else { idle_ms };

            // When KakaoTalk is not running, cleanup any remaining snapshots and stay completely idle.
            if cached_pids.is_empty() {
                if !caches.snapshots.is_empty() {
                    let (failures, err) = caches.drain_restore_all(api.as_ref());
                    report_restore(
                        &flags,
                        failures,
                        &err,
                        "restore on kakaotalk exit had failures",
                    );
                }
                caches.clear_transient();
                last_event_at = None;
                #[cfg(windows)]
                if let Some(hook) = hook.as_ref() {
                    hook.wait_message(Duration::from_millis(idle_ms));
                    continue;
                }
                thread::sleep(Duration::from_millis(idle_ms));
                continue;
            }

            let due_recon = last_full.elapsed() >= Duration::from_millis(recon_ms);
            let due_burst = burst_left > 0;
            if events.is_empty() && !due_recon && !due_burst {
                let remaining = Duration::from_millis(recon_ms).saturating_sub(last_full.elapsed());
                let wait = remaining.max(Duration::from_millis(10));
                #[cfg(windows)]
                if let Some(hook) = hook.as_ref() {
                    hook.wait_message(wait);
                    continue;
                }
                thread::sleep(wait);
                continue;
            }

            if !events.is_empty() {
                thread::sleep(Duration::from_millis(u64::from(
                    settings.burst_scan_interval_ms.max(10),
                )));
                #[cfg(windows)]
                if let Some(hook) = hook.as_ref() {
                    let mut extra = hook.drain();
                    if !pid_set.is_empty() {
                        extra.retain(|ev| {
                            let pid = api.get_window_thread_process_id(ev.hwnd);
                            pid_set.contains(&pid)
                        });
                    } else {
                        extra.clear();
                    }
                    let _ = extra;
                }
            }

            let _ = tick(
                api.as_ref(),
                &cached_pids,
                &settings,
                &rules,
                &mut caches,
                &flags,
            );
            last_full = Instant::now();
            if burst_left > 0 {
                burst_left -= 1;
                thread::sleep(Duration::from_millis(
                    settings.burst_scan_interval_ms.max(10) as u64,
                ));
            }
        }
        info!(
            "engine worker loop exited: stopping={}",
            flags.stopping.load(Ordering::SeqCst)
        );
        if flags.apply.load(Ordering::SeqCst) {
            let (failures, err) = caches.drain_restore_all(api.as_ref());
            report_restore(&flags, failures, &err, "restore on stop had failures");
        }
        caches
    })
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

    #[test]
    fn restore_failure_count_is_a_gauge_not_a_running_total() {
        let mut caches = EngineCaches::new();
        assert_eq!(caches.restore_failure_count(), 0);
        caches.stale.insert(
            WindowIdentity {
                hwnd: 1,
                pid: 2,
                class_name: "A".into(),
            },
            StaleState {
                attempts: 7,
                ..StaleState::default()
            },
        );
        caches.stale.insert(
            WindowIdentity {
                hwnd: 2,
                pid: 2,
                class_name: "A".into(),
            },
            StaleState::default(),
        );
        assert_eq!(caches.restore_failure_count(), 1);
        caches.clear_restore_failures();
        assert_eq!(caches.restore_failure_count(), 0);
    }
}
