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
        if thread::panicking() {
            error!("engine worker loop panicked; ad blocking has stopped");
            self.0
                .set_last_error("엔진 워커가 중단되었습니다. 프로그램을 다시 시작해 주세요.");
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
    thread::spawn(move || {
        info!("engine worker thread started");
        let _exit_guard = WorkerExitGuard(Arc::clone(&flags));
        let mut caches = EngineCaches::new();
        let mut last_pids: HashSet<i64> = HashSet::new();
        let mut cached_pids: Vec<i64> = Vec::new();
        let mut last_pid_scan: Option<Instant> = None;
        let mut absent_since = Instant::now();
        let mut burst_left = 0u32;
        let mut last_full = Instant::now() - Duration::from_secs(10);
        let mut was_enabled = flags.enabled.load(Ordering::SeqCst);
        let mut last_event_at: Option<Instant> = None;
        let mut quiet_since = Instant::now();
        let mut consecutive_panics = 0u32;
        let burst_interval =
            Duration::from_millis(u64::from(settings.burst_scan_interval_ms.max(10)));
        #[cfg(windows)]
        let mut hook: Option<kakao_win32::event_hook::EventHook> = None;
        #[cfg(windows)]
        let mut hooked_pids: HashSet<i64> = HashSet::new();
        #[cfg(windows)]
        let mut pid_watch = kakao_win32::process::PidWatch::default();

        // Pause while still pumping WinEvent messages when a hook exists, so
        // events arriving during the pause are delivered and coalesced.
        #[cfg(windows)]
        let pause =
            |hook: &Option<kakao_win32::event_hook::EventHook>, duration: Duration| match hook
                .as_ref()
            {
                Some(hook) => hook.pump_for(duration),
                None => thread::sleep(duration),
            };

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
                // Nothing pumps the hook while disabled; release it instead of
                // letting WinEvents pile up in this thread's queue. It is
                // reinstalled on the first enabled iteration.
                #[cfg(windows)]
                {
                    hook = None;
                    hooked_pids.clear();
                }
                thread::sleep(Duration::from_millis(1000));
                continue;
            }

            #[cfg(windows)]
            let pids_alive = pid_watch.all_alive();
            #[cfg(not(windows))]
            let pids_alive = false;
            let scan_interval = pid_scan_interval(&settings, pids_alive, absent_since.elapsed());
            #[cfg(windows)]
            if last_pid_scan.is_none_or(|at| at.elapsed() >= scan_interval) {
                cached_pids = kakao_win32::process::kakaotalk_pids().into_iter().collect();
                cached_pids.sort_unstable();
                last_pid_scan = Some(Instant::now());
                pid_watch.watch(&cached_pids);
            }
            if !cached_pids.is_empty() {
                absent_since = Instant::now();
            }

            let pid_set: HashSet<i64> = cached_pids.iter().copied().collect();
            if !pid_set.is_empty() && pid_set != last_pids {
                burst_left = settings.burst_scan_iterations.max(1);
                last_pids = pid_set.clone();
                quiet_since = Instant::now();
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
                let now = Instant::now();
                last_event_at = Some(now);
                quiet_since = now;
            }

            #[cfg(windows)]
            let hook_installed = hook.is_some();
            #[cfg(not(windows))]
            let hook_installed = false;
            let active = last_event_at.is_some_and(|at| at.elapsed() < ACTIVE_WINDOW);
            let recon =
                reconcile_interval(&settings, active, quiet_since.elapsed(), hook_installed);

            // When KakaoTalk is not running, cleanup any remaining snapshots and stay idle.
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
                let until_scan = last_pid_scan.map_or(Duration::ZERO, |at| {
                    scan_interval.saturating_sub(at.elapsed())
                });
                thread::sleep(until_scan.clamp(Duration::from_millis(10), ABSENT_MAX_WAIT));
                continue;
            }

            let due_recon = last_full.elapsed() >= recon;
            let due_burst = burst_left > 0;
            if events.is_empty() && !due_recon && !due_burst {
                let remaining = recon.saturating_sub(last_full.elapsed());
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
                // Let the rest of the burst arrive, then fold it into this
                // reconciliation. Pumping (not sleeping) is what actually
                // delivers those events so they can be discarded here.
                #[cfg(windows)]
                {
                    pause(&hook, burst_interval);
                    if let Some(hook) = hook.as_ref() {
                        let _ = hook.drain();
                    }
                }
                #[cfg(not(windows))]
                thread::sleep(burst_interval);
            }

            let completed = guarded_tick(
                api.as_ref(),
                &cached_pids,
                &settings,
                &rules,
                &mut caches,
                &flags,
                &mut consecutive_panics,
            );
            last_full = Instant::now();
            if !completed && consecutive_panics >= PANIC_BACKOFF_AFTER {
                thread::sleep(PANIC_BACKOFF);
                continue;
            }
            if burst_left > 0 {
                burst_left -= 1;
                #[cfg(windows)]
                pause(&hook, burst_interval);
                #[cfg(not(windows))]
                thread::sleep(burst_interval);
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
