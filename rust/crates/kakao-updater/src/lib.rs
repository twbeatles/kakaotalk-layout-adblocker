use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use sha2::{Digest, Sha256};
use tracing::{error, info, warn};

#[derive(Debug, thiserror::Error)]
pub enum HelperError {
    #[error("현재 실행 파일이 존재하지 않습니다: {0}")]
    CurrentMissing(PathBuf),
    #[error("새 업데이트 파일이 존재하지 않습니다: {0}")]
    ReplacementMissing(PathBuf),
    #[error("새 업데이트 파일 크기가 0입니다: {0}")]
    ReplacementEmpty(PathBuf),
    #[error("상위 프로세스({0}) 대기 시간 초과")]
    WaitTimeout(u32),
    #[error("백업 생성 실패: {0}")]
    BackupFailed(String),
    #[error("실행 파일 교체 실패: {0}")]
    ReplaceFailed(String),
    #[error("새 버전 실행 실패: {0}")]
    RelaunchFailed(String),
    #[error("롤백 실패: {0}")]
    RollbackFailed(String),
    #[error("새 업데이트 파일 해시가 일치하지 않습니다: {0}")]
    ReplacementHashMismatch(String),
}

/// Options for the swap. `expected_sha256` re-verifies the staged artifact just
/// before it replaces the running EXE: the app hashed it at download time, but
/// the helper is a separate process that starts later, so nothing had checked
/// the bytes actually being installed.
#[derive(Debug, Clone, Default)]
pub struct UpdateOptions {
    pub relaunch: bool,
    pub relaunch_args: Vec<String>,
    pub expected_sha256: Option<String>,
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

pub fn wait_for_process_exit(pid: u32, timeout: Duration) -> Result<(), HelperError> {
    if pid == 0 {
        return Ok(());
    }

    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
        };

        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) };
        let Ok(handle) = handle else {
            // Process might already have exited
            return Ok(());
        };
        if handle.is_invalid() {
            return Ok(());
        }

        let timeout_ms = timeout.as_millis().min(u32::MAX as u128) as u32;
        let wait_res = unsafe { WaitForSingleObject(handle, timeout_ms) };
        let _ = unsafe { CloseHandle(handle) };

        if wait_res == WAIT_OBJECT_0 {
            Ok(())
        } else if wait_res == WAIT_TIMEOUT {
            Err(HelperError::WaitTimeout(pid))
        } else {
            // Other wait return, treat as terminated
            Ok(())
        }
    }

    #[cfg(not(windows))]
    {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            std::thread::sleep(Duration::from_millis(50));
            // Non-windows test stub
            break;
        }
        Ok(())
    }
}

pub fn update_executable(
    current: &Path,
    replacement: &Path,
    pid: u32,
    timeout: Duration,
    relaunch: bool,
) -> Result<(), HelperError> {
    update_executable_with(
        current,
        replacement,
        pid,
        timeout,
        &UpdateOptions {
            relaunch,
            ..UpdateOptions::default()
        },
    )
}

pub fn update_executable_with(
    current: &Path,
    replacement: &Path,
    pid: u32,
    timeout: Duration,
    options: &UpdateOptions,
) -> Result<(), HelperError> {
    let relaunch = options.relaunch;
    info!(
        current = %current.display(),
        replacement = %replacement.display(),
        pid,
        "starting self-update replacement"
    );

    // 1. Wait for target process termination
    wait_for_process_exit(pid, timeout)?;

    // 2. Validate paths
    if !current.exists() {
        return Err(HelperError::CurrentMissing(current.to_path_buf()));
    }
    if !replacement.exists() {
        return Err(HelperError::ReplacementMissing(replacement.to_path_buf()));
    }
    let meta = std::fs::metadata(replacement)
        .map_err(|_e| HelperError::ReplacementMissing(replacement.to_path_buf()))?;
    if meta.len() == 0 {
        return Err(HelperError::ReplacementEmpty(replacement.to_path_buf()));
    }
    if let Some(expected) = options.expected_sha256.as_deref() {
        let actual = sha256_file(replacement)
            .map_err(|err| HelperError::ReplacementHashMismatch(err.to_string()))?;
        if !actual.eq_ignore_ascii_case(expected) {
            error!(%actual, %expected, "staged update failed re-verification");
            let _ = std::fs::remove_file(replacement);
            return Err(HelperError::ReplacementHashMismatch(format!(
                "expected {expected}, got {actual}"
            )));
        }
        info!("staged update re-verified before replacement");
    }

    // 3. Prepare backup path
    let backup = current.with_extension("exe.old");
    if backup.exists() {
        let _ = std::fs::remove_file(&backup);
    }

    // 4. Rename current -> current.exe.old
    if let Err(err) = std::fs::rename(current, &backup) {
        error!(%err, "failed to rename current executable to backup");
        return Err(HelperError::BackupFailed(err.to_string()));
    }

    // 5. Move replacement -> current
    let replace_res = if let Err(err) = std::fs::rename(replacement, current) {
        // Fallback to copy and remove in case of cross-device temp folder
        match std::fs::copy(replacement, current) {
            Ok(_) => {
                let _ = std::fs::remove_file(replacement);
                Ok(())
            }
            Err(copy_err) => {
                error!(rename_err = %err, %copy_err, "failed to move replacement into place");
                Err(HelperError::ReplaceFailed(format!("{err}; {copy_err}")))
            }
        }
    } else {
        Ok(())
    };

    if let Err(replace_err) = replace_res {
        // Rollback backup -> current
        warn!("attempting rollback after replacement failure");
        if let Err(rb_err) = std::fs::rename(&backup, current) {
            error!(%rb_err, "catastrophic: rollback failed");
            return Err(HelperError::RollbackFailed(format!(
                "{replace_err}; rollback error: {rb_err}"
            )));
        }
        return Err(replace_err);
    }

    // 6. Relaunch new version if requested
    if relaunch {
        info!(args = ?options.relaunch_args, "relaunching updated application");
        // Carry the original launch flags across the update so a tray/startup
        // launch keeps behaving like one.
        let mut cmd = Command::new(current);
        cmd.args(&options.relaunch_args);
        if let Err(err) = cmd.spawn() {
            error!(%err, "failed to relaunch updated executable, attempting rollback");
            if let Err(move_back) = std::fs::rename(current, replacement) {
                warn!(%move_back, "could not move the new executable back to staging");
            }
            match std::fs::rename(&backup, current) {
                Ok(()) => info!("rolled back to the previous executable"),
                Err(rb_err) => {
                    error!(%rb_err, "catastrophic: rollback after failed relaunch failed");
                    return Err(HelperError::RollbackFailed(format!(
                        "{err}; rollback error: {rb_err}"
                    )));
                }
            }
            return Err(HelperError::RelaunchFailed(err.to_string()));
        }
    }

    // 7. Cleanup backup if possible
    let _ = std::fs::remove_file(&backup);
    info!("self-update completed successfully");
    Ok(())
}
