#![cfg(windows)]

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW, TranslateMessage,
    EVENT_OBJECT_CREATE, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, EVENT_OBJECT_LOCATIONCHANGE,
    EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_SHOW, EVENT_SYSTEM_FOREGROUND, MSG, PM_REMOVE,
    QS_ALLINPUT, WINEVENT_OUTOFCONTEXT, WM_QUIT,
};

const OBJID_WINDOW: i32 = 0;
const CHILDID_SELF: i32 = 0;
const EVENT_QUEUE_CAPACITY: usize = 1024;

#[derive(Debug, Clone, Copy)]
pub struct WinEvent {
    pub hwnd: i64,
    pub event: u32,
    pub time: u32,
}

/// Replaces the sender whenever hooks are re-installed for a new PID set.
/// `OnceLock` could only ever be set once, which silently broke re-installation.
fn event_tx() -> &'static Mutex<Option<Sender<WinEvent>>> {
    static EVENT_TX: OnceLock<Mutex<Option<Sender<WinEvent>>>> = OnceLock::new();
    EVENT_TX.get_or_init(|| Mutex::new(None))
}

unsafe extern "system" fn hook_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    time: u32,
) {
    if hwnd.0.is_null() || id_object != OBJID_WINDOW || id_child != CHILDID_SELF {
        return;
    }
    // try_lock, never lock: this callback runs on the installing thread while it
    // pumps messages, and blocking here would stall that thread.
    let Ok(guard) = event_tx().try_lock() else {
        return;
    };
    if let Some(tx) = guard.as_ref() {
        let _ = tx.try_send(WinEvent {
            hwnd: hwnd.0 as isize as i64,
            event,
            time,
        });
    }
}

pub struct EventHook {
    hooks: Vec<HWINEVENTHOOK>,
    rx: Receiver<WinEvent>,
}

impl EventHook {
    /// Install hooks scoped to the given KakaoTalk PIDs.
    ///
    /// An unscoped hook (`idProcess = 0`) receives every window event in the
    /// session, including the continuous `EVENT_OBJECT_LOCATIONCHANGE` stream
    /// any dragged window produces. That traffic can fill the bounded channel
    /// and evict the KakaoTalk events the engine needs, so the hook is always
    /// bound to the processes actually being watched.
    pub fn install_for_pids(pids: &[i64]) -> Option<Self> {
        let targets: Vec<u32> = pids
            .iter()
            .filter(|pid| **pid > 0 && **pid <= i64::from(u32::MAX))
            .map(|pid| *pid as u32)
            .collect();
        if targets.is_empty() {
            return None;
        }
        let (tx, rx) = crossbeam_channel::bounded(EVENT_QUEUE_CAPACITY);
        if let Ok(mut guard) = event_tx().lock() {
            *guard = Some(tx);
        } else {
            return None;
        }
        let ranges = [
            (EVENT_OBJECT_CREATE, EVENT_OBJECT_NAMECHANGE),
            (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
        ];
        let mut hooks = Vec::new();
        for pid in targets {
            for (min, max) in ranges {
                let hook = unsafe {
                    SetWinEventHook(
                        min,
                        max,
                        None,
                        Some(hook_proc),
                        pid,
                        0,
                        WINEVENT_OUTOFCONTEXT,
                    )
                };
                if hook.0.is_null() {
                    for installed in &hooks {
                        unsafe {
                            let _ = UnhookWinEvent(*installed);
                        }
                    }
                    if let Ok(mut guard) = event_tx().lock() {
                        *guard = None;
                    }
                    return None;
                }
                hooks.push(hook);
            }
        }
        Some(Self { hooks, rx })
    }

    pub fn try_recv(&self) -> Result<WinEvent, TryRecvError> {
        self.rx.try_recv()
    }

    pub fn drain(&self) -> Vec<WinEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            events.push(event);
        }
        events
    }

    pub fn wait_message(&self, timeout: Duration) {
        unsafe {
            MsgWaitForMultipleObjects(None, false, timeout.as_millis() as u32, QS_ALLINPUT);
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

impl Drop for EventHook {
    fn drop(&mut self) {
        for hook in self.hooks.drain(..) {
            unsafe {
                let _ = UnhookWinEvent(hook);
            }
        }
        if let Ok(mut guard) = event_tx().lock() {
            *guard = None;
        }
    }
}

pub fn post_quit() {
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
    }
}

pub const EVENT_CREATE: u32 = EVENT_OBJECT_CREATE;
pub const EVENT_DESTROY: u32 = EVENT_OBJECT_DESTROY;
pub const EVENT_SHOW: u32 = EVENT_OBJECT_SHOW;
pub const EVENT_HIDE: u32 = EVENT_OBJECT_HIDE;
pub const EVENT_LOCATION: u32 = EVENT_OBJECT_LOCATIONCHANGE;
pub const EVENT_NAME: u32 = EVENT_OBJECT_NAMECHANGE;
pub const EVENT_FOREGROUND: u32 = EVENT_SYSTEM_FOREGROUND;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_requires_at_least_one_valid_pid() {
        assert!(EventHook::install_for_pids(&[]).is_none());
        assert!(EventHook::install_for_pids(&[0, -1]).is_none());
    }

    #[test]
    fn reinstall_replaces_the_sender_so_drain_keeps_working() {
        let pid = i64::from(std::process::id());
        let first = EventHook::install_for_pids(&[pid]).expect("first install");
        drop(first);
        // The old code stored the sender in a OnceLock, so this second install
        // silently kept publishing into the dropped receiver's channel.
        let second = EventHook::install_for_pids(&[pid]).expect("second install");
        assert!(second.drain().is_empty());
        let tx_present = event_tx().lock().map(|g| g.is_some()).unwrap_or(false);
        assert!(tx_present, "re-install must publish a live sender");
    }
}
