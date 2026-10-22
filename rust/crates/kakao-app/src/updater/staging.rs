use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use super::error::UpdateError;
use super::http::download_and_verify;
use super::model::{STAGING_SEQ, StagedUpdate, UPDATE_IN_PROGRESS, UpdateManifest};

pub fn try_begin_update() -> bool {
    UPDATE_IN_PROGRESS
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

pub fn end_update() {
    UPDATE_IN_PROGRESS.store(false, Ordering::SeqCst);
}

pub fn update_in_progress() -> bool {
    UPDATE_IN_PROGRESS.load(Ordering::SeqCst)
}

/// Launch flags worth carrying across an update. Diagnostic/smoke-test flags
/// (`--startup-trace`, `--exit-after-startup-ms`) are deliberately dropped.
pub fn relaunch_args_from<I, S>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .filter(|arg| matches!(arg.as_ref(), "--startup-launch" | "--minimized"))
        .map(|arg| arg.as_ref().to_string())
        .collect()
}

pub fn resolve_helper(current_exe: &Path) -> Result<PathBuf, UpdateError> {
    let current_dir = current_exe.parent().unwrap_or_else(|| Path::new("."));
    let mut candidates = vec![current_dir.join("kakao-updater.exe")];
    candidates.extend([
        current_dir.join("target/release/kakao-updater.exe"),
        current_dir.join("target/debug/kakao-updater.exe"),
        current_dir.join("../target/release/kakao-updater.exe"),
        current_dir.join("../target/debug/kakao-updater.exe"),
        current_dir.join("../../target/release/kakao-updater.exe"),
        current_dir.join("../../target/debug/kakao-updater.exe"),
    ]);
    for cand in &candidates {
        if helper_looks_valid(cand) {
            return Ok(cand.clone());
        }
    }
    Err(UpdateError::Message(
        "kakao-updater.exe 헬퍼 바이너리를 찾을 수 없습니다. 앱과 같은 폴더에 함께 두세요.".into(),
    ))
}

fn helper_looks_valid(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    bytes.len() >= 2 && bytes[0] == b'M' && bytes[1] == b'Z'
}

pub fn unique_staging_path(prefix: &str, version: &str) -> PathBuf {
    let seq = STAGING_SEQ.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "{prefix}_{version}_{}_{}_{}.exe",
        std::process::id(),
        unix_now(),
        seq
    ))
}

pub fn stage_helper(src: &Path) -> Result<PathBuf, UpdateError> {
    let dest = unique_staging_path("kakao-updater", "helper");
    std::fs::copy(src, &dest)
        .map_err(|err| UpdateError::Message(format!("업데이트 헬퍼 준비 실패: {err}")))?;
    Ok(dest)
}

/// Reject old helpers before the app exits. A hung probe is bounded and reaped.
pub fn verify_helper_protocol(helper: &Path) -> Result<(), UpdateError> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(helper);
    cmd.arg("--protocol-version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    sanitize_child_command(&mut cmd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let failed = || {
        UpdateError::Message("업데이트 헬퍼가 호환되지 않습니다. 최신 ZIP의 앱과 kakao-updater.exe를 함께 설치하세요.".into())
    };
    let mut child = cmd.spawn().map_err(|_| failed())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failed());
            }
        }
    }
    let output = child.wait_with_output().map_err(|_| failed())?;
    if output.status.success()
        && String::from_utf8_lossy(&output.stdout).trim() == "kakao-updater-protocol-2"
    {
        Ok(())
    } else {
        Err(failed())
    }
}

pub fn prepare_update(manifest: &UpdateManifest) -> Result<StagedUpdate, UpdateError> {
    let current_exe = std::env::current_exe()
        .map_err(|e| UpdateError::Message(format!("현재 실행 파일 경로 확인 실패: {e}")))?;
    let helper_src = resolve_helper(&current_exe)?;
    let helper = stage_helper(&helper_src)?;
    if let Err(err) = verify_helper_protocol(&helper) {
        let _ = std::fs::remove_file(&helper);
        return Err(err);
    }
    let replacement = unique_staging_path("KakaoTalkLayoutAdBlocker_v11_update", &manifest.version);
    if let Err(err) = download_and_verify(manifest, &replacement) {
        let _ = std::fs::remove_file(&helper);
        let _ = std::fs::remove_file(&replacement);
        return Err(err);
    }
    Ok(StagedUpdate {
        helper,
        current_exe,
        replacement,
        sha256: manifest.sha256.clone(),
        relaunch_args: relaunch_args_from(std::env::args().skip(1)),
    })
}

/// Child-process markers that must not leak into the helper. `KAKAO_UPDATE_READY_FILE`
/// belongs to one helper→app handshake; PyInstaller onefile markers would make a
/// frozen child resolve resources against the parent's temp dir.
pub fn should_strip_child_env_var(name: &str) -> bool {
    name == "KAKAO_UPDATE_READY_FILE"
        || name == "_MEIPASS"
        || name.starts_with("_PYI_")
        || name.starts_with("_MEI_")
}

fn sanitize_child_command(cmd: &mut std::process::Command) {
    let polluted: Vec<String> = std::env::vars_os()
        .filter_map(|(k, _)| k.into_string().ok())
        .filter(|k| should_strip_child_env_var(k))
        .collect();
    for key in polluted {
        cmd.env_remove(&key);
    }
    for key in [
        "KAKAO_UPDATE_READY_FILE",
        "_MEIPASS",
        "_MEIPASS2",
        "_PYI_APPLICATION_HOME_DIR",
        "_PYI_ARCHIVE_FILE",
        "_PYI_PARENT_PROCESS_LEVEL",
        "_MEI_TMPDIR",
    ] {
        cmd.env_remove(key);
    }
}

pub fn launch_helper(staged: &StagedUpdate) -> Result<(), UpdateError> {
    let pid = std::process::id();
    let mut cmd = std::process::Command::new(&staged.helper);
    sanitize_child_command(&mut cmd);
    cmd.arg("--pid")
        .arg(pid.to_string())
        .arg("--current")
        .arg(&staged.current_exe)
        .arg("--replacement")
        .arg(&staged.replacement);
    if !staged.sha256.is_empty() {
        cmd.arg("--sha256").arg(&staged.sha256);
    }
    for arg in &staged.relaunch_args {
        cmd.arg("--relaunch-arg").arg(arg);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    cmd.spawn()
        .map_err(|e| UpdateError::Message(format!("업데이트 헬퍼 실행 실패: {e}")))?;
    Ok(())
}

/// Delete staged files when the helper could not be started, so repeated
/// failures do not accumulate executables in %TEMP%.
pub fn discard_staged(staged: &StagedUpdate) {
    let _ = std::fs::remove_file(&staged.helper);
    let _ = std::fs::remove_file(&staged.replacement);
}

pub fn apply_update(manifest: &UpdateManifest) -> Result<(), UpdateError> {
    let staged = prepare_update(manifest)?;
    if let Err(err) = launch_helper(&staged) {
        discard_staged(&staged);
        return Err(err);
    }
    Ok(())
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relaunch_args_keep_launch_flags_and_drop_smoke_flags() {
        assert_eq!(
            relaunch_args_from(["--startup-launch", "--minimized"]),
            vec!["--startup-launch".to_string(), "--minimized".to_string()]
        );
        assert!(relaunch_args_from(["--startup-trace", "C:\t.json"]).is_empty());
        assert!(relaunch_args_from(["--exit-after-startup-ms", "500"]).is_empty());
        assert!(relaunch_args_from(Vec::<&str>::new()).is_empty());
    }

    #[test]
    fn unique_staging_paths_differ() {
        let left = unique_staging_path("KakaoTalkLayoutAdBlocker_v11_update", "11.1.2");
        let right = unique_staging_path("KakaoTalkLayoutAdBlocker_v11_update", "11.1.2");
        assert_ne!(left, right);
        assert!(
            left.file_name()
                .unwrap()
                .to_string_lossy()
                .contains("11.1.2")
        );
    }

    #[test]
    fn resolve_helper_rejects_temp_placeholder_and_accepts_mz() {
        let dir = std::env::temp_dir().join(format!("kakao_helper_resolve_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake_app = dir.join("app.exe");
        std::fs::write(&fake_app, b"MZ-app").unwrap();
        assert!(resolve_helper(&fake_app).is_err());

        let helper = dir.join("kakao-updater.exe");
        std::fs::write(&helper, b"MZ-helper-bytes").unwrap();
        let found = resolve_helper(&fake_app).unwrap();
        assert_eq!(found, helper);
        assert!(
            verify_helper_protocol(&helper).is_err(),
            "MZ header alone is not protocol compatibility"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn try_begin_update_is_single_flight() {
        end_update();
        assert!(try_begin_update());
        assert!(!try_begin_update());
        end_update();
        assert!(try_begin_update());
        end_update();
    }

    #[test]
    fn helper_child_env_strips_handshake_and_bootloader_markers() {
        assert!(should_strip_child_env_var("KAKAO_UPDATE_READY_FILE"));
        assert!(should_strip_child_env_var("_MEIPASS"));
        assert!(should_strip_child_env_var("_PYI_ARCHIVE_FILE"));
        assert!(should_strip_child_env_var("_MEI_TMPDIR"));
        assert!(!should_strip_child_env_var("PATH"));
        assert!(!should_strip_child_env_var("KAKAO_SANITIZE_PROBE_ACTIVE"));
    }
}
