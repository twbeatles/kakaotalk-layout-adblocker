use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use crate::config::AppSettings;

pub struct SharedFlags {
    pub enabled: Arc<AtomicBool>,
    pub aggressive: Arc<AtomicBool>,
    pub stopping: Arc<AtomicBool>,
    pub apply: Arc<AtomicBool>,
    pub startup: Arc<AtomicBool>,
    pub reset_restore: Arc<AtomicBool>,
    /// Gauge: windows currently stuck in a failed restore (not a running total).
    pub restore_failures: Arc<AtomicU32>,
    /// Gauge: confirmed KakaoTalk main windows seen by the last evaluation.
    pub main_windows: Arc<AtomicU32>,
    /// Running total of windows this process actually hid.
    pub hidden_windows: Arc<AtomicU32>,
    /// Running total of windows confirmed destroyed after `WM_CLOSE`.
    pub closed_windows: Arc<AtomicU32>,
    /// Running total of applied main-view resizes.
    pub resized_windows: Arc<AtomicU32>,
    /// Most recent engine-level error, surfaced in the tray status.
    pub last_error: Arc<Mutex<String>>,
    /// Whether `last_error` currently holds a restore failure, so it can be
    /// cleared once restores recover without wiping an unrelated message.
    restore_error_active: AtomicBool,
}

impl SharedFlags {
    pub fn from_settings(settings: &AppSettings, apply: bool) -> Arc<Self> {
        Arc::new(Self {
            enabled: Arc::new(AtomicBool::new(settings.enabled)),
            aggressive: Arc::new(AtomicBool::new(settings.aggressive_mode)),
            stopping: Arc::new(AtomicBool::new(false)),
            apply: Arc::new(AtomicBool::new(apply)),
            startup: Arc::new(AtomicBool::new(settings.run_on_startup)),
            reset_restore: Arc::new(AtomicBool::new(false)),
            restore_failures: Arc::new(AtomicU32::new(0)),
            main_windows: Arc::new(AtomicU32::new(0)),
            hidden_windows: Arc::new(AtomicU32::new(0)),
            closed_windows: Arc::new(AtomicU32::new(0)),
            resized_windows: Arc::new(AtomicU32::new(0)),
            last_error: Arc::new(Mutex::new(String::new())),
            restore_error_active: AtomicBool::new(false),
        })
    }

    pub fn set_last_error(&self, message: &str) {
        if let Ok(mut guard) = self.last_error.lock() {
            guard.clear();
            guard.push_str(message);
        }
        self.restore_error_active.store(false, Ordering::SeqCst);
    }

    /// Record a restore failure as the current error.
    pub fn set_restore_error(&self, message: &str) {
        self.set_last_error(message);
        self.restore_error_active.store(true, Ordering::SeqCst);
    }

    /// Clear `last_error` only if it still describes a restore failure.
    pub fn clear_restore_error(&self) {
        if self.restore_error_active.swap(false, Ordering::SeqCst) {
            if let Ok(mut guard) = self.last_error.lock() {
                guard.clear();
            }
        }
    }

    /// Publish the restore-failure gauge and keep `last_error` in step with it.
    pub fn report_restore_failures(&self, failures: u32, err: &str) {
        self.restore_failures.store(failures, Ordering::SeqCst);
        if failures == 0 {
            self.clear_restore_error();
        } else if !err.is_empty() {
            self.set_restore_error(err);
        }
    }

    pub fn last_error_text(&self) -> String {
        self.last_error
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags() -> Arc<SharedFlags> {
        SharedFlags::from_settings(&AppSettings::default(), true)
    }

    #[test]
    fn recovered_restore_clears_its_own_error() {
        let flags = flags();
        flags.report_restore_failures(1, "restore show failed hwnd=1");
        assert_eq!(flags.last_error_text(), "restore show failed hwnd=1");
        flags.report_restore_failures(0, "");
        assert_eq!(flags.last_error_text(), "");
    }

    #[test]
    fn restore_recovery_keeps_an_unrelated_error() {
        let flags = flags();
        flags.report_restore_failures(1, "restore show failed hwnd=1");
        flags.set_last_error("설정 자동 복구");
        flags.report_restore_failures(0, "");
        assert_eq!(flags.last_error_text(), "설정 자동 복구");
    }
}
