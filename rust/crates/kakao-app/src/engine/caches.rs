use std::collections::HashMap;
use std::time::{Duration, Instant};

use kakao_core::{CandidateState, WindowIdentity};
use kakao_win32::api::Win32Api;

use super::model::{RestoreSnapshot, StaleState};
use super::restore::restore_all;

/// Worker-owned caches. Bundled so the tick signature stays small and so the
/// cache-cleanup clock has a home.
#[derive(Default)]
pub struct EngineCaches {
    pub snapshots: HashMap<WindowIdentity, RestoreSnapshot>,
    pub states: HashMap<WindowIdentity, CandidateState>,
    pub stale: HashMap<WindowIdentity, StaleState>,
    pub(super) last_cleanup: Option<Instant>,
}

impl EngineCaches {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of windows currently stuck in a failed restore.
    pub fn restore_failure_count(&self) -> u32 {
        u32::try_from(
            self.stale
                .values()
                .filter(|state| state.attempts > 0)
                .count(),
        )
        .unwrap_or(u32::MAX)
    }

    /// Tray "복원 실패 초기화": forget the failure bookkeeping so every pending
    /// snapshot gets a fresh, immediate retry.
    pub fn clear_restore_failures(&mut self) {
        for state in self.stale.values_mut() {
            state.attempts = 0;
            state.cooldown = 0;
            state.warned = false;
        }
    }

    /// Restore everything and drop stale bookkeeping for whatever came back.
    pub fn drain_restore_all(&mut self, api: &dyn Win32Api) -> (u32, String) {
        let result = restore_all(api, &mut self.snapshots);
        let Self {
            snapshots, stale, ..
        } = self;
        stale.retain(|identity, _| snapshots.contains_key(identity));
        result
    }

    pub fn clear_transient(&mut self) {
        self.states.clear();
        self.stale.clear();
        self.last_cleanup = None;
    }

    pub(super) fn cleanup_due(&mut self, interval: Duration) -> bool {
        match self.last_cleanup {
            Some(at) if at.elapsed() < interval => false,
            _ => {
                self.last_cleanup = Some(Instant::now());
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_failure_count_is_a_gauge_not_a_running_total() {
        let mut caches = EngineCaches::new();
        assert_eq!(caches.restore_failure_count(), 0);
        caches.stale.insert(
            WindowIdentity {
                hwnd: 1,
                pid: 2,
                class_name: "A".into(),
            },
            StaleState {
                attempts: 7,
                ..StaleState::default()
            },
        );
        caches.stale.insert(
            WindowIdentity {
                hwnd: 2,
                pid: 2,
                class_name: "A".into(),
            },
            StaleState::default(),
        );
        assert_eq!(caches.restore_failure_count(), 1);
        caches.clear_restore_failures();
        assert_eq!(caches.restore_failure_count(), 0);
    }
}
