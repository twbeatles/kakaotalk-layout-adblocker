pub mod api;
pub mod fake;

#[cfg(windows)]
pub mod event_hook;
#[cfg(windows)]
pub mod process;
#[cfg(windows)]
pub mod real;
#[cfg(windows)]
pub mod single_instance;
#[cfg(windows)]
pub mod startup;
#[cfg(windows)]
pub mod tray;

#[cfg(windows)]
pub fn attach_parent_console() -> bool {
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS).is_ok() }
}

pub use api::{
    Win32Api, SMTO_ABORTIFHUNG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE,
    SW_SHOW, WM_CLOSE,
};
pub use fake::{ApiCall, CloseBehavior, FakeWin32, WindowSnapshot};

#[cfg(windows)]
pub use real::RealWin32;

#[cfg(test)]
mod tests {
    use kakao_core::WindowText;

    use super::api::window_text_from_length_and_copy;

    #[test]
    fn empty_length_with_error_is_unknown() {
        let text = window_text_from_length_and_copy(0, 5, 0, 0, 1, "");
        assert_eq!(text, WindowText::Unknown { error_code: 5 });
    }

    #[test]
    fn empty_length_without_error_is_known_empty() {
        let text = window_text_from_length_and_copy(0, 0, 0, 0, 1, "");
        assert_eq!(text, WindowText::Known(String::new()));
    }

    #[test]
    fn truncated_when_copy_fills_buffer() {
        let text = window_text_from_length_and_copy(4, 0, 4, 0, 5, "abcd");
        assert!(matches!(text, WindowText::Truncated(_)));
    }
}
