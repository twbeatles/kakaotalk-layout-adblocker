#![cfg(windows)]

use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, FindWindowW, GetCursorPos, GetMessageW, GetWindowLongPtrW, KillTimer,
    LoadIconW, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
    SetForegroundWindow, SetTimer, SetWindowLongPtrW, TrackPopupMenu, TranslateMessage, CS_HREDRAW,
    CS_VREDRAW, GWLP_USERDATA, IDI_APPLICATION, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    MSG, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_COMMAND, WM_CONTEXTMENU,
    WM_DESTROY, WM_RBUTTONUP, WM_TIMER, WNDCLASSW,
};

// MAKEINTRESOURCE(1): first ICON resource embedded by kakao-app/build.rs.
#[allow(clippy::manual_dangling_ptr)]
fn app_icon_resource() -> PCWSTR {
    PCWSTR(1usize as *const u16)
}

const WM_TRAY: u32 = WM_APP + 1;
const TRAY_RETRY_TIMER_ID: usize = 1;
const SHELL_WAIT_TIMEOUT: Duration = Duration::from_secs(15);
const SHELL_WAIT_POLL: Duration = Duration::from_millis(500);
static TRAY_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
pub const ID_TOGGLE_ENABLED: u32 = 1001;
pub const ID_TOGGLE_AGGRESSIVE: u32 = 1002;
pub const ID_TOGGLE_STARTUP: u32 = 1003;
pub const ID_RESET_RESTORE: u32 = 1004;
pub const ID_OPEN_LOGS: u32 = 1005;
pub const ID_OPEN_RELEASES: u32 = 1006;
pub const ID_CHECK_UPDATE: u32 = 1007;
pub const ID_EXIT: u32 = 1008;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    ToggleEnabled,
    ToggleAggressive,
    ToggleStartup,
    ResetRestoreFailures,
    OpenLogs,
    OpenReleases,
    CheckUpdate,
    Exit,
}

impl TrayCommand {
    fn from_id(id: u32) -> Option<Self> {
        match id {
            ID_TOGGLE_ENABLED => Some(Self::ToggleEnabled),
            ID_TOGGLE_AGGRESSIVE => Some(Self::ToggleAggressive),
            ID_TOGGLE_STARTUP => Some(Self::ToggleStartup),
            ID_RESET_RESTORE => Some(Self::ResetRestoreFailures),
            ID_OPEN_LOGS => Some(Self::OpenLogs),
            ID_OPEN_RELEASES => Some(Self::OpenReleases),
            ID_CHECK_UPDATE => Some(Self::CheckUpdate),
            ID_EXIT => Some(Self::Exit),
            _ => None,
        }
    }
}

pub struct TrayFlags {
    pub enabled: Arc<AtomicBool>,
    pub aggressive: Arc<AtomicBool>,
    pub startup: Arc<AtomicBool>,
}

struct TrayHost<F>
where
    F: FnMut(TrayCommand),
{
    flags: TrayFlags,
    on_command: F,
    nid: NOTIFYICONDATAW,
    taskbar_created: u32,
    icon_added: bool,
}

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

fn shell_tray_present() -> bool {
    unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }
        .ok()
        .is_some_and(|hwnd| !hwnd.0.is_null())
}

fn try_add_notify_icon(nid: &NOTIFYICONDATAW) -> bool {
    unsafe { Shell_NotifyIconW(NIM_ADD, nid).as_bool() }
}

pub fn run_loop<F>(flags: TrayFlags, on_command: F) -> Result<(), String>
where
    F: FnMut(TrayCommand),
{
    unsafe { run_loop_inner(flags, on_command, None::<fn()>) }
}

pub fn run_loop_with_ready<F, R>(flags: TrayFlags, on_command: F, on_ready: R) -> Result<(), String>
where
    F: FnMut(TrayCommand),
    R: FnOnce(),
{
    unsafe { run_loop_inner(flags, on_command, Some(on_ready)) }
}

pub fn request_exit() -> bool {
    let raw = TRAY_HWND.load(Ordering::SeqCst);
    if raw == 0 {
        return false;
    }
    let hwnd = HWND(raw as *mut core::ffi::c_void);
    unsafe { PostMessageW(Some(hwnd), WM_COMMAND, WPARAM(ID_EXIT as usize), LPARAM(0)) }.is_ok()
}

unsafe fn run_loop_inner<F, R>(
    flags: TrayFlags,
    on_command: F,
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
        lpfnWndProc: Some(wnd_proc::<F>),
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
    write_tip(&mut nid, "KakaoTalk Layout AdBlocker");
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
    let mut host = TrayHost {
        flags,
        on_command,
        nid,
        taskbar_created,
        icon_added: added,
    };
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, std::ptr::addr_of_mut!(host) as isize);
    if !added {
        let _ = SetTimer(Some(hwnd), TRAY_RETRY_TIMER_ID, 2000, None);
    }
    if let Some(on_ready) = on_ready {
        on_ready();
    }

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    TRAY_HWND.store(0, Ordering::SeqCst);
    let _ = Shell_NotifyIconW(NIM_DELETE, &host.nid);
    Ok(())
}

unsafe extern "system" fn wnd_proc<F: FnMut(TrayCommand)>(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let host = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut TrayHost<F>;
    if host.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    match msg {
        WM_TRAY => {
            let event = lparam.0 as u32;
            if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                unsafe { show_menu(hwnd, &mut *host) };
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 as u32) & 0xFFFF;
            if let Some(cmd) = TrayCommand::from_id(id) {
                let exit = cmd == TrayCommand::Exit;
                (unsafe { &mut *host }.on_command)(cmd);
                if exit {
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
                let _ = Shell_NotifyIconW(NIM_DELETE, &(*host).nid);
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            unsafe {
                if wparam.0 == TRAY_RETRY_TIMER_ID && !(*host).icon_added {
                    let added = Shell_NotifyIconW(NIM_ADD, &(*host).nid).as_bool();
                    (*host).icon_added = added;
                    if added {
                        let _ = KillTimer(Some(hwnd), TRAY_RETRY_TIMER_ID);
                    }
                }
            }
            LRESULT(0)
        }
        msg if msg == unsafe { (*host).taskbar_created } && msg != 0 => {
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &(*host).nid);
                let added = Shell_NotifyIconW(NIM_ADD, &(*host).nid).as_bool();
                (*host).icon_added = added;
                if added {
                    let _ = KillTimer(Some(hwnd), TRAY_RETRY_TIMER_ID);
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

unsafe fn show_menu<F: FnMut(TrayCommand)>(hwnd: HWND, host: &mut TrayHost<F>) {
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    append(menu, 0, "KakaoTalk Layout AdBlocker", false, false);
    append_sep(menu);
    let enabled = host.flags.enabled.load(Ordering::SeqCst);
    append(
        menu,
        ID_TOGGLE_ENABLED,
        if enabled {
            "차단 끄기"
        } else {
            "차단 켜기"
        },
        false,
        true,
    );
    append(
        menu,
        ID_TOGGLE_AGGRESSIVE,
        "공격 모드",
        host.flags.aggressive.load(Ordering::SeqCst),
        true,
    );
    append(
        menu,
        ID_TOGGLE_STARTUP,
        "시작프로그램 등록",
        host.flags.startup.load(Ordering::SeqCst),
        true,
    );
    append(menu, ID_RESET_RESTORE, "복원 실패 초기화", false, true);
    append_sep(menu);
    append(menu, ID_OPEN_LOGS, "로그 폴더 열기", false, true);
    append(menu, ID_OPEN_RELEASES, "GitHub 릴리스 열기", false, true);
    append(menu, ID_CHECK_UPDATE, "업데이트 확인", false, true);
    append_sep(menu);
    append(menu, ID_EXIT, "종료", false, true);

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(hwnd);
    let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, None, hwnd, None);
    let _ = DestroyMenu(menu);
}

fn append(
    menu: windows::Win32::UI::WindowsAndMessaging::HMENU,
    id: u32,
    text: &str,
    checked: bool,
    enabled: bool,
) {
    let mut flags = MF_STRING;
    if checked {
        flags |= MF_CHECKED;
    }
    if !enabled {
        flags |= MF_GRAYED;
    }
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let _ = unsafe { AppendMenuW(menu, flags, id as usize, PCWSTR(wide.as_ptr())) };
}

fn append_sep(menu: windows::Win32::UI::WindowsAndMessaging::HMENU) {
    let _ = unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()) };
}

fn load_app_icon(
    instance: windows::Win32::Foundation::HINSTANCE,
) -> Result<windows::Win32::UI::WindowsAndMessaging::HICON, String> {
    unsafe {
        LoadIconW(Some(instance), app_icon_resource())
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .map_err(|err| err.to_string())
    }
}

fn write_tip(nid: &mut NOTIFYICONDATAW, tip: &str) {
    let mut wide: Vec<u16> = tip.encode_utf16().take(127).collect();
    wide.push(0);
    for (i, ch) in wide.iter().enumerate() {
        if i < nid.szTip.len() {
            nid.szTip[i] = *ch;
        }
    }
}

pub fn shell_open(target: &str) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    let result = unsafe {
        windows::Win32::UI::Shell::ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    result.0 as usize > 32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn menu_ids_map_to_commands() {
        assert_eq!(
            TrayCommand::from_id(ID_TOGGLE_ENABLED),
            Some(TrayCommand::ToggleEnabled)
        );
        assert_eq!(TrayCommand::from_id(ID_EXIT), Some(TrayCommand::Exit));
        assert_eq!(TrayCommand::from_id(0), None);
    }

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
