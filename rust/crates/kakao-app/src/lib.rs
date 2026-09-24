pub mod args;
pub mod config;
pub mod dialogs;
pub mod dump;
pub mod dump_cmd;
pub mod engine;
pub mod graph_build;
pub mod observability;
pub mod self_check;
pub mod startup;
pub mod startup_repair;
pub mod updater;

// `main.rs` (`kakao_app::{Args, ...}`)와의 호환을 위해 CLI 표면을 루트에 유지.
pub use args::{should_attach_parent_console, Args};

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tracing::{error, info};

use config::{ensure_runtime_files, load_rules, load_settings, runtime_paths};

/// How long shutdown waits for the engine worker to restore windows and exit.
const WORKER_STOP_TIMEOUT: Duration = Duration::from_secs(3);
use engine::{spawn_worker, tick, EngineCaches, SharedFlags};

pub fn run_with_args(args: Args) -> i32 {
    if !cfg!(windows) {
        eprintln!("This application only supports Windows.");
        return 2;
    }
    if args.dump_tree_series && args.dump_series_duration_ms > 10_000 {
        eprintln!("--dump-series-duration-ms must be <= 10000");
        return 2;
    }
    let paths = runtime_paths();
    let _ = std::fs::create_dir_all(&paths.appdata_dir);

    if args.self_check {
        observability::init_tracing(Some(&paths.log_file), "INFO");
        return self_check::run(
            args.json,
            args.self_check_report.as_deref(),
            args.strict_self_check,
        );
    }
    if args.check_update {
        return match updater::check_for_update() {
            Ok(manifest) => {
                println!("update available {}", manifest.version);
                0
            }
            Err(updater::UpdateError::NoUpdate) => {
                println!("up to date");
                0
            }
            Err(err) => {
                eprintln!("{err}");
                1
            }
        };
    }

    let bootstrap_warnings = ensure_runtime_files(&paths);
    let (mut settings, warnings) = load_settings(&paths.settings_file);
    let (rules, rule_warnings) = load_rules(&paths.rules_file);
    observability::init_tracing(Some(&paths.log_file), &settings.log_level);
    let startup_warnings: Vec<String> = bootstrap_warnings
        .into_iter()
        .chain(warnings)
        .chain(rule_warnings)
        .collect();
    for warning in &startup_warnings {
        tracing::warn!("{warning}");
    }
    if args.startup_launch || args.minimized {
        settings.start_minimized = true;
    }

    #[cfg(windows)]
    let api: Arc<dyn kakao_win32::Win32Api> = Arc::new(kakao_win32::RealWin32::new());
    #[cfg(not(windows))]
    let api: Arc<dyn kakao_win32::Win32Api> = Arc::new(
        kakao_win32::FakeWin32::from_dump_json("{\"pids\":[],\"windows\":[]}").expect("empty fake"),
    );

    #[cfg(windows)]
    let pids: Vec<i64> = kakao_win32::process::kakaotalk_pids().into_iter().collect();
    #[cfg(not(windows))]
    let pids: Vec<i64> = Vec::new();

    if args.dump_tree || args.dump_tree_series {
        let core = settings.to_core();
        return dump_cmd::run_dump_commands(api.as_ref(), &pids, &core, &rules, &paths, &args);
    }

    let diagnostic = args.shadow && !args.apply;
    let apply = args.apply || !args.shadow;
    // Keep the named mutex handle alive until this function returns so a
    // second Explorer/startup launch cannot start another apply+tray loop.
    #[cfg(windows)]
    let _instance_guard = if !diagnostic {
        match kakao_win32::single_instance::InstanceMutex::acquire() {
            Ok(guard) => Some(guard),
            Err(_) => {
                eprintln!("already running");
                #[cfg(windows)]
                {
                    // `std::env::args()` starts with argv[0]; including it made
                    // this always look like a diagnostic CLI invocation, so the
                    // message box never appeared and a second double-click
                    // exited silently on the GUI-subsystem build.
                    if !should_attach_parent_console(std::env::args().skip(1)) {
                        dialogs::show_info_box(
                            "KakaoTalk Layout AdBlocker",
                            "프로그램이 이미 실행 중입니다.",
                        );
                    }
                }
                return 0;
            }
        }
    } else {
        None
    };

    let flags = SharedFlags::from_settings(&settings, apply && !args.shadow);
    if args.shadow {
        flags
            .apply
            .store(false, std::sync::atomic::Ordering::SeqCst);
        info!("shadow mode: no Hide/Resize/Close");
        let mut caches = EngineCaches::new();
        let evaluation = tick(api.as_ref(), &pids, &settings, &rules, &mut caches, &flags);
        println!(
            "shadow main={} candidates={} hide={:?}",
            evaluation.state.main_window_count,
            evaluation.candidates.len(),
            evaluation.actions.hide
        );
        return 0;
    }

    // A self-healed settings file used to be visible only in the log.
    if let Some(summary) = observability::startup_warning_summary(&startup_warnings) {
        flags.set_last_error(&summary);
    }
    if startup_repair::adopt_registry_startup_state(
        &mut settings,
        startup::current_command().as_deref(),
    ) {
        flags
            .startup
            .store(true, std::sync::atomic::Ordering::SeqCst);
        match config::update_settings(&paths.settings_file, &settings, |s| {
            s.run_on_startup = true;
        }) {
            Ok(_) => info!("run_on_startup synced to the existing Run registration"),
            Err(err) => {
                tracing::warn!(%err, "failed to save run_on_startup synced from the registry")
            }
        }
    }
    startup_repair::maybe_repair_startup_registration(&settings);

    let worker = spawn_worker(api, flags.clone(), settings.clone(), rules);
    info!("spawn_worker completed in main thread");
    let pending_update: Arc<Mutex<Option<updater::StagedUpdate>>> = Arc::new(Mutex::new(None));
    let startup_trace = args.startup_trace.clone();
    let startup_launch = args.startup_launch;
    let minimized_requested = args.minimized || args.startup_launch;

    if let Some(exit_ms) = args.exit_after_startup_ms {
        let stopping = flags.stopping.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(exit_ms));
            stopping.store(true, std::sync::atomic::Ordering::SeqCst);
            #[cfg(windows)]
            {
                for _ in 0..50 {
                    if kakao_win32::tray::request_exit() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        });
    }
    #[cfg(windows)]
    {
        use std::sync::atomic::Ordering;

        use kakao_win32::tray::{TrayCommand, TrayFlags, TrayStatus};

        use crate::config::{update_settings, VERSION};

        let flags_for_tray = flags.clone();
        let settings_path = paths.settings_file.clone();
        let log_dir = paths.appdata_dir.clone();
        let pending_for_tray = pending_update.clone();
        let mut settings = settings;
        let tray_result = kakao_win32::tray::run_loop_with_ready(
            TrayFlags {
                enabled: flags.enabled.clone(),
                aggressive: flags.aggressive.clone(),
                startup: flags.startup.clone(),
            },
            TrayStatus {
                main_windows: flags.main_windows.clone(),
                hidden_windows: flags.hidden_windows.clone(),
                closed_windows: flags.closed_windows.clone(),
                resized_windows: flags.resized_windows.clone(),
                restore_failures: flags.restore_failures.clone(),
                last_error: flags.last_error.clone(),
            },
            move |command| match command {
                // Toggles re-read the file and change one field (update_settings)
                // so JSON edits made while running are not reverted.
                TrayCommand::ToggleEnabled => {
                    let next = !flags_for_tray.enabled.load(Ordering::SeqCst);
                    match update_settings(&settings_path, &settings, |s| s.enabled = next) {
                        Ok(written) => {
                            settings = written;
                            flags_for_tray.enabled.store(next, Ordering::SeqCst);
                        }
                        Err(err) => {
                            tracing::warn!(%err, "failed to save settings, rolling back enabled toggle");
                        }
                    }
                }
                TrayCommand::ToggleAggressive => {
                    let next = !flags_for_tray.aggressive.load(Ordering::SeqCst);
                    match update_settings(&settings_path, &settings, |s| s.aggressive_mode = next) {
                        Ok(written) => {
                            settings = written;
                            flags_for_tray.aggressive.store(next, Ordering::SeqCst);
                        }
                        Err(err) => {
                            tracing::warn!(%err, "failed to save settings, rolling back aggressive toggle");
                        }
                    }
                }
                TrayCommand::ToggleStartup => {
                    let next = !flags_for_tray.startup.load(Ordering::SeqCst);
                    if crate::startup::set_enabled(next) {
                        match update_settings(&settings_path, &settings, |s| {
                            s.run_on_startup = next
                        }) {
                            Ok(written) => {
                                settings = written;
                                flags_for_tray.startup.store(next, Ordering::SeqCst);
                            }
                            Err(err) => {
                                tracing::warn!(%err, "failed to save settings, rolling back startup toggle");
                                let _ = crate::startup::set_enabled(!next);
                            }
                        }
                    }
                }
                TrayCommand::ResetRestoreFailures => {
                    flags_for_tray.reset_restore.store(true, Ordering::SeqCst);
                    flags_for_tray.restore_failures.store(0, Ordering::SeqCst);
                }
                TrayCommand::OpenLogs => {
                    let _ = kakao_win32::tray::shell_open(&log_dir.to_string_lossy());
                }
                TrayCommand::OpenReleases => {
                    let _ = kakao_win32::tray::shell_open(
                        "https://github.com/twbeatles/kakaotalk-layout-adblocker/releases",
                    );
                }
                TrayCommand::CheckUpdate => {
                    if !updater::try_begin_update() {
                        dialogs::show_info_box(
                            "업데이트 확인",
                            "업데이트 작업이 이미 진행 중입니다.",
                        );
                        return;
                    }
                    let stopping = flags_for_tray.stopping.clone();
                    let pending = pending_for_tray.clone();
                    std::thread::spawn(move || {
                        info!("checking for updates in background thread");
                        match updater::check_for_update() {
                            Ok(manifest) => {
                                info!("update available: {}", manifest.version);
                                let msg = format!(
                                    "새 버전 v{}가 출시되었습니다.\n\n지금 업데이트를 다운로드하고 프로그램을 재시작하시겠습니까?",
                                    manifest.version
                                );
                                if !dialogs::ask_yes_no("업데이트 확인", &msg) {
                                    updater::end_update();
                                    return;
                                }
                                info!("user accepted update, preparing...");
                                match updater::prepare_update(&manifest) {
                                    Ok(staged) => {
                                        if let Ok(mut guard) = pending.lock() {
                                            *guard = Some(staged);
                                        }
                                        stopping.store(true, Ordering::SeqCst);
                                        let _ = kakao_win32::tray::request_exit();
                                    }
                                    Err(err) => {
                                        updater::end_update();
                                        error!(%err, "failed to prepare update");
                                        dialogs::show_error_box(
                                            "업데이트 실패",
                                            &format!(
                                                "업데이트 적용 중 오류가 발생했습니다:\n{err}"
                                            ),
                                        );
                                    }
                                }
                            }
                            Err(updater::UpdateError::NoUpdate) => {
                                updater::end_update();
                                info!("already running latest version");
                                dialogs::show_info_box(
                                    "업데이트 확인",
                                    &format!("현재 최신 버전(v{})을 사용 중입니다.", VERSION),
                                );
                            }
                            Err(err) => {
                                updater::end_update();
                                tracing::warn!(%err, "update check failed");
                                dialogs::show_error_box(
                                    "업데이트 확인 실패",
                                    &format!("업데이트 정보를 확인하지 못했습니다:\n{err}"),
                                );
                            }
                        }
                    });
                }
                TrayCommand::Exit => {}
            },
            {
                let startup_trace = startup_trace.clone();
                move || {
                    observability::write_startup_trace(
                        startup_trace.as_deref(),
                        startup_launch,
                        minimized_requested,
                        true,
                        "",
                    );
                }
            },
        );
        if let Err(err) = tray_result {
            tracing::warn!("tray unavailable: {err}");
            observability::write_startup_trace(
                startup_trace.as_deref(),
                startup_launch,
                minimized_requested,
                false,
                &err,
            );
            flags
                .stopping
                .store(true, std::sync::atomic::Ordering::SeqCst);
            info!("tray unavailable, stopping worker then exiting");
            match engine::join_with_timeout(worker, WORKER_STOP_TIMEOUT) {
                Some(Err(_)) => error!("engine worker panic"),
                None => tracing::warn!("engine worker did not stop in time; exiting anyway"),
                Some(Ok(_)) => {}
            }
            return 1;
        }
    }
    #[cfg(not(windows))]
    {
        match worker.join() {
            Ok(_) => return 0,
            Err(_) => {
                error!("engine worker panic");
                return 1;
            }
        }
    }
    flags
        .stopping
        .store(true, std::sync::atomic::Ordering::SeqCst);
    // Bounded: restoring windows calls ShowWindow/SetWindowPos synchronously on
    // KakaoTalk's windows, which can block while KakaoTalk is not responding.
    // Waiting forever kept an invisible process holding the single-instance
    // mutex; exiting the process ends the stuck thread instead.
    let join_ok = match engine::join_with_timeout(worker, WORKER_STOP_TIMEOUT) {
        Some(Ok(_)) => true,
        Some(Err(_)) => {
            error!("engine worker panic");
            false
        }
        None => {
            tracing::warn!(
                timeout_ms = WORKER_STOP_TIMEOUT.as_millis() as u64,
                "engine worker did not stop in time (KakaoTalk may not be responding); exiting without it"
            );
            false
        }
    };
    let staged = pending_update
        .lock()
        .ok()
        .and_then(|mut guard| guard.take());
    if let Some(staged) = staged {
        match updater::launch_helper(&staged) {
            Ok(()) => {
                info!("update helper launched after restore");
                updater::end_update();
                return if join_ok { 0 } else { 1 };
            }
            Err(err) => {
                updater::discard_staged(&staged);
                updater::end_update();
                error!(%err, "failed to launch update helper after restore");
                return 1;
            }
        }
    }
    if join_ok {
        0
    } else {
        1
    }
}
