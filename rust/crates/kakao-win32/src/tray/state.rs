use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

pub struct TrayFlags {
    pub enabled: Arc<AtomicBool>,
    pub aggressive: Arc<AtomicBool>,
    pub startup: Arc<AtomicBool>,
}

/// Live engine counters shown in the tray tooltip and menu header. Without
/// these the "복원 실패 초기화" menu item resets a number nobody can see.
#[derive(Clone, Default)]
pub struct TrayStatus {
    pub main_windows: Arc<AtomicU32>,
    pub hidden_windows: Arc<AtomicU32>,
    pub closed_windows: Arc<AtomicU32>,
    pub resized_windows: Arc<AtomicU32>,
    pub restore_failures: Arc<AtomicU32>,
    pub last_error: Arc<Mutex<String>>,
}

/// Plain snapshot so the status text can be unit-tested without Win32.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusSnapshot {
    pub enabled: bool,
    pub aggressive: bool,
    pub main_windows: u32,
    pub hidden_windows: u32,
    pub closed_windows: u32,
    pub resized_windows: u32,
    pub restore_failures: u32,
    pub last_error: String,
}

impl StatusSnapshot {
    pub(super) fn capture(flags: &TrayFlags, status: &TrayStatus) -> Self {
        Self {
            enabled: flags.enabled.load(Ordering::SeqCst),
            aggressive: flags.aggressive.load(Ordering::SeqCst),
            main_windows: status.main_windows.load(Ordering::SeqCst),
            hidden_windows: status.hidden_windows.load(Ordering::SeqCst),
            closed_windows: status.closed_windows.load(Ordering::SeqCst),
            resized_windows: status.resized_windows.load(Ordering::SeqCst),
            restore_failures: status.restore_failures.load(Ordering::SeqCst),
            last_error: status
                .last_error
                .lock()
                .map(|guard| guard.clone())
                .unwrap_or_default(),
        }
    }
}
