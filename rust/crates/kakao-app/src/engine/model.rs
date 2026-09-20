use std::time::Duration;

use kakao_core::WindowIdentity;

/// Consecutive ticks a hidden window must go unmatched before it is restored.
pub const RESTORE_MISS_THRESHOLD: u32 = 2;
/// After this many consecutive restore failures a window stops being retried
/// every tick and falls back to the long cooldown below.
pub(super) const RESTORE_MAX_ATTEMPTS: u32 = 5;
/// ~60s at the default 200ms reconciliation. A window can become restorable
/// again later (for example when KakaoTalk's main window is reopened), so the
/// engine keeps retrying — just rarely, and without repeating the warning.
pub(super) const RESTORE_GIVEUP_COOLDOWN_TICKS: u32 = 300;
/// How long after the last WinEvent the engine treats KakaoTalk as "active"
/// and reconciles at `poll_interval_ms` instead of `idle_poll_interval_ms`.
pub(super) const ACTIVE_WINDOW: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct RestoreSnapshot {
    pub identity: WindowIdentity,
    pub was_visible: bool,
    pub rect: Option<kakao_core::Rect>,
    pub top_level: bool,
}

/// Per-window bookkeeping for the stale-hide restore path.
#[derive(Clone, Debug, Default)]
pub struct StaleState {
    /// Consecutive ticks this hidden window was not matched as an ad.
    pub miss_streak: u32,
    /// Consecutive failed restore attempts. `0` means "not currently failing".
    pub attempts: u32,
    /// Ticks to skip before the next retry (exponential backoff).
    pub cooldown: u32,
    /// Whether the failure was already reported, so it is logged only once.
    pub warned: bool,
}
