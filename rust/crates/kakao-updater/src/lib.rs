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
    /// None preserves compatibility for library callers without the app protocol.
    pub readiness_timeout: Option<Duration>,
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
            OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
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

/// Whether the helper should start the previous version after a failed
/// update. The app has already exited by the time the helper runs, so without
/// this a failed swap (for example no write access to the install folder) left
/// the user with no ad blocker running. A wait timeout is the exception: the
/// app is still running, so starting another copy would be wrong.
pub fn should_relaunch_previous(err: &HelperError) -> bool {
    !matches!(err, HelperError::WaitTimeout(_))
}

/// Start `current` (the previous version after a failed or rolled-back
/// update). Returns whether the process was spawned. The child inherits a
/// sanitized environment so handshake/bootloader markers cannot leak.
pub fn relaunch_previous(current: &Path, args: &[String]) -> bool {
    if !current.is_file() {
        return false;
    }
    let mut cmd = Command::new(current);
    cmd.args(args);
    sanitize_command_env(&mut cmd);
    match cmd.spawn() {
        Ok(_) => {
            info!(current = %current.display(), "relaunched the previous version after a failed update");
            true
        }
        Err(err) => {
            error!(%err, "could not relaunch the previous version");
            false
        }
    }
}

/// Canonical backup next to the installed executable (`app.exe` →
/// `app.exe.old`). A single fixed name keeps recovery predictable; a stale
/// file from a previous attempt is overwritten before the swap.
pub fn backup_path_for(current: &Path) -> PathBuf {
    current.with_extension("exe.old")
}

/// Whether a child-process environment variable must be stripped before
/// spawning helper / relaunched / smoke-probe processes.
///
/// `KAKAO_UPDATE_READY_FILE` is owned by one helper→app handshake; leaking it
/// lets an unrelated child acknowledge the wrong launch. `_MEIPASS` / `_PYI_*`
/// / `_MEI_*` are PyInstaller onefile bootloader markers; a frozen child that
/// inherits them resolves bundled resources against the parent's temp dir.
pub fn should_strip_env_var(name: &str) -> bool {
    name == "KAKAO_UPDATE_READY_FILE"
        || name == "_MEIPASS"
        || name.starts_with("_PYI_")
        || name.starts_with("_MEI_")
}

/// Known bootloader/handshake keys that may have been set directly on `cmd`
/// (where wildcard enumeration is impossible). Parent-inherited `_PYI_*` /
/// `_MEI_*` keys are additionally removed via process-env enumeration above.
const KNOWN_STRIP_KEYS: &[&str] = &[
    "KAKAO_UPDATE_READY_FILE",
    "_MEIPASS",
    "_MEIPASS2",
    "_PYI_APPLICATION_HOME_DIR",
    "_PYI_ARCHIVE_FILE",
    "_PYI_PARENT_PROCESS_LEVEL",
    "_MEI_TMPDIR",
];

/// Remove handshake/bootloader markers from a child command's environment.
/// Call before adding the child's own `KAKAO_UPDATE_READY_FILE` (if any).
pub fn sanitize_command_env(cmd: &mut Command) {
    let polluted: Vec<String> = std::env::vars_os()
        .filter_map(|(k, _)| k.into_string().ok())
        .filter(|k| should_strip_env_var(k))
        .collect();
    for key in polluted {
        cmd.env_remove(&key);
    }
    // Explicitly cover markers that may have been set directly on `cmd`
    // (or absent from this process's own environment).
    for key in KNOWN_STRIP_KEYS {
        cmd.env_remove(key);
    }
}

/// A previous run may have renamed the installed exe to the backup and then
/// crashed before moving the replacement into place. In that state `current`
/// is missing while the backup still holds the previous version: restore it
/// so the next attempt starts from an installed executable again.
/// Returns `true` when a restore was performed.
pub fn recover_interrupted_swap(current: &Path) -> Result<bool, HelperError> {
    let backup = backup_path_for(current);
    if current.exists() || !backup.exists() {
        return Ok(false);
    }
    std::fs::rename(&backup, current).map_err(|err| {
        HelperError::RollbackFailed(format!(
            "interrupted update recovery failed ({} -> {}): {err}",
            backup.display(),
            current.display()
        ))
    })?;
    info!(
        current = %current.display(),
        "recovered interrupted update from backup"
    );
    Ok(true)
}

/// Remove a staged replacement that will not be installed.
pub fn discard_replacement(replacement: &Path) {
    if replacement.is_file() {
        let _ = std::fs::remove_file(replacement);
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

    // 1b. A crash between the two renames leaves `current` missing with the
    // previous version in the backup. Restore it so this run swaps from an
    // installed executable instead of failing with `CurrentMissing`.
    recover_interrupted_swap(current)?;

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

    // 3. Prepare backup path: overwrite a stale backup from a previous
    // attempt so the next run never fails with "backup already exists".
    // Removal failure is logged; the rename below then reports `BackupFailed`
    // with the installed exe still in place.
    let backup = backup_path_for(current);
    if backup.exists()
        && let Err(err) = std::fs::remove_file(&backup)
    {
        warn!(
            backup = %backup.display(),
            %err,
            "stale update backup could not be removed before swap"
        );
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
        if let Err(err) = launch_verified(&mut cmd, options.readiness_timeout) {
            error!(%err, "new app did not become ready; rolling back");
            // launch_verified reaps our child before returning an error, so
            // Windows no longer has its image locked during rollback.
            let _ = std::fs::remove_file(current);
            std::fs::rename(&backup, current)
                .map_err(|rb| HelperError::RollbackFailed(format!("{err}; {rb}")))?;
            return Err(HelperError::RelaunchFailed(err));
        }
    }

    // 7. Cleanup backup if possible
    let _ = std::fs::remove_file(&backup);
    info!("self-update completed successfully");
    Ok(())
}

/// The helper owns both this directory and the child it may stop. A readiness
/// file from another/previous process cannot acknowledge this launch.
fn launch_verified(cmd: &mut Command, timeout: Option<Duration>) -> Result<(), String> {
    let Some(timeout) = timeout else {
        return cmd.spawn().map(|_| ()).map_err(|e| e.to_string());
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("kakao-ready-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("ready");
    let result = (|| {
        sanitize_command_env(cmd);
        let mut child = cmd
            .env("KAKAO_UPDATE_READY_FILE", &path)
            .spawn()
            .map_err(|e| e.to_string())?;
        let deadline = std::time::Instant::now() + timeout;
        let mut acknowledged: Option<std::time::Instant> = None;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Err(format!("app exited before readiness: {status}")),
                Ok(None) => {}
                Err(err) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(err.to_string());
                }
            }
            if std::fs::read_to_string(&path)
                .ok()
                .is_some_and(|s| s == child.id().to_string())
            {
                let at = acknowledged.get_or_insert_with(std::time::Instant::now);
                if at.elapsed() >= Duration::from_millis(250) {
                    return Ok(());
                }
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err("app readiness timed out".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    })();
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
    result
}
