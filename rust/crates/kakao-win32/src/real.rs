#![cfg(windows)]

use std::cell::RefCell;

use kakao_core::{Rect, WindowText};
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WIN32_ERROR, WPARAM};
use windows::Win32::Graphics::Gdi::UpdateWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GetClassNameW, GetClientRect, GetParent, GetWindowLongPtrW,
    GetWindowRect, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsHungAppWindow,
    IsWindow, IsWindowVisible, SendMessageTimeoutW, SetWindowPos, ShowWindow, GWL_STYLE,
    SEND_MESSAGE_TIMEOUT_FLAGS, SET_WINDOW_POS_FLAGS, SHOW_WINDOW_CMD, WS_VISIBLE,
};

use crate::api::{window_text_from_length_and_copy, Win32Api, SMTO_ABORTIFHUNG};

thread_local! {
    static ENUM_ACCUM: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };
}

fn hwnd_from_i64(hwnd: i64) -> HWND {
    HWND(hwnd as isize as *mut core::ffi::c_void)
}

fn hwnd_to_i64(hwnd: HWND) -> i64 {
    hwnd.0 as isize as i64
}

unsafe extern "system" fn enum_proc(hwnd: HWND, _lparam: LPARAM) -> BOOL {
    ENUM_ACCUM.with(|acc| acc.borrow_mut().push(hwnd_to_i64(hwnd)));
    BOOL::from(true)
}

#[derive(Default)]
pub struct RealWin32;

impl RealWin32 {
    pub fn new() -> Self {
        Self
    }
}

impl Win32Api for RealWin32 {
    fn enum_windows(&self, cb: &mut dyn FnMut(i64) -> bool) -> bool {
        ENUM_ACCUM.with(|acc| acc.borrow_mut().clear());
        let ok = unsafe { EnumWindows(Some(enum_proc), LPARAM(0)) }.is_ok();
        ENUM_ACCUM.with(|acc| {
            for &hwnd in acc.borrow().iter() {
                if !cb(hwnd) {
                    break;
                }
            }
        });
        ok
    }

    fn enum_child_windows(&self, parent: i64, cb: &mut dyn FnMut(i64) -> bool) -> bool {
        ENUM_ACCUM.with(|acc| acc.borrow_mut().clear());
        let ok =
            unsafe { EnumChildWindows(Some(hwnd_from_i64(parent)), Some(enum_proc), LPARAM(0)) }
                .as_bool();
        // Win32 EnumChildWindows walks all descendants. The API contract is
        // direct children only; filter by GetParent == parent.
        ENUM_ACCUM.with(|acc| {
            for &hwnd in acc.borrow().iter() {
                if self.get_parent(hwnd) != parent {
                    continue;
                }
                if !cb(hwnd) {
                    break;
                }
            }
        });
        ok
    }

    fn enum_descendant_windows(&self, parent: i64, cb: &mut dyn FnMut(i64) -> bool) -> bool {
        ENUM_ACCUM.with(|acc| acc.borrow_mut().clear());
        let ok =
            unsafe { EnumChildWindows(Some(hwnd_from_i64(parent)), Some(enum_proc), LPARAM(0)) }
                .as_bool();
        let hwnds = ENUM_ACCUM.with(|acc| std::mem::take(&mut *acc.borrow_mut()));
        for hwnd in hwnds {
            if !cb(hwnd) {
                break;
            }
        }
        ok
    }

    fn get_window_thread_process_id(&self, hwnd: i64) -> i64 {
        let mut pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(hwnd_from_i64(hwnd), Some(&mut pid));
        }
        i64::from(pid)
    }

    fn get_class_name(&self, hwnd: i64) -> String {
        let mut buf = [0u16; 256];
        let n = unsafe { GetClassNameW(hwnd_from_i64(hwnd), &mut buf) };
        if n <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..n as usize])
    }

    fn get_window_text_result(&self, hwnd: i64) -> WindowText {
        unsafe { windows::Win32::Foundation::SetLastError(WIN32_ERROR(0)) };
        let length = unsafe { GetWindowTextLengthW(hwnd_from_i64(hwnd)) };
        let length_error = unsafe { windows::Win32::Foundation::GetLastError() }.0;
        if length < 0 || (length == 0 && length_error != 0) {
            return window_text_from_length_and_copy(length, length_error, 0, 0, 1, "");
        }
        if length == 0 {
            return WindowText::Known(String::new());
        }
        let buffer_len = length.saturating_add(1).max(1);
        let mut buf = vec![0u16; buffer_len as usize];
        unsafe { windows::Win32::Foundation::SetLastError(WIN32_ERROR(0)) };
        let copied = unsafe { GetWindowTextW(hwnd_from_i64(hwnd), &mut buf) };
        let text_error = unsafe { windows::Win32::Foundation::GetLastError() }.0;
        let end = buf.iter().position(|ch| *ch == 0).unwrap_or(buf.len());
        let value = String::from_utf16_lossy(&buf[..end]);
        window_text_from_length_and_copy(
            length,
            length_error,
            copied,
            text_error,
            buffer_len,
            &value,
        )
    }

    fn get_parent(&self, hwnd: i64) -> i64 {
        unsafe { GetParent(hwnd_from_i64(hwnd)) }
            .map(hwnd_to_i64)
            .unwrap_or(0)
    }

    fn get_window_rect(&self, hwnd: i64) -> Option<Rect> {
        let mut rect = RECT::default();
        unsafe { GetWindowRect(hwnd_from_i64(hwnd), &mut rect) }
            .ok()
            .map(|_| Rect {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            })
    }

    fn get_client_rect(&self, hwnd: i64) -> Option<Rect> {
        let mut rect = RECT::default();
        unsafe { GetClientRect(hwnd_from_i64(hwnd), &mut rect) }
            .ok()
            .map(|_| Rect {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            })
    }

    fn is_window(&self, hwnd: i64) -> bool {
        unsafe { IsWindow(Some(hwnd_from_i64(hwnd))) }.as_bool()
    }

    fn is_window_visible(&self, hwnd: i64) -> bool {
        unsafe { IsWindowVisible(hwnd_from_i64(hwnd)) }.as_bool()
    }

    fn has_visible_style(&self, hwnd: i64) -> bool {
        let style = unsafe { GetWindowLongPtrW(hwnd_from_i64(hwnd), GWL_STYLE) };
        (style as u32) & WS_VISIBLE.0 != 0
    }

    fn is_hung_app_window(&self, hwnd: i64) -> bool {
        unsafe { IsHungAppWindow(hwnd_from_i64(hwnd)) }.as_bool()
    }

    fn show_window(&self, hwnd: i64, cmd: i32) -> bool {
        unsafe { ShowWindow(hwnd_from_i64(hwnd), SHOW_WINDOW_CMD(cmd)) }.as_bool()
    }

    fn set_window_pos(
        &self,
        hwnd: i64,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> bool {
        unsafe {
            SetWindowPos(
                hwnd_from_i64(hwnd),
                Some(HWND::default()),
                x,
                y,
                width,
                height,
                SET_WINDOW_POS_FLAGS(flags),
            )
        }
        .is_ok()
    }

    fn send_message_timeout(
        &self,
        hwnd: i64,
        msg: u32,
        wparam: usize,
        lparam: isize,
        timeout_ms: u32,
    ) -> (bool, isize) {
        let mut result = usize::default();
        let ok = unsafe {
            SendMessageTimeoutW(
                hwnd_from_i64(hwnd),
                msg,
                WPARAM(wparam),
                LPARAM(lparam),
                SEND_MESSAGE_TIMEOUT_FLAGS(SMTO_ABORTIFHUNG),
                timeout_ms.max(1),
                Some(&mut result),
            )
        };
        (ok.0 != 0, result as isize)
    }

    fn update_window(&self, hwnd: i64) -> bool {
        unsafe { UpdateWindow(hwnd_from_i64(hwnd)) }.as_bool()
    }

    // UpdateWindow lives in gdi32.

    fn get_last_error(&self) -> u32 {
        unsafe { windows::Win32::Foundation::GetLastError() }.0
    }
}
