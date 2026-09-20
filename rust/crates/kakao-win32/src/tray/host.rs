use std::cell::{Cell, RefCell};
use std::mem::size_of;
use std::sync::atomic::Ordering;
use std::time::Duration;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, KillTimer, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SetTimer, SetWindowLongPtrW, TranslateMessage, CS_HREDRAW, CS_VREDRAW,
    GWLP_USERDATA, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_COMMAND, WM_CONTEXTMENU,
    WM_DESTROY, WM_RBUTTONUP, WM_TIMER, WNDCLASSW,
};

use super::command::{TrayCommand, ID_EXIT};
use super::menu::{load_app_icon, show_menu, write_tip};
use super::shell_ready::{
    continue_message_loop_without_icon, notify_icon_retry_delays, shell_tray_present,
    try_add_notify_icon, wait_for_shell_ready,
};
use super::state::{StatusSnapshot, TrayFlags, TrayStatus};
use super::status_text::status_tooltip;

const WM_TRAY: u32 = WM_APP + 1;
const TRAY_RETRY_TIMER_ID: usize = 1;
const TRAY_STATUS_TIMER_ID: usize = 2;
const TRAY_STATUS_INTERVAL_MS: u32 = 1000;
const SHELL_WAIT_TIMEOUT: Duration = Duration::from_secs(15);
const SHELL_WAIT_POLL: Duration = Duration::from_millis(500);
static TRAY_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// Tray window state.
///
/// Every field is interior-mutable so `wnd_proc` only ever takes a shared
/// reference. The previous version handed out `&mut TrayHost` and invoked the
/// user callback through it; a modal `MessageBoxW` in that callback pumps
/// messages and can re-enter `wnd_proc`, which would alias the `&mut`.
pub(super) struct TrayHost {
    pub(super) flags: TrayFlags,
    pub(super) status: TrayStatus,
    pub(super) nid: RefCell<NOTIFYICONDATAW>,
    pub(super) taskbar_created: u32,
    pub(super) icon_added: Cell<bool>,
    pub(super) pending: RefCell<Vec<TrayCommand>>,
}

pub fn run_loop<F>(flags: TrayFlags, status: TrayStatus, on_command: F) -> Result<(), String>
where
    F: FnMut(TrayCommand),
{
    unsafe { run_loop_inner(flags, status, on_command, None::<fn()>) }
}

pub fn run_loop_with_ready<F, R>(
    flags: TrayFlags,
    status: TrayStatus,
    on_command: F,
    on_ready: R,
) -> Result<(), String>
where
    F: FnMut(TrayCommand),
    R: FnOnce(),
{
    unsafe { run_loop_inner(flags, status, on_command, Some(on_ready)) }
}

pub fn request_exit() -> bool {
    let raw = TRAY_HWND.load(Ordering::SeqCst);
    if raw == 0 {
        return false;
    }
    let hwnd = HWND(raw as *mut core::ffi::c_void);
    unsafe { PostMessageW(Some(hwnd), WM_COMMAND, WPARAM(ID_EXIT as usize), LPARAM(0)) }.is_ok()
}

#[allow(clippy::too_many_lines)]
unsafe fn run_loop_inner<F, R>(
    flags: TrayFlags,
    status: TrayStatus,
    mut on_command: F,
    on_ready: Option<R>,
) -> Result<(), String>
where
    F: FnMut(TrayCommand),
    R: FnOnce(),
{
    if !shell_tray_present() {
        let _ = wait_for_shell_ready(SHELL_WAIT_TIMEOUT, SHELL_WAIT_POLL);
    }

    let instance = GetModuleHandleW(None).map_err(|err| err.to_string())?;
    let icon = load_app_icon(instance.into())?;
    let class = w!("KakaoTalkLayoutAdBlockerTray");
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        hInstance: instance.into(),
        hIcon: icon,
        lpszClassName: class,
        ..Default::default()
    };
    let _ = RegisterClassW(&wc);
    // Note: HWND_MESSAGE creates a message-only window which Shell_NotifyIconW rejects with 0x80004005 (E_FAIL).
    // Shell_NotifyIconW requires a standard top-level window (parent: None) to receive shell callbacks.
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class,
        w!("KakaoTalk Layout AdBlocker"),
        WINDOW_STYLE::default(),
        0,
        0,
        0,
        0,
        None,
        None,
        Some(instance.into()),
        None,
    )
    .map_err(|err| err.to_string())?;
    let mut nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: icon,
        ..Default::default()
    };
    write_tip(
        &mut nid,
        &status_tooltip(&StatusSnapshot::capture(&flags, &status)),
    );
    let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    let mut added = try_add_notify_icon(&nid);
    if !added {
        for delay in notify_icon_retry_delays() {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            std::thread::sleep(*delay);
            if try_add_notify_icon(&nid) {
                added = true;
                break;
            }
        }
    }
    if !added && !continue_message_loop_without_icon() {
        let err = windows::Win32::Foundation::GetLastError();
        let _ = DestroyWindow(hwnd);
        return Err(format!("Shell_NotifyIconW NIM_ADD failed: {err:?}"));
    }

    TRAY_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
    let host = TrayHost {
        flags,
        status,
        nid: RefCell::new(nid),
        taskbar_created,
        icon_added: Cell::new(added),
        pending: RefCell::new(Vec::new()),
    };
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, std::ptr::addr_of!(host) as isize);
    if !added {
        let _ = SetTimer(Some(hwnd), TRAY_RETRY_TIMER_ID, 2000, None);
    }
    let _ = SetTimer(
        Some(hwnd),
        TRAY_STATUS_TIMER_ID,
        TRAY_STATUS_INTERVAL_MS,
        None,
    );
    if let Some(on_ready) = on_ready {
        on_ready();
    }

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
        // Commands are queued by wnd_proc and run here, with no borrow of
        // TrayHost held. A modal dialog inside on_command may pump messages
        // and re-enter wnd_proc; that is now safe.
        let commands: Vec<TrayCommand> = host.pending.borrow_mut().drain(..).collect();
        for command in commands {
            on_command(command);
        }
    }

    TRAY_HWND.store(0, Ordering::SeqCst);
    let _ = Shell_NotifyIconW(NIM_DELETE, &*host.nid.borrow());
    Ok(())
}

fn refresh_tooltip(host: &TrayHost) {
    if !host.icon_added.get() {
        return;
    }
    let snapshot = StatusSnapshot::capture(&host.flags, &host.status);
    let tip = status_tooltip(&snapshot);
    let mut nid = host.nid.borrow_mut();
    if tip_matches(&nid, &tip) {
        return;
    }
    write_tip(&mut nid, &tip);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &*nid);
    }
}

fn tip_matches(nid: &NOTIFYICONDATAW, tip: &str) -> bool {
    let end = nid.szTip.iter().position(|ch| *ch == 0).unwrap_or(0);
    String::from_utf16_lossy(&nid.szTip[..end]) == tip
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let host = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const TrayHost;
    if host.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let host = unsafe { &*host };
    match msg {
        WM_TRAY => {
            let event = lparam.0 as u32;
            if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                unsafe { show_menu(hwnd, host) };
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 as u32) & 0xFFFF;
            if let Some(cmd) = TrayCommand::from_id(id) {
                host.pending.borrow_mut().push(cmd);
                if cmd == TrayCommand::Exit {
                    unsafe {
                        let _ = DestroyWindow(hwnd);
                    }
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                TRAY_HWND.store(0, Ordering::SeqCst);
                let _ = KillTimer(Some(hwnd), TRAY_RETRY_TIMER_ID);
                let _ = KillTimer(Some(hwnd), TRAY_STATUS_TIMER_ID);
                let _ = Shell_NotifyIconW(NIM_DELETE, &*host.nid.borrow());
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            match wparam.0 {
                TRAY_RETRY_TIMER_ID if !host.icon_added.get() => unsafe {
                    let added = Shell_NotifyIconW(NIM_ADD, &*host.nid.borrow()).as_bool();
                    host.icon_added.set(added);
                    if added {
                        let _ = KillTimer(Some(hwnd), TRAY_RETRY_TIMER_ID);
                    }
                },
                TRAY_STATUS_TIMER_ID => refresh_tooltip(host),
                _ => {}
            }
            LRESULT(0)
        }
        msg if msg == host.taskbar_created && msg != 0 => {
            unsafe {
                let nid = host.nid.borrow();
                let _ = Shell_NotifyIconW(NIM_DELETE, &*nid);
                let added = Shell_NotifyIconW(NIM_ADD, &*nid).as_bool();
                drop(nid);
                host.icon_added.set(added);
                if added {
                    let _ = KillTimer(Some(hwnd), TRAY_RETRY_TIMER_ID);
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
