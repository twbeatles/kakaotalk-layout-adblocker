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
    rotate_log_if_needed, rotated_log_path, RotatingLog, RotatingLogWriter, LOG_ROTATE_BYTES,
};
pub use paths::runtime_paths;
pub use paths::{
    RuntimePaths, APPDATA_DIRNAME, LOG_FILE, RULES_FILE, SETTINGS_FILE, UPDATE_PUBLIC_KEY_B64,
    VERSION,
};
pub use settings::{load_settings, save_settings, update_settings, AppSettings};
pub use storage::{atomic_write, ensure_runtime_files, load_rules};
