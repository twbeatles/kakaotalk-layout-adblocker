use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const LOG_ROTATE_BYTES: u64 = 5 * 1024 * 1024;

pub fn rotated_log_path(path: &Path) -> PathBuf {
    path.with_extension("log.1")
}

pub fn rotate_log_if_needed(path: &Path) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if meta.len() <= LOG_ROTATE_BYTES {
        return;
    }
    rotate_log_now(path);
}

fn rotate_log_now(path: &Path) {
    let rotated = rotated_log_path(path);
    let _ = fs::remove_file(&rotated);
    let _ = fs::rename(path, rotated);
}

/// Append-only log file that rotates itself while the process runs.
///
/// The previous setup checked the size once at startup and then handed a plain
/// `File` to `tracing`. A tray app registered as a startup program can stay up
/// for weeks, so that check never ran again and the log grew without bound.
/// Rotating through this writer also avoids renaming a file that still has a
/// live append handle, which would keep writing into the rotated copy.
pub struct RotatingLog {
    path: PathBuf,
    max_bytes: u64,
    state: Mutex<Option<LogState>>,
}

struct LogState {
    file: fs::File,
    len: u64,
}

impl RotatingLog {
    pub fn open(path: &Path, max_bytes: u64) -> io::Result<Self> {
        rotate_log_if_needed(path);
        let state = open_append(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            max_bytes: max_bytes.max(64 * 1024),
            state: Mutex::new(Some(state)),
        })
    }

    fn write_record(&self, buf: &[u8]) -> io::Result<usize> {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let needs_rotate = guard
            .as_ref()
            .is_some_and(|state| state.len.saturating_add(buf.len() as u64) > self.max_bytes);
        if needs_rotate {
            // Drop the handle before renaming so the reopened file is the new one.
            *guard = None;
            rotate_log_now(&self.path);
        }
        if guard.is_none() {
            match open_append(&self.path) {
                Ok(state) => *guard = Some(state),
                // Losing the file must not take down logging as a whole; the
                // console layer keeps working and the next record retries.
                Err(_) => return Ok(buf.len()),
            }
        }
        let Some(state) = guard.as_mut() else {
            return Ok(buf.len());
        };
        match state.file.write(buf) {
            Ok(written) => {
                state.len = state.len.saturating_add(written as u64);
                Ok(written)
            }
            Err(err) => {
                *guard = None;
                Err(err)
            }
        }
    }

    fn flush_inner(&self) -> io::Result<()> {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match guard.as_mut() {
            Some(state) => state.file.flush(),
            None => Ok(()),
        }
    }
}

fn open_append(path: &Path) -> io::Result<LogState> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let len = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    Ok(LogState { file, len })
}

pub struct RotatingLogWriter<'a> {
    owner: &'a RotatingLog,
}

impl Write for RotatingLogWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.owner.write_record(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.owner.flush_inner()
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for RotatingLog {
    type Writer = RotatingLogWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        RotatingLogWriter { owner: self }
    }
}
