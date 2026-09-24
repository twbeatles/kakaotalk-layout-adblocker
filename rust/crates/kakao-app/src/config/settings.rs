use std::path::Path;

use kakao_core::LayoutSettings;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::storage::load_json_value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub run_on_startup: bool,
    #[serde(default = "default_true")]
    pub start_minimized: bool,
    #[serde(default = "default_poll")]
    pub poll_interval_ms: u32,
    #[serde(default = "default_idle_poll")]
    pub idle_poll_interval_ms: u32,
    /// Upper bound for the idle reconciliation interval once KakaoTalk has
    /// produced no window events for a while. `<= idle_poll_interval_ms`
    /// disables the backoff.
    #[serde(default = "default_idle_backoff_max")]
    pub idle_backoff_max_ms: u32,
    #[serde(default = "default_pid_scan")]
    pub pid_scan_interval_ms: u32,
    #[serde(default = "default_cache_cleanup")]
    pub cache_cleanup_interval_ms: u32,
    #[serde(default = "default_burst_iter")]
    pub burst_scan_iterations: u32,
    #[serde(default = "default_burst_interval")]
    pub burst_scan_interval_ms: u32,
    #[serde(default = "default_true")]
    pub aggressive_mode: bool,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        serde_json::from_value(serde_json::json!({})).expect("default settings")
    }
}

impl AppSettings {
    pub fn to_core(&self) -> LayoutSettings {
        LayoutSettings {
            enabled: self.enabled,
            aggressive_mode: self.aggressive_mode,
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_poll() -> u32 {
    50
}
fn default_idle_poll() -> u32 {
    200
}
fn default_idle_backoff_max() -> u32 {
    1000
}
fn default_pid_scan() -> u32 {
    200
}
fn default_cache_cleanup() -> u32 {
    1000
}
fn default_burst_iter() -> u32 {
    3
}
fn default_burst_interval() -> u32 {
    20
}
fn default_log_level() -> String {
    "INFO".into()
}

pub fn load_settings(path: &Path) -> (AppSettings, Vec<String>) {
    let (value, mut warnings) = load_json_value(path, "layout_settings_v11.json");
    if value.is_null()
        || (value.as_object().map(|o| o.is_empty()).unwrap_or(false) && !path.is_file())
    {
        return (AppSettings::default(), warnings);
    }
    let (settings, merge_warnings) =
        merge_typed_settings(AppSettings::default(), &value, "layout_settings_v11.json");
    warnings.extend(merge_warnings);
    (settings, warnings)
}

fn merge_typed_settings(
    base: AppSettings,
    overrides: &Value,
    label: &str,
) -> (AppSettings, Vec<String>) {
    if overrides.is_null() {
        return (base, Vec::new());
    }
    let Some(over) = overrides.as_object() else {
        return (base, Vec::new());
    };
    if over.is_empty() {
        return (base, Vec::new());
    }
    let Ok(mut current) = serde_json::to_value(&base) else {
        return (
            base,
            vec![format!("{label} 직렬화에 실패해 기본값을 유지합니다.")],
        );
    };
    let mut result = base;
    let mut warnings = Vec::new();
    for (key, value) in over {
        let mut trial = current.clone();
        if let Some(map) = trial.as_object_mut() {
            map.insert(key.clone(), value.clone());
        }
        match serde_json::from_value::<AppSettings>(trial.clone()) {
            Ok(parsed) => {
                current = trial;
                result = parsed;
            }
            Err(_) => {
                warnings.push(format!(
                    "{label} 필드 '{key}' 타입이 올바르지 않아 기존/기본값을 유지합니다."
                ));
            }
        }
    }
    (result, warnings)
}

/// Read-modify-write for tray toggles: apply `change` to the settings
/// currently on disk rather than to the copy loaded at startup, so values the
/// user edited in the JSON while the app was running are not overwritten.
/// Falls back to `fallback` when the file cannot be read cleanly. Returns the
/// settings that were written.
pub fn update_settings(
    path: &Path,
    fallback: &AppSettings,
    change: impl FnOnce(&mut AppSettings),
) -> std::io::Result<AppSettings> {
    let (on_disk, warnings) = load_settings(path);
    let mut next = if warnings.is_empty() && path.is_file() {
        on_disk
    } else {
        fallback.clone()
    };
    change(&mut next);
    save_settings(path, &next)?;
    Ok(next)
}

pub fn save_settings(path: &Path, settings: &AppSettings) -> std::io::Result<()> {
    let body = serde_json::to_string_pretty(settings).unwrap_or_else(|_| "{}".into());
    super::storage::atomic_write(path, &format!("{body}\n"))
}
