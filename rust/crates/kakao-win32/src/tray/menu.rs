use std::sync::atomic::Ordering;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Shell::NOTIFYICONDATAW;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, LoadIconW, SetForegroundWindow,
    TrackPopupMenu, HMENU, IDI_APPLICATION, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    TPM_RIGHTBUTTON,
};

use super::command::{
    ID_CHECK_UPDATE, ID_EXIT, ID_OPEN_LOGS, ID_OPEN_RELEASES, ID_RESET_RESTORE,
    ID_TOGGLE_AGGRESSIVE, ID_TOGGLE_ENABLED, ID_TOGGLE_STARTUP,
};
use super::host::TrayHost;
use super::state::StatusSnapshot;
use super::status_text::status_menu_lines;

// MAKEINTRESOURCE(1): first ICON resource embedded by kakao-app/build.rs.
#[allow(clippy::manual_dangling_ptr)]
fn app_icon_resource() -> PCWSTR {
    PCWSTR(1usize as *const u16)
}

pub(super) unsafe fn show_menu(hwnd: windows::Win32::Foundation::HWND, host: &TrayHost) {
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

fn append(menu: HMENU, id: u32, text: &str, checked: bool, enabled: bool) {
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

fn append_sep(menu: HMENU) {
    let _ = unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()) };
}

pub(super) fn load_app_icon(
    instance: windows::Win32::Foundation::HINSTANCE,
) -> Result<windows::Win32::UI::WindowsAndMessaging::HICON, String> {
    unsafe {
        LoadIconW(Some(instance), app_icon_resource())
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .map_err(|err| err.to_string())
    }
}

pub(super) fn write_tip(nid: &mut NOTIFYICONDATAW, tip: &str) {
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
