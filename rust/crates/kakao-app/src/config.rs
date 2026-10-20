// Pure-move split of the former monolithic `config.rs` (SRP).
//
// - `paths`: well-known filenames + `RuntimePaths`
// - `settings`: `AppSettings` (typed overlay, load/save)
// - `storage`: JSON self-heal I/O (`*.broken-*`, atomic write, bootstrap)
// - `log`: size-rotating log writer
//
// No defaults/heal semantics changed; this file only re-exports the previous
// public surface.
mod log;
mod paths;
mod settings;
mod storage;

pub use log::{
    LOG_ROTATE_BYTES, RotatingLog, RotatingLogWriter, rotate_log_if_needed, rotated_log_path,
};
pub use paths::runtime_paths;
pub use paths::{
    APPDATA_DIRNAME, LOG_FILE, RULES_FILE, RuntimePaths, SETTINGS_FILE, UPDATE_PUBLIC_KEY_B64,
    VERSION,
};
pub use settings::{AppSettings, load_settings, save_settings, update_settings};
pub use storage::{atomic_write, ensure_runtime_files, load_rules};
