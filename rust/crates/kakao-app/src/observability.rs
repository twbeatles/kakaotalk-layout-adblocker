use std::time::{SystemTime, UNIX_EPOCH};

pub fn write_startup_trace(
    path: Option<&std::path::Path>,
    startup_launch: bool,
    minimized_requested: bool,
    tray_available: bool,
    tray_start_error: &str,
) {
    let Some(trace_path) = path else {
        return;
    };
    let trace = serde_json::json!({
        "startup_launch": startup_launch,
        "minimized_requested": minimized_requested,
        "shell_wait_attempted": true,
        "shell_wait_ok": tray_available,
        "tray_import_ok": tray_available,
        "tray_available": tray_available,
        "tray_start_error": tray_start_error,
        "window_hidden_after_start": tray_available,
    });
    if let Some(parent) = trace_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(
        trace_path,
        serde_json::to_string_pretty(&trace).unwrap_or_default(),
    );
}

pub fn init_tracing(log_path: Option<&std::path::Path>, log_level: &str) {
    use tracing_subscriber::prelude::*;
    let level = match log_level.to_ascii_uppercase().as_str() {
        "TRACE" => tracing::Level::TRACE,
        "DEBUG" => tracing::Level::DEBUG,
        "WARN" | "WARNING" => tracing::Level::WARN,
        "ERROR" => tracing::Level::ERROR,
        _ => tracing::Level::INFO,
    };
    let env_filter = tracing_subscriber::EnvFilter::from_default_env().add_directive(level.into());

    let fmt_layer = tracing_subscriber::fmt::layer();

    if let Some(path) = log_path {
        // RotatingLog keeps checking the size while the process runs; a plain
        // append handle only ever got the one startup check.
        if let Ok(rotating) =
            crate::config::RotatingLog::open(path, crate::config::LOG_ROTATE_BYTES)
        {
            let file_layer = tracing_subscriber::fmt::layer()
                .with_writer(rotating)
                .with_ansi(false);
            let _ = tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .with(file_layer)
                .try_init();
            return;
        }
    }

    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .try_init();
}

pub fn file_stamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}
