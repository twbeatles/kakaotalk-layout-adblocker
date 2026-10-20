#![cfg(windows)]

use kakao_win32::RealWin32;
use kakao_win32::api::{SW_HIDE, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER, Win32Api};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW,
    WINDOW_EX_STYLE, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};
use windows::core::w;

unsafe extern "system" fn smoke_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[test]
fn own_hidden_window_roundtrip() {
    unsafe {
        let class = w!("KakaoAdblockWin32Smoke");
        let instance = GetModuleHandleW(None).expect("module");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(smoke_wnd_proc),
            hInstance: instance.into(),
            lpszClassName: class,
            ..Default::default()
        };
        let atom = RegisterClassW(&wc);
        assert!(atom != 0, "register class");
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!("kakao-win32-smoke"),
            WS_OVERLAPPEDWINDOW,
            40,
            40,
            240,
            180,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .expect("create window");
        let raw = hwnd.0 as isize as i64;
        let api = RealWin32::new();
        assert!(api.is_window(raw));
        let class_name = api.get_class_name(raw);
        assert_eq!(class_name, "KakaoAdblockWin32Smoke");
        let original = api.get_window_rect(raw).expect("rect");
        let _ = api.show_window(raw, SW_HIDE);
        assert!(!api.is_window_visible(raw));
        let _ = api.show_window(raw, SW_SHOW);
        assert!(api.set_window_pos(
            raw,
            original.left,
            original.top,
            original.width(),
            original.height(),
            SWP_NOZORDER | SWP_NOACTIVATE,
        ));
        let restored = api.get_window_rect(raw).expect("restored rect");
        assert_eq!(restored.width(), original.width());
        assert_eq!(restored.height(), original.height());
        let _ = DestroyWindow(HWND(hwnd.0));
    }
}
