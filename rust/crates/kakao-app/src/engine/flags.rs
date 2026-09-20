use std::sync::atomic::{AtomicBool, AtomicU32};
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
        })
    }

    pub fn set_last_error(&self, message: &str) {
        if let Ok(mut guard) = self.last_error.lock() {
            guard.clear();
            guard.push_str(message);
        }
    }

    pub fn last_error_text(&self) -> String {
        self.last_error
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}
