use kakao_updater::{update_executable, HelperError};
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
