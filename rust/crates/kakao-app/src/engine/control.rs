//! Recovery/retry state independent of native message delivery.
use super::{EngineCaches, SharedFlags};
use kakao_win32::Win32Api;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct OffRestore {
    next: Option<Instant>,
    failures: u32,
}
impl OffRestore {
    pub fn reset(&mut self) {
        self.next = None;
        self.failures = 0;
    }
    pub fn retry(
        &mut self,
        api: &dyn Win32Api,
        caches: &mut EngineCaches,
        flags: &SharedFlags,
        now: Instant,
    ) {
        if !flags.apply.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        if caches.snapshots.is_empty() {
            flags.report_restore_failures(0, "");
            self.reset();
            return;
        }
        if self.next.is_some_and(|at| now < at) {
            return;
        }
        let (count, err) = caches.drain_restore_all(api);
        flags.report_restore_failures(count, &err);
        if count == 0 {
            self.reset();
        } else {
            self.failures = self.failures.saturating_add(1);
            let delay = Duration::from_millis(
                200u64.saturating_mul(1 << self.failures.saturating_sub(1).min(5)),
            )
            .min(Duration::from_secs(5));
            self.next = Some(now + delay);
        }
    }
    pub fn wait(&self, now: Instant) -> Duration {
        self.next.map_or(Duration::from_secs(1), |at| {
            at.saturating_duration_since(now)
        })
    }
}

#[derive(Default)]
pub(super) struct HookRetry {
    next: Option<Instant>,
    failures: u32,
}
impl HookRetry {
    pub fn reset(&mut self) {
        self.next = None;
        self.failures = 0;
    }
    pub fn due(&self, now: Instant) -> bool {
        self.next.is_none_or(|at| now >= at)
    }
    pub fn failed(&mut self, now: Instant) {
        self.failures = self.failures.saturating_add(1);
        self.next = Some(now + Duration::from_secs(1 << self.failures.saturating_sub(1).min(5)));
    }
    pub fn remaining(&self, now: Instant) -> Duration {
        self.next.map_or(Duration::from_secs(32), |at| {
            at.saturating_duration_since(now)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppSettings;
    use kakao_core::LayoutRules;
    use kakao_win32::FakeWin32;
    #[test]
    fn off_restore_recovers_without_enabling_and_reset_retries_immediately() {
        let api = FakeWin32::from_dump_json(include_str!(
            "../../../../../tests/fixtures/window_dumps/owned_popup_legacy_ad.json"
        ))
        .unwrap();
        let settings = AppSettings::default();
        let flags = SharedFlags::from_settings(&settings, true);
        let mut caches = EngineCaches::new();
        super::super::tick(
            &api,
            &api.pids(),
            &settings,
            &LayoutRules::default(),
            &mut caches,
            &flags,
        );
        flags
            .enabled
            .store(false, std::sync::atomic::Ordering::SeqCst);
        api.set_hung(527936, true);
        let now = Instant::now();
        let mut retry = OffRestore::default();
        retry.retry(&api, &mut caches, &flags, now);
        assert_eq!(
            flags
                .restore_failures
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        api.set_hung(527936, false);
        retry.retry(&api, &mut caches, &flags, now + Duration::from_millis(199));
        assert!(!api.is_window_visible(527936));
        retry.reset();
        retry.retry(&api, &mut caches, &flags, now + Duration::from_millis(199));
        assert!(api.is_window_visible(527936));
        assert!(caches.snapshots.is_empty());
        assert_eq!(
            flags
                .restore_failures
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
        assert!(flags.last_error_text().is_empty());
    }
    #[test]
    fn hook_failure_retries_same_pid_with_capped_backoff() {
        let now = Instant::now();
        let mut retry = HookRetry::default();
        assert!(retry.due(now));
        retry.failed(now);
        assert!(!retry.due(now + Duration::from_millis(999)));
        assert!(retry.due(now + Duration::from_secs(1)));
        retry.failed(now + Duration::from_secs(1));
        assert!(!retry.due(now + Duration::from_secs(2)));
        assert!(retry.due(now + Duration::from_secs(3)));
        for _ in 0..20 {
            retry.failed(now);
        }
        assert_eq!(retry.remaining(now), Duration::from_secs(32));
        retry.reset();
        assert!(retry.due(now));
    }
}
