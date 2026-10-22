use kakao_updater::{HelperError, update_executable};
use std::fs;
use std::time::Duration;

#[test]
fn update_replaces_current_with_new_content() {
    let dir = std::env::temp_dir().join("kakao_updater_test_success");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let current = dir.join("app.exe");
    let replacement = dir.join("new_app.exe");

    fs::write(&current, b"OLD_VERSION_V1").unwrap();
    fs::write(&replacement, b"NEW_VERSION_V2").unwrap();

    let res = update_executable(&current, &replacement, 0, Duration::from_secs(5), false);
    assert!(res.is_ok(), "update_executable failed: {:?}", res.err());

    // Current must now contain the new content
    let content = fs::read(&current).unwrap();
    assert_eq!(content, b"NEW_VERSION_V2");

    // Old backup should be cleaned up
    let backup = dir.join("app.exe.old");
    assert!(!backup.exists());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn missing_replacement_returns_error_and_preserves_current() {
    let dir = std::env::temp_dir().join("kakao_updater_test_missing_repl");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let current = dir.join("app.exe");
    let replacement = dir.join("non_existent.exe");

    fs::write(&current, b"OLD_VERSION_V1").unwrap();

    let res = update_executable(&current, &replacement, 0, Duration::from_secs(5), false);
    assert!(matches!(res, Err(HelperError::ReplacementMissing(_))));

    // Current must be intact
    assert_eq!(fs::read(&current).unwrap(), b"OLD_VERSION_V1");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn empty_replacement_returns_error_and_preserves_current() {
    let dir = std::env::temp_dir().join("kakao_updater_test_empty_repl");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let current = dir.join("app.exe");
    let replacement = dir.join("empty.exe");

    fs::write(&current, b"OLD_VERSION_V1").unwrap();
    fs::write(&replacement, b"").unwrap(); // 0 bytes

    let res = update_executable(&current, &replacement, 0, Duration::from_secs(5), false);
    assert!(matches!(res, Err(HelperError::ReplacementEmpty(_))));

    // Current must be intact
    assert_eq!(fs::read(&current).unwrap(), b"OLD_VERSION_V1");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn missing_current_returns_error() {
    let dir = std::env::temp_dir().join("kakao_updater_test_missing_cur");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let current = dir.join("non_existent_app.exe");
    let replacement = dir.join("new_app.exe");

    fs::write(&replacement, b"NEW_VERSION").unwrap();

    let res = update_executable(&current, &replacement, 0, Duration::from_secs(5), false);
    assert!(matches!(res, Err(HelperError::CurrentMissing(_))));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn failed_update_relaunches_previous_except_on_wait_timeout() {
    // PROJECT_AUDIT 2026-09-24 §5: a failed swap left no ad blocker running.
    use kakao_updater::should_relaunch_previous;
    assert!(should_relaunch_previous(&HelperError::BackupFailed(
        "denied".into()
    )));
    assert!(should_relaunch_previous(&HelperError::ReplaceFailed(
        "denied".into()
    )));
    assert!(should_relaunch_previous(
        &HelperError::ReplacementHashMismatch("x".into())
    ));
    assert!(
        !should_relaunch_previous(&HelperError::WaitTimeout(42)),
        "the app is still running after a wait timeout"
    );
}

#[test]
fn relaunch_previous_needs_an_existing_file_and_discard_removes_staging() {
    use kakao_updater::{discard_replacement, relaunch_previous};
    let dir = std::env::temp_dir().join(format!("kakao_updater_discard_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    assert!(!relaunch_previous(&dir.join("missing.exe"), &[]));

    let staged = dir.join("staged.exe");
    fs::write(&staged, b"MZ").unwrap();
    discard_replacement(&staged);
    assert!(!staged.exists());
    discard_replacement(&staged); // idempotent
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn readiness_failure_rolls_back_and_success_releases_backup() {
    use kakao_updater::{UpdateOptions, update_executable_with};
    for mode in ["exit", "timeout", "ack"] {
        // Copy only this test executable into an isolated test-owned directory.
        let dir =
            std::env::temp_dir().join(format!("kakao_handshake_{}_{}", std::process::id(), mode));
        fs::create_dir_all(&dir).unwrap();
        let current = dir.join("app.exe");
        let replacement = dir.join("next.exe");
        fs::write(&current, b"previous-version").unwrap();
        fs::copy(std::env::current_exe().unwrap(), &replacement).unwrap();
        // Each named fixture inherits the helper-owned readiness path.
        let result = update_executable_with(
            &current,
            &replacement,
            0,
            Duration::from_secs(1),
            &UpdateOptions {
                relaunch: true,
                relaunch_args: vec![
                    "--exact".into(),
                    format!("readiness_{mode}_child"),
                    "--nocapture".into(),
                ],
                readiness_timeout: Some(Duration::from_millis(if mode == "ack" {
                    1500
                } else {
                    400
                })),
                ..UpdateOptions::default()
            },
        );
        if mode == "ack" {
            assert!(result.is_ok(), "{result:?}");
        } else {
            assert!(
                matches!(result, Err(HelperError::RelaunchFailed(_))),
                "{mode}: {result:?}"
            );
            assert_eq!(fs::read(&current).unwrap(), b"previous-version");
        }
        if mode == "ack" {
            assert!(!current.with_extension("exe.old").exists());
        } else {
            assert_eq!(
                fs::read(current.with_extension("exe.old")).unwrap(),
                b"previous-version"
            );
        }
        // Successful child may still be running; leave its isolated executable
        // for the OS temp cleanup rather than deleting a live image.
        if mode != "ack" {
            let _ = fs::remove_dir_all(&dir);
        }
    }
}

#[test]
fn readiness_exit_child() {
    if std::env::var_os("KAKAO_UPDATE_READY_FILE").is_some() {
        std::process::exit(17);
    }
}
#[test]
fn readiness_timeout_child() {
    if std::env::var_os("KAKAO_UPDATE_READY_FILE").is_some() {
        std::thread::sleep(Duration::from_secs(2));
    }
}
#[test]
fn readiness_ack_child() {
    if let Some(path) = std::env::var_os("KAKAO_UPDATE_READY_FILE") {
        std::fs::write(path, std::process::id().to_string()).unwrap();
        std::thread::sleep(Duration::from_millis(600));
        std::process::exit(0);
    }
}

// --- WinError-5 / stale-backup regression suite (rename-rename protocol) ---

#[test]
fn stale_backup_is_overwritten_not_rejected() {
    // A previous attempt may leave `<app>.exe.old` behind. The next run must
    // overwrite it, never fail with "backup already exists".
    let dir = std::env::temp_dir().join(format!("kakao_updater_stale_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let current = dir.join("app.exe");
    let replacement = dir.join("next.exe");
    let backup = current.with_extension("exe.old");
    fs::write(&current, b"OLD").unwrap();
    fs::write(&backup, b"STALE-BACKUP").unwrap();
    fs::write(&replacement, b"NEW").unwrap();

    let res = update_executable(&current, &replacement, 0, Duration::from_secs(5), false);
    assert!(res.is_ok(), "{:?}", res.err());
    assert_eq!(fs::read(&current).unwrap(), b"NEW");
    assert!(!backup.exists(), "backup must be cleaned after success");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn interrupted_swap_recovers_backup_then_installs() {
    // Crash between the two renames: current missing, backup holds the old
    // version. The next run must restore-then-swap instead of CurrentMissing.
    use kakao_updater::backup_path_for;
    let dir = std::env::temp_dir().join(format!("kakao_updater_crash_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let current = dir.join("app.exe");
    let replacement = dir.join("next.exe");
    let backup = backup_path_for(&current);
    assert_eq!(backup, current.with_extension("exe.old"));
    fs::write(&backup, b"OLD-VERSION").unwrap();
    assert!(!current.exists());
    fs::write(&replacement, b"NEW-VERSION").unwrap();

    let res = update_executable(&current, &replacement, 0, Duration::from_secs(5), false);
    assert!(res.is_ok(), "{:?}", res.err());
    assert_eq!(fs::read(&current).unwrap(), b"NEW-VERSION");
    assert!(!backup.exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn interrupted_swap_with_missing_replacement_restores_current() {
    // Even when the staged replacement is gone, recovery must put the backup
    // back so the user still has an installed exe.
    let dir =
        std::env::temp_dir().join(format!("kakao_updater_crash_norepl_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let current = dir.join("app.exe");
    let backup = current.with_extension("exe.old");
    fs::write(&backup, b"OLD-VERSION").unwrap();

    let res = update_executable(
        &current,
        &dir.join("gone.exe"),
        0,
        Duration::from_secs(5),
        false,
    );
    assert!(matches!(res, Err(HelperError::ReplacementMissing(_))));
    assert_eq!(fs::read(&current).unwrap(), b"OLD-VERSION");
    assert!(!backup.exists(), "recovery consumes the backup");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn first_move_failure_leaves_current_intact() {
    // Simulate "cannot move installed exe to backup" by occupying the backup
    // path with a directory: stale removal fails and rename onto a directory
    // fails, so the installed exe must remain untouched.
    let dir = std::env::temp_dir().join(format!("kakao_updater_firstmove_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let current = dir.join("app.exe");
    let replacement = dir.join("next.exe");
    fs::write(&current, b"OLD").unwrap();
    fs::write(&replacement, b"NEW").unwrap();
    let backup = current.with_extension("exe.old");
    fs::create_dir_all(&backup).unwrap();

    let res = update_executable(&current, &replacement, 0, Duration::from_secs(5), false);
    assert!(
        matches!(res, Err(HelperError::BackupFailed(_))),
        "expected BackupFailed, got {res:?}"
    );
    assert_eq!(fs::read(&current).unwrap(), b"OLD");
    assert!(
        replacement.exists(),
        "replacement must not be consumed on first-move failure"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn protected_env_vars_are_stripped() {
    use kakao_updater::{sanitize_command_env, should_strip_env_var};
    assert!(should_strip_env_var("KAKAO_UPDATE_READY_FILE"));
    assert!(should_strip_env_var("_MEIPASS"));
    assert!(should_strip_env_var("_PYI_APPLICATION_HOME_DIR"));
    assert!(should_strip_env_var("_PYI_ARCHIVE_FILE"));
    assert!(should_strip_env_var("_MEI_TMPDIR"));
    assert!(!should_strip_env_var("PATH"));
    assert!(!should_strip_env_var("APPDATA"));
    assert!(!should_strip_env_var("KAKAO_SANITIZE_PROBE_ACTIVE"));

    // End-to-end: a child spawned through the sanitizer must not observe
    // markers that were present on the command, mirroring inherited
    // bootloader/handshake env without mutating the shared test process env.
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.arg("--exact")
        .arg("sanitize_probe_child")
        .arg("--nocapture")
        .env("KAKAO_SANITIZE_PROBE_ACTIVE", "1")
        .env("_MEIPASS", "leaked-parent-temp")
        .env("_PYI_ARCHIVE_FILE", "leaked-archive")
        .env("KAKAO_UPDATE_READY_FILE", "leaked-handshake");
    sanitize_command_env(&mut cmd);
    // The probe marker itself must survive sanitizing.
    cmd.env("KAKAO_SANITIZE_PROBE_ACTIVE", "1");
    let status = cmd.status().expect("spawn probe child");
    assert!(status.success(), "sanitized child leaked protected env");
}

#[test]
fn sanitize_probe_child() {
    if std::env::var_os("KAKAO_SANITIZE_PROBE_ACTIVE").is_none() {
        return;
    }
    let mut leaked = Vec::new();
    for key in ["_MEIPASS", "_PYI_ARCHIVE_FILE", "KAKAO_UPDATE_READY_FILE"] {
        if std::env::var_os(key).is_some() {
            leaked.push(key);
        }
    }
    if leaked.is_empty() {
        std::process::exit(0);
    } else {
        eprintln!("leaked env into sanitized child: {leaked:?}");
        std::process::exit(1);
    }
}
