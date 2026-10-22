use kakao_updater::{PreparedInstall, backup_path_for, restore_previous_version};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn temp(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kakao-transaction-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn preparation_is_non_destructive_and_commit_keeps_complete_backup() {
    let dir = temp("prepare");
    let current = dir.join("한글 app.exe");
    let replacement = dir.join("download.exe");
    fs::write(&current, b"OLD").unwrap();
    fs::write(&replacement, b"NEW").unwrap();
    fs::write(backup_path_for(&current), b"PREVIOUS-BACKUP").unwrap();
    let prepared = PreparedInstall::prepare(&current, &replacement, None).unwrap();
    assert_eq!(fs::read(&current).unwrap(), b"OLD");
    assert_eq!(
        fs::read(backup_path_for(&current)).unwrap(),
        b"PREVIOUS-BACKUP"
    );
    drop(prepared);
    assert_eq!(fs::read(&current).unwrap(), b"OLD");
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 3);
    PreparedInstall::prepare(&current, &replacement, None)
        .unwrap()
        .commit()
        .unwrap();
    assert_eq!(fs::read(&current).unwrap(), b"NEW");
    assert_eq!(fs::read(backup_path_for(&current)).unwrap(), b"OLD");
    restore_previous_version(&current).unwrap();
    assert_eq!(fs::read(&current).unwrap(), b"OLD");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn invalid_local_copy_never_touches_installed_file_or_backup() {
    let dir = temp("invalid");
    let current = dir.join("app.exe");
    let next = dir.join("next.exe");
    fs::write(&current, b"OLD").unwrap();
    fs::write(&next, b"NEW").unwrap();
    fs::write(backup_path_for(&current), b"BACKUP").unwrap();
    assert!(PreparedInstall::prepare(&current, &next, Some(&"0".repeat(64))).is_err());
    assert_eq!(fs::read(&current).unwrap(), b"OLD");
    assert_eq!(fs::read(backup_path_for(&current)).unwrap(), b"BACKUP");
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 3);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn installed_file_cannot_be_used_as_a_failed_download() {
    let dir = temp("alias");
    let current = dir.join("app.exe");
    fs::write(&current, b"INSTALLED").unwrap();
    let result = kakao_updater::update_executable_with(
        &current,
        &current,
        0,
        std::time::Duration::ZERO,
        &kakao_updater::UpdateOptions {
            expected_sha256: Some("0".repeat(64)),
            ..Default::default()
        },
    );
    assert!(result.is_err());
    assert_eq!(fs::read(&current).unwrap(), b"INSTALLED");
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(windows)]
#[test]
fn commit_sharing_failure_keeps_current_and_complete_backup() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = temp("locked");
    let current = dir.join("app.exe");
    let next = dir.join("next.exe");
    fs::write(&current, b"OLD").unwrap();
    fs::write(&next, b"NEW").unwrap();
    let prepared = PreparedInstall::prepare(&current, &next, None).unwrap();
    // Permit reads but deny replacement of this test-owned file.
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&current)
        .unwrap();
    assert!(prepared.commit().is_err());
    assert_eq!(fs::read(&current).unwrap(), b"OLD");
    assert_eq!(fs::read(backup_path_for(&current)).unwrap(), b"OLD");
    drop(lock);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 3);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn helper_protocol_and_explicit_recovery_work_without_launching_app() {
    let helper = env!("CARGO_BIN_EXE_kakao-updater");
    let output = Command::new(helper)
        .arg("--protocol-version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        kakao_updater::HELPER_PROTOCOL
    );
    let dir = temp("cli");
    let current = dir.join("app.exe");
    fs::write(&current, b"PARTIAL-OLD-HELPER").unwrap();
    fs::write(backup_path_for(&current), b"GOOD-BACKUP").unwrap();
    let status = Command::new(helper)
        .args(["--recover", "--no-relaunch", "--current"])
        .arg(&current)
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(fs::read(&current).unwrap(), b"GOOD-BACKUP");
    assert_eq!(fs::read(backup_path_for(&current)).unwrap(), b"GOOD-BACKUP");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn interrupted_child_leaves_complete_current_at_prepare_and_commit_boundaries() {
    for phase in ["prepare", "commit"] {
        let dir = temp(phase);
        let current = dir.join("app.exe");
        fs::write(&current, b"OLD").unwrap();
        fs::write(dir.join("next.exe"), b"NEW").unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "interrupted_install_child"])
            .env("KAKAO_TEST_INSTALL_DIR", &dir)
            .env("KAKAO_TEST_INSTALL_PHASE", phase)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read(&current).unwrap(),
            if phase == "prepare" { b"OLD" } else { b"NEW" }
        );
        if phase == "commit" {
            assert_eq!(fs::read(backup_path_for(&current)).unwrap(), b"OLD");
            restore_previous_version(&current).unwrap();
            assert_eq!(fs::read(&current).unwrap(), b"OLD");
        }
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn interrupted_install_child() {
    let Some(dir) = std::env::var_os("KAKAO_TEST_INSTALL_DIR").map(PathBuf::from) else {
        return;
    };
    let prepared =
        PreparedInstall::prepare(&dir.join("app.exe"), &dir.join("next.exe"), None).unwrap();
    if std::env::var("KAKAO_TEST_INSTALL_PHASE").unwrap() == "commit" {
        prepared.commit().unwrap();
    }
    // Exit deliberately bypasses Drop: simulates a terminated helper with no
    // cleanup handler, exclusively against files created by the parent test.
    std::process::exit(0);
}
