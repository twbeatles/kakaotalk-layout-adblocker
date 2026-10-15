use std::collections::HashSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use kakao_core::LayoutRules;
use kakao_win32::api::Win32Api;
use tracing::{error, info, warn};

use super::caches::EngineCaches;
use super::flags::SharedFlags;
use super::model::ACTIVE_WINDOW;
use super::restore::report_restore;
use super::schedule::{pid_scan_interval, reconcile_interval, ABSENT_MAX_WAIT};
use super::tick::tick;
use crate::config::AppSettings;

/// Consecutive panicking ticks after which the worker pauses before retrying,
/// so a deterministic panic does not spin at the reconciliation rate.
const PANIC_BACKOFF_AFTER: u32 = 3;
const PANIC_BACKOFF: Duration = Duration::from_secs(5);

/// Reports a worker loop that died outside the per-tick panic guard. Without
/// this the tray kept showing a healthy status while nothing was blocked.
struct WorkerExitGuard(Arc<SharedFlags>);

impl Drop for WorkerExitGuard {
    fn drop(&mut self) {
        self.0.unregister_worker();
        if thread::panicking() {
            error!("engine worker loop panicked; ad blocking has stopped");
            self.0
                .set_last_error("엔진 워커가 중단되었습니다. 프로그램을 다시 시작해 주세요.");
            self.0.worker_exited.store(true, Ordering::SeqCst);
        }
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Run one `tick`, containing a panic instead of letting it kill the worker.
///
/// Restarting the thread would lose `caches.snapshots`, so windows hidden
/// before the panic could never be restored. Keeping the loop alive keeps the
/// snapshots; only the transient candidate states are discarded. Returns
/// whether the tick completed.
pub(super) fn guarded_tick(
    api: &dyn Win32Api,
    pids: &[i64],
    settings: &AppSettings,
    rules: &LayoutRules,
    caches: &mut EngineCaches,
    flags: &SharedFlags,
    consecutive_panics: &mut u32,
) -> bool {
    match catch_unwind(AssertUnwindSafe(|| {
        tick(api, pids, settings, rules, caches, flags)
    })) {
        Ok(_) => {
            *consecutive_panics = 0;
            true
        }
        Err(payload) => {
            *consecutive_panics = consecutive_panics.saturating_add(1);
            let message = panic_message(payload.as_ref());
            error!(
                panic = %message,
                consecutive = *consecutive_panics,
                "engine tick panicked; snapshots kept, continuing"
            );
            flags.set_last_error(&format!("엔진 오류(자동 복구 중): {message}"));
            caches.states.clear();
            false
        }
    }
}

/// Wait for `handle` for at most `timeout`.
///
/// Returns `None` when the thread is still running. The caller then exits the
/// process anyway, which ends the stuck thread. A worker can be stuck in a
/// synchronous `ShowWindow`/`SetWindowPos` on a not-responding KakaoTalk, and
/// an unbounded `join` kept an invisible process holding the single-instance
/// mutex until KakaoTalk recovered.
pub fn join_with_timeout<T>(
    handle: thread::JoinHandle<T>,
    timeout: Duration,
) -> Option<thread::Result<T>> {
    let deadline = Instant::now() + timeout;
    while !handle.is_finished() {
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(10));
    }
    Some(handle.join())
}

pub fn spawn_worker(
    api: Arc<dyn Win32Api>,
    flags: Arc<SharedFlags>,
    settings: AppSettings,
    rules: LayoutRules,
) -> thread::JoinHandle<EngineCaches> {
    spawn_with_runtime(
        api,
        flags,
        settings,
        rules,
        super::runtime::NativeRuntime::default,
    )
}

fn spawn_with_runtime<R: super::runtime::WorkerRuntime + 'static>(
    api: Arc<dyn Win32Api>,
    flags: Arc<SharedFlags>,
    settings: AppSettings,
    rules: LayoutRules,
    runtime: impl FnOnce() -> R + Send + 'static,
) -> thread::JoinHandle<EngineCaches> {
    thread::spawn(move || {
        let _exit_guard = WorkerExitGuard(Arc::clone(&flags));
        run_worker(api.as_ref(), &flags, &settings, &rules, runtime())
    })
}

fn run_worker(
    api: &dyn Win32Api,
    flags: &SharedFlags,
    settings: &AppSettings,
    rules: &LayoutRules,
    mut runtime: impl super::runtime::WorkerRuntime,
) -> EngineCaches {
    use super::control::{HookRetry, OffRestore};
    flags.register_worker();
    info!("engine worker thread started");
    let mut caches = EngineCaches::new();
    let mut cached_pids = Vec::new();
    let mut process_scan_failed = false;
    let mut last_pid_scan: Option<Instant> = None;
    let mut absent_since = Instant::now();
    let mut last_pids = HashSet::new();
    let mut hook_pids = HashSet::new();
    let mut hook_retry = HookRetry::default();
    let mut off_restore = OffRestore::default();
    let mut burst_left = 0u32;
    let mut pending_event = false;
    let mut last_full = Instant::now() - Duration::from_secs(10);
    let mut was_enabled = flags.enabled.load(Ordering::SeqCst);
    let mut was_aggressive = flags.aggressive.load(Ordering::SeqCst);
    let mut last_event_at: Option<Instant> = None;
    let mut quiet_since = Instant::now();
    let mut consecutive_panics = 0u32;
    let burst_interval =
        Duration::from_millis(u64::from(settings.burst_scan_interval_ms.clamp(10, 1000)));

    while !flags.stopping.load(Ordering::SeqCst) {
        let reset = flags.reset_restore.swap(false, Ordering::SeqCst);
        if reset {
            caches.clear_restore_failures();
            off_restore.reset();
        }
        let enabled = flags.enabled.load(Ordering::SeqCst);
        let aggressive = flags.aggressive.load(Ordering::SeqCst);
        if enabled != was_enabled || aggressive != was_aggressive {
            last_full = Instant::now() - Duration::from_secs(10);
            if enabled && !was_enabled {
                last_pid_scan = None;
            }
            off_restore.reset();
        }
        was_enabled = enabled;
        was_aggressive = aggressive;
        if !enabled {
            runtime.remove();
            hook_pids.clear();
            hook_retry.reset();
            flags.hook_degraded.store(false, Ordering::SeqCst);
            off_restore.retry(api, &mut caches, flags, Instant::now());
            flags.worker_ready.store(true, Ordering::SeqCst);
            runtime.wait(flags, off_restore.wait(Instant::now()));
            continue;
        }
        let scan_interval = pid_scan_interval(settings, runtime.alive(), absent_since.elapsed());
        if last_pid_scan.is_none_or(|at| at.elapsed() >= scan_interval) {
            let scan_started = Instant::now();
            match runtime.scan() {
                Some(mut found) => {
                    found.sort_unstable();
                    cached_pids = found;
                    if process_scan_failed {
                        flags.clear_error_if("프로세스 탐색 실패: 다음 주기에 재시도합니다.");
                    }
                    process_scan_failed = false;
                    runtime.watch(&cached_pids);
                }
                None => {
                    process_scan_failed = true;
                    warn!("process scan failed; retaining known PIDs");
                    flags.set_last_error("프로세스 탐색 실패: 다음 주기에 재시도합니다.");
                }
            }
            last_pid_scan = Some(Instant::now());
            tracing::debug!(target: "kakao_latency", scan_us = scan_started.elapsed().as_micros() as u64, pid_count = cached_pids.len(), "PID scan");
        }
        if !cached_pids.is_empty() {
            absent_since = Instant::now();
        }
        let pid_set: HashSet<i64> = cached_pids.iter().copied().collect();
        if pid_set != last_pids {
            burst_left = settings.burst_scan_iterations.clamp(1, 10);
            last_pids = pid_set.clone();
            quiet_since = Instant::now();
        }
        if pid_set != hook_pids {
            runtime.remove();
            hook_retry.reset();
            hook_pids = pid_set.clone();
            if pid_set.is_empty() {
                flags.hook_degraded.store(false, Ordering::SeqCst);
            }
        }
        if !pid_set.is_empty() && !runtime.hooked() && hook_retry.due(Instant::now()) {
            if runtime.install(&cached_pids) {
                hook_retry.reset();
                if flags.hook_degraded.swap(false, Ordering::SeqCst) {
                    info!("WinEvent hook recovered");
                }
            } else {
                hook_retry.failed(Instant::now());
                flags.hook_degraded.store(true, Ordering::SeqCst);
                warn!("WinEvent hook unavailable; polling, automatic retry scheduled");
            }
        }
        let mut events = runtime.events();
        events.retain(|event| pid_set.contains(&api.get_window_thread_process_id(event.hwnd)));
        if !events.is_empty() {
            pending_event = true;
            let now = Instant::now();
            last_event_at = Some(now);
            quiet_since = now;
            if events.iter().any(|event| event.follow_up) {
                burst_left = settings.burst_scan_iterations.clamp(1, 10);
            }
        }
        let active = last_event_at.is_some_and(|at| at.elapsed() < ACTIVE_WINDOW);
        let recon = reconcile_interval(settings, active, quiet_since.elapsed(), runtime.hooked());
        if cached_pids.is_empty() {
            off_restore.retry(api, &mut caches, flags, Instant::now());
            caches.clear_transient();
            last_event_at = None;
            let wait = last_pid_scan.map_or(Duration::ZERO, |at| {
                scan_interval.saturating_sub(at.elapsed())
            });
            flags.worker_ready.store(true, Ordering::SeqCst);
            runtime.wait(
                flags,
                wait.clamp(Duration::from_millis(10), ABSENT_MAX_WAIT),
            );
            continue;
        }
        let due_burst = burst_left > 0 && last_full.elapsed() >= burst_interval;
        // Coalesce subsequent traffic at the burst cadence without delaying
        // the first event after quiet. Remember events across early wakeups.
        if (pending_event && last_full.elapsed() < burst_interval)
            || (!pending_event && last_full.elapsed() < recon && !due_burst)
        {
            let mut wait = if pending_event || burst_left > 0 {
                burst_interval.saturating_sub(last_full.elapsed())
            } else {
                recon.saturating_sub(last_full.elapsed())
            };
            if !runtime.hooked() {
                wait = wait.min(hook_retry.remaining(Instant::now()));
            }
            runtime.wait(flags, wait.max(Duration::from_millis(1)));
            continue;
        }
        pending_event = false;
        // First event scan is immediate. Follow-ups wait AFTER applying, not
        // before the first hide; weak confirmations still occupy distinct ticks.
        let tick_started = Instant::now();
        let completed = guarded_tick(
            api,
            &cached_pids,
            settings,
            rules,
            &mut caches,
            flags,
            &mut consecutive_panics,
        );
        if completed {
            flags.successful_ticks.fetch_add(1, Ordering::SeqCst);
            flags.worker_ready.store(true, Ordering::SeqCst);
        }
        tracing::debug!(target: "kakao_latency", event_count = events.len(), oldest_event_age_ms = events.iter().map(|e| e.age_ms).max().unwrap_or(0), tick_us = tick_started.elapsed().as_micros() as u64, hook_installed = runtime.hooked(), "reconcile latency");
        last_full = Instant::now();
        if !completed && consecutive_panics >= PANIC_BACKOFF_AFTER {
            runtime.wait(flags, PANIC_BACKOFF);
            continue;
        }
        if burst_left > 0 {
            burst_left -= 1;
            runtime.wait(flags, burst_interval);
        }
    }
    if flags.apply.load(Ordering::SeqCst) {
        let (failures, err) = caches.drain_restore_all_final(api);
        report_restore(flags, failures, &err, "restore on stop had failures");
    }
    flags.worker_exited.store(true, Ordering::SeqCst);
    caches
}

#[cfg(test)]
mod tests {
    use super::*;
    use kakao_win32::FakeWin32;

    fn owned_popup_fixture() -> (FakeWin32, Vec<i64>) {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/window_dumps/owned_popup_legacy_ad.json");
        let text = std::fs::read_to_string(path).expect("fixture");
        let api = FakeWin32::from_dump_json(&text).expect("fake");
        let pids = api.pids();
        (api, pids)
    }

    #[test]
    fn a_panicking_tick_keeps_snapshots_and_reports_the_error() {
        let (api, pids) = owned_popup_fixture();
        let settings = AppSettings::default();
        let flags = SharedFlags::from_settings(&settings, true);
        let rules = LayoutRules::default();
        let mut caches = EngineCaches::new();
        let mut panics = 0;

        assert!(guarded_tick(
            &api,
            &pids,
            &settings,
            &rules,
            &mut caches,
            &flags,
            &mut panics
        ));
        let hidden = caches.snapshots.len();
        assert!(hidden > 0, "fixture must hide the ad host");

        api.set_panic_on_enum_windows(true);
        assert!(!guarded_tick(
            &api,
            &pids,
            &settings,
            &rules,
            &mut caches,
            &flags,
            &mut panics
        ));
        assert_eq!(panics, 1);
        assert_eq!(
            caches.snapshots.len(),
            hidden,
            "snapshots must survive a panic"
        );
        assert!(flags.last_error_text().contains("엔진 오류"));

        api.set_panic_on_enum_windows(false);
        assert!(guarded_tick(
            &api,
            &pids,
            &settings,
            &rules,
            &mut caches,
            &flags,
            &mut panics
        ));
        assert_eq!(panics, 0);
    }

    #[test]
    fn join_with_timeout_gives_up_on_a_stuck_thread() {
        let stuck = thread::spawn(|| thread::sleep(Duration::from_secs(5)));
        let started = Instant::now();
        assert!(join_with_timeout(stuck, Duration::from_millis(100)).is_none());
        assert!(started.elapsed() < Duration::from_secs(2));

        let quick = thread::spawn(|| 7);
        assert_eq!(
            join_with_timeout(quick, Duration::from_secs(2)).map(|r| r.ok()),
            Some(Some(7))
        );
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::super::runtime::{WindowEvent, WorkerRuntime};
    use super::*;
    use kakao_win32::FakeWin32;
    use std::sync::mpsc;
    struct Runtime {
        pids: Vec<i64>,
        hooked: bool,
        attempts: u32,
        tx: mpsc::Sender<u32>,
    }
    impl WorkerRuntime for Runtime {
        fn scan(&mut self) -> Option<Vec<i64>> {
            Some(self.pids.clone())
        }
        fn alive(&self) -> bool {
            true
        }
        fn watch(&mut self, _: &[i64]) {}
        fn install(&mut self, _: &[i64]) -> bool {
            self.attempts += 1;
            self.hooked = self.attempts >= 3;
            let _ = self.tx.send(self.attempts);
            self.hooked
        }
        fn remove(&mut self) {
            self.hooked = false;
        }
        fn hooked(&self) -> bool {
            self.hooked
        }
        fn events(&self) -> Vec<WindowEvent> {
            Vec::new()
        }
        fn wait(&mut self, flags: &SharedFlags, timeout: Duration) {
            flags.wait_for_control(timeout);
        }
    }
    fn wait_until(mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !predicate() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        }
    }
    fn off_restore_scenario(reset: bool) {
        let api = Arc::new(
            FakeWin32::from_dump_json(include_str!(
                "../../../../../tests/fixtures/window_dumps/owned_popup_legacy_ad.json"
            ))
            .unwrap(),
        );
        let settings = AppSettings::default();
        let flags = SharedFlags::from_settings(&settings, true);
        let (tx, _) = mpsc::channel();
        let pids = api.pids();
        let handle = spawn_with_runtime(
            api.clone(),
            flags.clone(),
            settings,
            LayoutRules::default(),
            move || Runtime {
                pids,
                tx,
                hooked: false,
                attempts: 0,
            },
        );
        wait_until(|| !api.is_window_visible(527936));
        api.set_hung(527936, true);
        flags.enabled.store(false, Ordering::SeqCst);
        flags.notify_control();
        wait_until(|| flags.restore_failures.load(Ordering::SeqCst) == 1);
        let hidden = flags.hidden_windows.load(Ordering::SeqCst);
        api.set_hung(527936, false);
        if reset {
            flags.reset_restore.store(true, Ordering::SeqCst);
            flags.notify_control();
        }
        wait_until(|| api.is_window_visible(527936));
        assert_eq!(flags.hidden_windows.load(Ordering::SeqCst), hidden);
        assert_eq!(flags.restore_failures.load(Ordering::SeqCst), 0);
        flags.stopping.store(true, Ordering::SeqCst);
        flags.notify_control();
        assert!(join_with_timeout(handle, Duration::from_secs(1))
            .unwrap()
            .is_ok());
    }
    #[test]
    fn real_worker_off_reset_recovers_without_new_hides() {
        off_restore_scenario(true);
    }
    #[test]
    fn real_worker_off_automatically_retries_without_reset() {
        off_restore_scenario(false);
    }
    #[test]
    fn worker_recovers_hook_after_two_failures_with_identical_pids() {
        let api = Arc::new(
            FakeWin32::from_dump_json(include_str!(
                "../../../../../tests/fixtures/window_dumps/owned_popup_legacy_ad.json"
            ))
            .unwrap(),
        );
        let settings = AppSettings::default();
        let flags = SharedFlags::from_settings(&settings, false);
        let (tx, rx) = mpsc::channel();
        let pids = api.pids();
        let handle = spawn_with_runtime(
            api,
            flags.clone(),
            settings,
            LayoutRules::default(),
            move || Runtime {
                pids,
                tx,
                hooked: false,
                attempts: 0,
            },
        );
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), 1);
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), 2);
        assert_eq!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), 3);
        wait_until(|| !flags.hook_degraded.load(Ordering::SeqCst));
        flags.stopping.store(true, Ordering::SeqCst);
        flags.notify_control();
        assert!(join_with_timeout(handle, Duration::from_secs(1))
            .unwrap()
            .is_ok());
    }
}

#[cfg(test)]
mod event_tests {
    use super::super::runtime::{WindowEvent, WorkerRuntime};
    use super::*;
    use kakao_win32::FakeWin32;
    use std::cell::Cell;
    struct LateSignal {
        api: Arc<FakeWin32>,
        events: Cell<bool>,
        waits: u32,
    }
    impl WorkerRuntime for LateSignal {
        fn scan(&mut self) -> Option<Vec<i64>> {
            Some(self.api.pids())
        }
        fn alive(&self) -> bool {
            true
        }
        fn watch(&mut self, _: &[i64]) {}
        fn install(&mut self, _: &[i64]) -> bool {
            true
        }
        fn remove(&mut self) {}
        fn hooked(&self) -> bool {
            true
        }
        fn events(&self) -> Vec<WindowEvent> {
            if self.events.replace(false) {
                vec![WindowEvent {
                    hwnd: 102,
                    age_ms: 0,
                    follow_up: true,
                }]
            } else {
                Vec::new()
            }
        }
        fn wait(&mut self, flags: &SharedFlags, timeout: Duration) {
            self.waits += 1;
            if self.waits == 1 {
                assert_eq!(
                    flags.successful_ticks.load(Ordering::SeqCst),
                    1,
                    "first scan must precede any burst wait"
                );
                assert!(self.api.is_window_visible(102));
                self.api.set_text(103, "Advertisement");
                thread::sleep(timeout);
            } else {
                assert!(
                    !self.api.is_window_visible(102),
                    "follow-up must detect late signal without another event"
                );
                flags.stopping.store(true, Ordering::SeqCst);
            }
        }
    }
    #[test]
    fn first_event_scan_is_immediate_and_followup_detects_late_text() {
        let api = Arc::new(
            FakeWin32::from_dump_json(include_str!(
                "../../../../../tests/fixtures/window_dumps/launch_ad_flash_banner.json"
            ))
            .unwrap(),
        );
        api.set_text(103, "");
        let settings = AppSettings::default();
        let flags = SharedFlags::from_settings(&settings, true);
        let runtime = LateSignal {
            api: api.clone(),
            events: Cell::new(true),
            waits: 0,
        };
        run_worker(
            api.as_ref(),
            &flags,
            &settings,
            &LayoutRules::default(),
            runtime,
        );
    }
}
