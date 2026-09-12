#![cfg(windows)]

use std::cell::{Cell, RefCell};
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
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
const TRAY_STATUS_TIMER_ID: usize = 2;
const TRAY_STATUS_INTERVAL_MS: u32 = 1000;
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
    fn capture(flags: &TrayFlags, status: &TrayStatus) -> Self {
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

/// Single-line summary for the notification-area tooltip (NIF_TIP, 127 wchars).
pub fn status_tooltip(snapshot: &StatusSnapshot) -> String {
    let mut text = format!(
        "KakaoTalk Layout AdBlocker\n차단 {} · 공격 {} · 메인윈도우 {}",
        on_off(snapshot.enabled),
        on_off(snapshot.aggressive),
        snapshot.main_windows
    );
    if snapshot.restore_failures > 0 {
        text.push_str(&format!("\n복원 실패 {}건", snapshot.restore_failures));
    }
    truncate_utf16(&text, 127)
}

/// Grayed header lines shown above the tray menu items.
pub fn status_menu_lines(snapshot: &StatusSnapshot) -> Vec<String> {
    let mut lines = vec![
        "KakaoTalk Layout AdBlocker".to_string(),
        format!(
            "차단 {} · 공격 모드 {} · 메인윈도우 {}",
            on_off(snapshot.enabled),
            on_off(snapshot.aggressive),
            snapshot.main_windows
        ),
        format!(
            "누적 숨김 {} · 누적 닫힘 {} · 누적 리사이즈 {}",
            snapshot.hidden_windows, snapshot.closed_windows, snapshot.resized_windows
        ),
    ];
    if snapshot.restore_failures > 0 {
        lines.push(format!(
            "복원 실패 {}건 (초기화 가능)",
            snapshot.restore_failures
        ));
    }
    let error = snapshot.last_error.trim();
    if !error.is_empty() {
        lines.push(format!("오류: {}", truncate_utf16(error, 60)));
    }
    lines
}

fn on_off(value: bool) -> &'static str {
    if value {
        "ON"
    } else {
        "OFF"
    }
}

fn truncate_utf16(text: &str, max_units: usize) -> String {
    if text.encode_utf16().count() <= max_units {
        return text.to_string();
    }
    let mut out = String::new();
    let mut units = 0usize;
    for ch in text.chars() {
        let len = ch.len_utf16();
        if units + len > max_units {
            break;
        }
        units += len;
        out.push(ch);
    }
    out
}

/// Tray window state.
///
/// Every field is interior-mutable so `wnd_proc` only ever takes a shared
/// reference. The previous version handed out `&mut TrayHost` and invoked the
/// user callback through it; a modal `MessageBoxW` in that callback pumps
/// messages and can re-enter `wnd_proc`, which would alias the `&mut`.
struct TrayHost {
    flags: TrayFlags,
    status: TrayStatus,
    nid: RefCell<NOTIFYICONDATAW>,
    taskbar_created: u32,
    icon_added: Cell<bool>,
    pending: RefCell<Vec<TrayCommand>>,
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

unsafe fn show_menu(hwnd: HWND, host: &TrayHost) {
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    let snapshot = StatusSnapshot::capture(&host.flags, &host.status);
    for line in status_menu_lines(&snapshot) {
        append(menu, 0, &line, false, false);
    }
    append_sep(menu);
    append(
        menu,
        ID_TOGGLE_ENABLED,
        if snapshot.enabled {
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
        snapshot.aggressive,
        true,
    );
    append(
        menu,
        ID_TOGGLE_STARTUP,
        "시작프로그램 등록",
        host.flags.startup.load(Ordering::SeqCst),
        true,
    );
    append(
        menu,
        ID_RESET_RESTORE,
        "복원 실패 초기화",
        false,
        snapshot.restore_failures > 0 || !snapshot.last_error.is_empty(),
    );
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
    nid.szTip.fill(0);
    let wide: Vec<u16> = tip.encode_utf16().take(nid.szTip.len() - 1).collect();
    for (i, ch) in wide.iter().enumerate() {
        nid.szTip[i] = *ch;
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

    #[test]
    fn status_lines_report_counters_and_failures() {
        let snapshot = StatusSnapshot {
            enabled: true,
            aggressive: false,
            main_windows: 2,
            hidden_windows: 7,
            closed_windows: 1,
            resized_windows: 12,
            restore_failures: 3,
            last_error: "restore show failed hwnd=42".into(),
        };
        let lines = status_menu_lines(&snapshot);
        assert!(lines.iter().any(|l| l.contains("차단 ON")));
        assert!(lines.iter().any(|l| l.contains("공격 모드 OFF")));
        assert!(lines.iter().any(|l| l.contains("메인윈도우 2")));
        assert!(lines.iter().any(|l| l.contains("누적 숨김 7")));
        assert!(lines.iter().any(|l| l.contains("누적 닫힘 1")));
        assert!(lines.iter().any(|l| l.contains("누적 리사이즈 12")));
        assert!(lines.iter().any(|l| l.contains("복원 실패 3건")));
        assert!(lines.iter().any(|l| l.contains("hwnd=42")));
    }

    #[test]
    fn status_lines_omit_failure_rows_when_clean() {
        let snapshot = StatusSnapshot {
            enabled: true,
            aggressive: true,
            main_windows: 1,
            ..StatusSnapshot::default()
        };
        let lines = status_menu_lines(&snapshot);
        assert!(!lines.iter().any(|l| l.contains("복원 실패")));
        assert!(!lines.iter().any(|l| l.starts_with("오류:")));
    }

    #[test]
    fn tooltip_fits_the_win32_limit() {
        let snapshot = StatusSnapshot {
            enabled: true,
            aggressive: true,
            main_windows: u32::MAX,
            restore_failures: u32::MAX,
            last_error: "x".repeat(500),
            ..StatusSnapshot::default()
        };
        let tip = status_tooltip(&snapshot);
        assert!(tip.encode_utf16().count() <= 127);
        assert!(tip.starts_with("KakaoTalk Layout AdBlocker"));
    }
}
