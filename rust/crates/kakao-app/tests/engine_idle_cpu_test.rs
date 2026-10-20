use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use kakao_app::config::AppSettings;
use kakao_app::engine::{SharedFlags, spawn_worker};
use kakao_core::LayoutRules;
use kakao_win32::FakeWin32;

#[test]
fn worker_stops_cleanly_without_spinning() {
    let api = Arc::new(FakeWin32::from_dump_json(r#"{"pids":[],"windows":[]}"#).unwrap());
    let settings = AppSettings {
        enabled: true,
        aggressive_mode: false,
        idle_poll_interval_ms: 200,
        pid_scan_interval_ms: 500,
        poll_interval_ms: 50,
        ..AppSettings::default()
    };
    let flags = SharedFlags::from_settings(&settings, true);
    let rules = LayoutRules::default();

    let start = Instant::now();
    let handle = spawn_worker(api, Arc::clone(&flags), settings, rules);

    // Let the worker run for 300ms
    thread::sleep(Duration::from_millis(300));
    flags.stopping.store(true, Ordering::SeqCst);

    let res = handle.join();
    assert!(res.is_ok(), "worker thread should join cleanly");
    assert!(start.elapsed() >= Duration::from_millis(300));
}

#[cfg(windows)]
#[test]
fn real_win32_worker_runs_cleanly() {
    let api = Arc::new(kakao_win32::RealWin32::new());
    let settings = AppSettings {
        enabled: true,
        aggressive_mode: false,
        idle_poll_interval_ms: 200,
        pid_scan_interval_ms: 500,
        poll_interval_ms: 50,
        ..AppSettings::default()
    };
    let flags = SharedFlags::from_settings(&settings, false); // apply = false for test safety
    let rules = LayoutRules::default();

    let start = Instant::now();
    let handle = spawn_worker(api, Arc::clone(&flags), settings, rules);

    // Let the worker run for 1000ms
    thread::sleep(Duration::from_millis(1000));
    flags.stopping.store(true, Ordering::SeqCst);

    let res = handle.join();
    assert!(res.is_ok(), "real win32 worker thread should join cleanly");
    assert!(start.elapsed() >= Duration::from_millis(1000));
}
