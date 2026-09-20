use std::time::{Duration, Instant};

use windows::core::w;
use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIM_ADD, NOTIFYICONDATAW};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

pub fn wait_for_shell_ready_with(
    timeout: Duration,
    poll: Duration,
    required_hits: u32,
    mut is_ready: impl FnMut() -> bool,
    mut sleep_fn: impl FnMut(Duration),
    mut elapsed: impl FnMut() -> Duration,
) -> bool {
    let timeout = timeout.max(Duration::ZERO);
    let poll = poll.max(Duration::from_millis(50));
    let required_hits = required_hits.max(1);
    let mut hits = 0u32;
    while elapsed() < timeout {
        if is_ready() {
            hits += 1;
            if hits >= required_hits {
                return true;
            }
        } else {
            hits = 0;
        }
        sleep_fn(poll);
    }
    is_ready()
}

pub fn wait_for_shell_ready(timeout: Duration, poll: Duration) -> bool {
    if shell_tray_present() {
        return true;
    }
    let start = Instant::now();
    wait_for_shell_ready_with(
        timeout,
        poll,
        2,
        shell_tray_present,
        std::thread::sleep,
        || start.elapsed(),
    )
}

pub fn notify_icon_retry_delays() -> &'static [Duration] {
    const DELAYS: &[Duration] = &[
        Duration::from_millis(100),
        Duration::from_millis(250),
        Duration::from_millis(500),
        Duration::from_millis(1000),
        Duration::from_millis(2000),
        Duration::from_millis(4000),
    ];
    DELAYS
}

pub fn continue_message_loop_without_icon() -> bool {
    true
}

pub(super) fn shell_tray_present() -> bool {
    unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }
        .ok()
        .is_some_and(|hwnd| !hwnd.0.is_null())
}

pub(super) fn try_add_notify_icon(nid: &NOTIFYICONDATAW) -> bool {
    unsafe { Shell_NotifyIconW(NIM_ADD, nid).as_bool() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_for_shell_ready_requires_two_consecutive_hits() {
        let mut calls = 0u32;
        let mut now = Duration::ZERO;
        let ok = wait_for_shell_ready_with(
            Duration::from_secs(2),
            Duration::from_millis(100),
            2,
            || {
                calls += 1;
                calls >= 2
            },
            |_| {},
            || {
                now += Duration::from_millis(100);
                now
            },
        );
        assert!(ok);
        assert!(calls >= 2);
    }

    #[test]
    fn wait_for_shell_ready_resets_hits_after_a_gap() {
        let states = [false, true, false, true, true];
        let mut index = 0usize;
        let mut now = Duration::ZERO;
        let ok = wait_for_shell_ready_with(
            Duration::from_secs(2),
            Duration::from_millis(100),
            2,
            || {
                let ready = states.get(index).copied().unwrap_or(true);
                index += 1;
                ready
            },
            |_| {},
            || {
                now += Duration::from_millis(100);
                now
            },
        );
        assert!(ok);
        assert!(index >= 5);
    }

    #[test]
    fn notify_icon_retries_cover_logon_delay() {
        let total: Duration = notify_icon_retry_delays().iter().copied().sum();
        assert!(total >= Duration::from_secs(5));
        assert!(notify_icon_retry_delays().len() >= 4);
    }

    #[test]
    fn tray_keeps_message_loop_when_notify_icon_add_fails() {
        assert!(continue_message_loop_without_icon());
    }
}
