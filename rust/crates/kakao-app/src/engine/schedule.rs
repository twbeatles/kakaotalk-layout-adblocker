//! Worker cadence decisions, kept free of Win32 and clocks so they can be
//! unit-tested. None of this touches the ad-decision algorithm; it only
//! decides *when* the worker scans processes and reconciles windows.

use std::time::Duration;

use crate::config::AppSettings;

/// Full Toolhelp re-sync while every known KakaoTalk PID is alive. It only
/// exists to notice an additional KakaoTalk process; a PID that dies triggers
/// an immediate rescan through the liveness check instead.
pub const ALIVE_RESYNC_INTERVAL: Duration = Duration::from_secs(30);

/// While KakaoTalk is absent the scan interval grows with the absence:
/// `(absent for at least, scan at most this often)`.
const ABSENT_BACKOFF: [(Duration, Duration); 2] = [
    (Duration::from_secs(30), Duration::from_secs(2)),
    (Duration::from_secs(5), Duration::from_secs(1)),
];

/// Longest the worker sleeps in one go while KakaoTalk is absent, so stop and
/// disable requests are still noticed promptly.
pub const ABSENT_MAX_WAIT: Duration = Duration::from_secs(1);

/// How long KakaoTalk must stay free of window events before the idle
/// reconciliation interval starts to back off.
pub const QUIET_BEFORE_BACKOFF: Duration = Duration::from_secs(10);

/// How often to rescan processes for KakaoTalk.
///
/// A full process snapshot costs a few milliseconds. Repeating it every
/// `pid_scan_interval_ms` (200ms) for as long as KakaoTalk is closed made the
/// "KakaoTalk not running" state the most expensive one, so the interval
/// backs off to 1s after 5s and 2s after 30s of absence.
pub fn pid_scan_interval(
    settings: &AppSettings,
    pids_alive: bool,
    absent_for: Duration,
) -> Duration {
    if pids_alive {
        return ALIVE_RESYNC_INTERVAL;
    }
    let base = Duration::from_millis(u64::from(settings.pid_scan_interval_ms.max(200)));
    let floor = ABSENT_BACKOFF
        .iter()
        .find(|(after, _)| absent_for >= *after)
        .map_or(Duration::ZERO, |(_, interval)| *interval);
    base.max(floor)
}

/// How often to reconcile KakaoTalk's windows.
///
/// - active (a window event within `ACTIVE_WINDOW`): `poll_interval_ms`
/// - idle: `idle_poll_interval_ms`
/// - idle and quiet for `QUIET_BEFORE_BACKOFF`: doubles every further 10s up
///   to `idle_backoff_max_ms`, but only while the WinEvent hook is installed.
///   Without the hook polling is the only detector, so it never backs off.
pub fn reconcile_interval(
    settings: &AppSettings,
    active: bool,
    quiet_for: Duration,
    hook_installed: bool,
) -> Duration {
    let idle = Duration::from_millis(u64::from(settings.idle_poll_interval_ms.max(200)));
    if active {
        return Duration::from_millis(u64::from(settings.poll_interval_ms.max(50))).min(idle);
    }
    let max = Duration::from_millis(u64::from(settings.idle_backoff_max_ms));
    if !hook_installed || max <= idle || quiet_for < QUIET_BEFORE_BACKOFF {
        return idle;
    }
    let doublings = ((quiet_for - QUIET_BEFORE_BACKOFF).as_secs() / 10 + 1).min(16) as u32;
    idle.saturating_mul(1u32 << doublings).min(max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> AppSettings {
        AppSettings::default()
    }

    #[test]
    fn alive_kakaotalk_resyncs_rarely() {
        assert_eq!(
            pid_scan_interval(&settings(), true, Duration::ZERO),
            ALIVE_RESYNC_INTERVAL
        );
    }

    #[test]
    fn absent_kakaotalk_scan_backs_off() {
        let s = settings();
        assert_eq!(
            pid_scan_interval(&s, false, Duration::ZERO),
            Duration::from_millis(200)
        );
        assert_eq!(
            pid_scan_interval(&s, false, Duration::from_secs(4)),
            Duration::from_millis(200)
        );
        assert_eq!(
            pid_scan_interval(&s, false, Duration::from_secs(5)),
            Duration::from_secs(1)
        );
        assert_eq!(
            pid_scan_interval(&s, false, Duration::from_secs(29)),
            Duration::from_secs(1)
        );
        assert_eq!(
            pid_scan_interval(&s, false, Duration::from_secs(600)),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn a_larger_configured_scan_interval_still_wins() {
        let s = AppSettings {
            pid_scan_interval_ms: 5000,
            ..settings()
        };
        assert_eq!(
            pid_scan_interval(&s, false, Duration::from_secs(600)),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn active_uses_poll_interval() {
        assert_eq!(
            reconcile_interval(&settings(), true, Duration::ZERO, true),
            Duration::from_millis(50)
        );
    }

    #[test]
    fn idle_backs_off_to_the_cap_only_after_a_quiet_period() {
        let s = settings();
        let at = |secs| reconcile_interval(&s, false, Duration::from_secs(secs), true);
        assert_eq!(at(0), Duration::from_millis(200));
        assert_eq!(at(9), Duration::from_millis(200));
        assert_eq!(at(10), Duration::from_millis(400));
        assert_eq!(at(20), Duration::from_millis(800));
        assert_eq!(at(30), Duration::from_millis(1000));
        assert_eq!(at(3600), Duration::from_millis(1000));
    }

    #[test]
    fn no_backoff_without_the_hook_or_when_disabled() {
        let s = settings();
        assert_eq!(
            reconcile_interval(&s, false, Duration::from_secs(3600), false),
            Duration::from_millis(200)
        );
        let off = AppSettings {
            idle_backoff_max_ms: 0,
            ..settings()
        };
        assert_eq!(
            reconcile_interval(&off, false, Duration::from_secs(3600), true),
            Duration::from_millis(200)
        );
    }
}
