//! Every copy completes beside its destination before a replacement rename.
//! Process interruption cannot expose a half-copied installed executable.
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{HelperError, backup_path_for, sha256_file};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match fs::rename(source, destination) {
            Err(err)
                if cfg!(windows)
                    && matches!(err.raw_os_error(), Some(5 | 32 | 33))
                    && std::time::Instant::now() < deadline =>
            {
                // An exited child or virus scanner can briefly retain an image
                // mapping. Never remove destination to work around that lock.
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            result => return result,
        }
    }
}

struct PendingFile(PathBuf);
impl Drop for PendingFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn synced_copy(source: &Path, destination: &Path) -> io::Result<PendingFile> {
    let mut input = File::open(source)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let temporary = destination.with_file_name(format!(
        ".kakao-install-{}-{stamp}-{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let pending = PendingFile(temporary);
    io::copy(&mut input, &mut output)?;
    output.sync_all()?;
    Ok(pending)
}

/// Prepared files remain siblings of `current`, even if the download lives on
/// another volume. Dropping before commit leaves the installed file untouched.
pub struct PreparedInstall {
    current: PathBuf,
    replacement: PendingFile,
    backup: PendingFile,
}

impl PreparedInstall {
    pub fn prepare(
        current: &Path,
        replacement: &Path,
        expected: Option<&str>,
    ) -> Result<Self, HelperError> {
        if current == replacement || current.canonicalize().ok() == replacement.canonicalize().ok()
        {
            return Err(HelperError::ReplaceFailed(
                "replacement equals current".into(),
            ));
        }
        if fs::metadata(replacement).map_or(true, |m| m.len() == 0) {
            return Err(HelperError::ReplacementEmpty(replacement.to_path_buf()));
        }
        let local = synced_copy(replacement, current)
            .map_err(|e| HelperError::ReplaceFailed(e.to_string()))?;
        if let Some(expected) = expected {
            let actual = sha256_file(&local.0)
                .map_err(|e| HelperError::ReplacementHashMismatch(e.to_string()))?;
            if !actual.eq_ignore_ascii_case(expected) {
                return Err(HelperError::ReplacementHashMismatch(actual));
            }
        }
        let backup =
            synced_copy(current, current).map_err(|e| HelperError::BackupFailed(e.to_string()))?;
        Ok(Self {
            current: current.to_path_buf(),
            replacement: local,
            backup,
        })
    }

    pub fn commit(self) -> Result<(), HelperError> {
        // Publish a complete backup without first removing an existing one.
        replace_file(&self.backup.0, &backup_path_for(&self.current))
            .map_err(|e| HelperError::BackupFailed(e.to_string()))?;
        // Same-volume replacement: never remove current and never copy into it.
        replace_file(&self.replacement.0, &self.current)
            .map_err(|e| HelperError::ReplaceFailed(e.to_string()))
    }
}

/// Explicit recovery also handles an existing but unusable current file left
/// by older helpers. The caller must stop the installed application first.
pub fn restore_previous_version(current: &Path) -> Result<(), HelperError> {
    let backup = backup_path_for(current);
    if !backup.is_file() || fs::metadata(&backup).map_or(true, |m| m.len() == 0) {
        return Err(HelperError::RollbackFailed("usable backup missing".into()));
    }
    let local =
        synced_copy(&backup, current).map_err(|e| HelperError::RollbackFailed(e.to_string()))?;
    replace_file(&local.0, current).map_err(|e| HelperError::RollbackFailed(e.to_string()))?;
    // Keep the backup for another recovery attempt if relaunch is refused.
    Ok(())
}
