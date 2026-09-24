use kakao_core::{Rect, WindowText};

pub const SW_HIDE: i32 = 0;
pub const SW_SHOW: i32 = 5;
pub const SWP_NOSIZE: u32 = 0x0001;
pub const SWP_NOMOVE: u32 = 0x0002;
pub const SWP_NOZORDER: u32 = 0x0004;
pub const SWP_NOACTIVATE: u32 = 0x0010;
pub const WM_CLOSE: u32 = 0x0010;
pub const SMTO_ABORTIFHUNG: u32 = 0x0002;

pub trait Win32Api: Send + Sync {
    fn enum_windows(&self, cb: &mut dyn FnMut(i64) -> bool) -> bool;
    fn enum_child_windows(&self, parent: i64, cb: &mut dyn FnMut(i64) -> bool) -> bool;
    /// Every descendant of `parent` in Win32 `EnumChildWindows` order, without
    /// the direct-child filter. Lets graph building enumerate a tree once.
    fn enum_descendant_windows(&self, parent: i64, cb: &mut dyn FnMut(i64) -> bool) -> bool;
    fn get_window_thread_process_id(&self, hwnd: i64) -> i64;
    fn get_class_name(&self, hwnd: i64) -> String;
    fn get_window_text_result(&self, hwnd: i64) -> WindowText;
    fn get_parent(&self, hwnd: i64) -> i64;
    fn get_window_rect(&self, hwnd: i64) -> Option<Rect>;
    fn get_client_rect(&self, hwnd: i64) -> Option<Rect>;
    fn is_window(&self, hwnd: i64) -> bool;
    fn is_window_visible(&self, hwnd: i64) -> bool;
    /// The window's own `WS_VISIBLE` style. Unlike `is_window_visible` it does
    /// not depend on ancestors, so a child restored under a hidden parent
    /// still reports success.
    fn has_visible_style(&self, hwnd: i64) -> bool;
    /// `IsHungAppWindow`: the owning thread has not pumped messages for ~5s.
    /// Synchronous cross-thread calls (`ShowWindow`, `SetWindowPos`) on such a
    /// window can block the caller indefinitely.
    fn is_hung_app_window(&self, hwnd: i64) -> bool;
    fn show_window(&self, hwnd: i64, cmd: i32) -> bool;
    fn set_window_pos(
        &self,
        hwnd: i64,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> bool;
    fn send_message_timeout(
        &self,
        hwnd: i64,
        msg: u32,
        wparam: usize,
        lparam: isize,
        timeout_ms: u32,
    ) -> (bool, isize);
    fn update_window(&self, hwnd: i64) -> bool;
    fn get_last_error(&self) -> u32;
}

pub fn window_text_from_length_and_copy(
    length: i32,
    length_error: u32,
    copied: i32,
    text_error: u32,
    buffer_len: i32,
    value: &str,
) -> WindowText {
    if length < 0 {
        return WindowText::Unknown {
            error_code: length_error,
        };
    }
    if length == 0 && length_error != 0 {
        return WindowText::Unknown {
            error_code: length_error,
        };
    }
    if copied == 0 && length > 0 && text_error != 0 {
        return WindowText::Unknown {
            error_code: text_error,
        };
    }
    let truncated = length > 0 && copied >= buffer_len - 1 && length >= buffer_len - 1;
    if truncated {
        WindowText::Truncated(value.to_string())
    } else {
        WindowText::Known(value.to_string())
    }
}
