use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use kakao_core::LayoutRules;
use kakao_win32::api::Win32Api;
use tracing::{info, warn};

use super::caches::EngineCaches;
use super::flags::SharedFlags;
use super::model::ACTIVE_WINDOW;
use super::restore::report_restore;
use super::tick::tick;
use crate::config::AppSettings;

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
