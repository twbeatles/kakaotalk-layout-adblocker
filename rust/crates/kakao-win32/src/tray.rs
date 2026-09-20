#![cfg(windows)]

// Pure-move split of the former monolithic `tray.rs` (SRP).
//
// - `command`: menu IDs + `TrayCommand`
// - `state`: `TrayFlags`/`TrayStatus`/`StatusSnapshot`
// - `status_text`: tooltip + menu header rendering (Win32-free, unit-tested)
// - `shell_ready`: `Shell_TrayWnd` readiness + notify-icon retry policy
// - `host`: message-only window + `run_loop` event loop
// - `menu`: popup-menu construction, icon/tip helpers, `shell_open`
//
// No tray/menu/status semantics changed; this file only re-exports the
// previous public surface.
mod command;
mod host;
mod menu;
mod shell_ready;
mod state;
mod status_text;

pub use command::{
    TrayCommand, ID_CHECK_UPDATE, ID_EXIT, ID_OPEN_LOGS, ID_OPEN_RELEASES, ID_RESET_RESTORE,
    ID_TOGGLE_AGGRESSIVE, ID_TOGGLE_ENABLED, ID_TOGGLE_STARTUP,
};
pub use host::{request_exit, run_loop, run_loop_with_ready};
pub use menu::shell_open;
pub use shell_ready::{
    continue_message_loop_without_icon, notify_icon_retry_delays, wait_for_shell_ready,
    wait_for_shell_ready_with,
};
pub use state::{StatusSnapshot, TrayFlags, TrayStatus};
pub use status_text::{status_menu_lines, status_tooltip};
