use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use kakao_core::{LayoutRules, LayoutSettings};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const VERSION: &str = "11.1.4";
pub const APPDATA_DIRNAME: &str = "KakaoTalkAdBlockerLayout";
pub const SETTINGS_FILE: &str = "layout_settings_v11.json";
pub const RULES_FILE: &str = "layout_rules_v11.json";
pub const LOG_FILE: &str = "layout_adblock.log";
pub const UPDATE_PUBLIC_KEY_B64: &str = "Cix9d2r5UZxpDL4Bp9CWNrjMDRTQHF5Y1snTMYnMQ2U=";

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

#[derive(Debug, Clone)]
pub struct RuntimePaths {
    pub appdata_dir: PathBuf,
    pub settings_file: PathBuf,
    pub rules_file: PathBuf,
    pub log_file: PathBuf,
}

pub fn runtime_paths() -> RuntimePaths {
    let appdata = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".").join("AppData").join("Roaming"));
    let appdata_dir = appdata.join(APPDATA_DIRNAME);
    RuntimePaths {
        settings_file: appdata_dir.join(SETTINGS_FILE),
        rules_file: appdata_dir.join(RULES_FILE),
        log_file: appdata_dir.join(LOG_FILE),
        appdata_dir,
    }
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

pub fn load_rules(path: &Path) -> (LayoutRules, Vec<String>) {
    let (value, mut warnings) = load_json_value(path, "layout_rules_v11.json");
    let (mut rules, merge_warnings) = LayoutRules::default().overlay_with_warnings(&value);
    warnings.extend(merge_warnings);
    if rules.banner_min_height_px > rules.banner_max_height_px {
        std::mem::swap(
            &mut rules.banner_min_height_px,
            &mut rules.banner_max_height_px,
        );
        warnings.push(
            "layout_rules_v11.json banner 높이 범위(min/max)가 역전되어 자동 교정했습니다.".into(),
        );
    }
    rules.popup_search_depth = rules.popup_search_depth.clamp(1, 2);
    if value.get("ad_candidate_classes").is_none() {
        rules.ad_candidate_classes = rules.main_window_classes.clone();
    }
    (rules, warnings)
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

fn load_json_value(path: &Path, label: &str) -> (Value, Vec<String>) {
    let mut warnings = Vec::new();
    cleanup_broken_backups(path);
    if !path.is_file() {
        return (Value::Object(Default::default()), warnings);
    }
    match fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => (Value::Object(map), warnings),
            Ok(_) => {
                backup_broken(path, label, "최상위 타입이 object가 아님", &mut warnings);
                heal_default(path, label, &mut warnings);
                (Value::Object(Default::default()), warnings)
            }
            Err(_) => {
                backup_broken(path, label, "JSON 파싱 실패", &mut warnings);
                heal_default(path, label, &mut warnings);
                (Value::Object(Default::default()), warnings)
            }
        },
        Err(err) => {
            warnings.push(format!("{label} 읽기 실패: {err}"));
            (Value::Object(Default::default()), warnings)
        }
    }
}

fn backup_broken(path: &Path, label: &str, reason: &str, warnings: &mut Vec<String>) {
    let stamp = timestamp();
    let backup = path.with_file_name(format!(
        "{}.broken-{stamp}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
    ));
    match fs::copy(path, &backup) {
        Ok(_) => warnings.push(format!(
            "{label} 손상 감지: {reason}. 백업 생성: {}.",
            backup.display()
        )),
        Err(err) => warnings.push(format!("{label} 손상 감지: {reason}. 백업 실패({err}).")),
    }
}

fn heal_default(path: &Path, label: &str, warnings: &mut Vec<String>) {
    let default = if label.contains("settings") {
        serde_json::to_string_pretty(&AppSettings::default()).unwrap_or_else(|_| "{}".into())
    } else {
        serde_json::to_string_pretty(&LayoutRules::default()).unwrap_or_else(|_| "{}".into())
    };
    match atomic_write(path, &format!("{default}\n")) {
        Ok(()) => warnings.push(format!(
            "{label} 자동 복구 성공: 기본값 JSON으로 재생성했습니다."
        )),
        Err(err) => warnings.push(format!(
            "{label} 자동 복구 실패({err}). 기본값으로 동작합니다."
        )),
    }
}

pub fn atomic_write(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Per-process temp name: `--self-check` runs outside the single-instance
    // mutex, so it can write these files while the tray app is saving settings.
    // A shared `foo.tmp` would let the two truncate each other mid-write.
    let tmp = path.with_file_name(format!(
        "{}.{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
        std::process::id(),
        TMP_SEQ.fetch_add(1, AtomicOrdering::Relaxed)
    ));
    let write_result = (|| -> io::Result<()> {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.flush()?;
        Ok(())
    })();
    if let Err(err) = write_result {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = fs::remove_file(&tmp);
            Err(err)
        }
    }
}

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

const BROKEN_BACKUP_MAX_AGE_SECS: u64 = 30 * 24 * 60 * 60;
pub const LOG_ROTATE_BYTES: u64 = 5 * 1024 * 1024;

pub fn ensure_runtime_files(paths: &RuntimePaths) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Err(err) = fs::create_dir_all(&paths.appdata_dir) {
        warnings.push(format!("APPDATA 디렉터리 생성 실패: {err}"));
        return warnings;
    }
    if !paths.settings_file.is_file() {
        match save_settings(&paths.settings_file, &AppSettings::default()) {
            Ok(()) => {}
            Err(err) => warnings.push(format!("layout_settings_v11.json 기본값 생성 실패: {err}")),
        }
    }
    if !paths.rules_file.is_file() {
        let body =
            serde_json::to_string_pretty(&LayoutRules::default()).unwrap_or_else(|_| "{}".into());
        if let Err(err) = atomic_write(&paths.rules_file, &format!("{body}\n")) {
            warnings.push(format!("layout_rules_v11.json 기본값 생성 실패: {err}"));
        }
    }
    warnings
}

pub fn rotated_log_path(path: &Path) -> PathBuf {
    path.with_extension("log.1")
}

pub fn rotate_log_if_needed(path: &Path) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if meta.len() <= LOG_ROTATE_BYTES {
        return;
    }
    rotate_log_now(path);
}

fn rotate_log_now(path: &Path) {
    let rotated = rotated_log_path(path);
    let _ = fs::remove_file(&rotated);
    let _ = fs::rename(path, rotated);
}

/// Append-only log file that rotates itself while the process runs.
///
/// The previous setup checked the size once at startup and then handed a plain
/// `File` to `tracing`. A tray app registered as a startup program can stay up
/// for weeks, so that check never ran again and the log grew without bound.
/// Rotating through this writer also avoids renaming a file that still has a
/// live append handle, which would keep writing into the rotated copy.
pub struct RotatingLog {
    path: PathBuf,
    max_bytes: u64,
    state: Mutex<Option<LogState>>,
}

struct LogState {
    file: fs::File,
    len: u64,
}

impl RotatingLog {
    pub fn open(path: &Path, max_bytes: u64) -> io::Result<Self> {
        rotate_log_if_needed(path);
        let state = open_append(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            max_bytes: max_bytes.max(64 * 1024),
            state: Mutex::new(Some(state)),
        })
    }

    fn write_record(&self, buf: &[u8]) -> io::Result<usize> {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let needs_rotate = guard
            .as_ref()
            .is_some_and(|state| state.len.saturating_add(buf.len() as u64) > self.max_bytes);
        if needs_rotate {
            // Drop the handle before renaming so the reopened file is the new one.
            *guard = None;
            rotate_log_now(&self.path);
        }
        if guard.is_none() {
            match open_append(&self.path) {
                Ok(state) => *guard = Some(state),
                // Losing the file must not take down logging as a whole; the
                // console layer keeps working and the next record retries.
                Err(_) => return Ok(buf.len()),
            }
        }
        let Some(state) = guard.as_mut() else {
            return Ok(buf.len());
        };
        match state.file.write(buf) {
            Ok(written) => {
                state.len = state.len.saturating_add(written as u64);
                Ok(written)
            }
            Err(err) => {
                *guard = None;
                Err(err)
            }
        }
    }

    fn flush_inner(&self) -> io::Result<()> {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match guard.as_mut() {
            Some(state) => state.file.flush(),
            None => Ok(()),
        }
    }
}

fn open_append(path: &Path) -> io::Result<LogState> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let len = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    Ok(LogState { file, len })
}

pub struct RotatingLogWriter<'a> {
    owner: &'a RotatingLog,
}

impl Write for RotatingLogWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.owner.write_record(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.owner.flush_inner()
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for RotatingLog {
    type Writer = RotatingLogWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        RotatingLogWriter { owner: self }
    }
}

fn cleanup_broken_backups(path: &Path) {
    let Some(parent) = path.parent() else {
        return;
    };
    let prefix = format!(
        "{}.broken-",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("")
    );
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut backups: Vec<PathBuf> = fs::read_dir(parent)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix))
        })
        .collect();
    backups.retain(|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let stamp = name.rsplit('-').next().and_then(|s| s.parse::<u64>().ok());
        if let Some(secs) = stamp {
            if now.saturating_sub(secs) > BROKEN_BACKUP_MAX_AGE_SECS {
                let _ = fs::remove_file(p);
                return false;
            }
        }
        true
    });
    backups.sort();
    let keep = 10usize;
    if backups.len() > keep {
        for old in backups.iter().take(backups.len() - keep) {
            let _ = fs::remove_file(old);
        }
    }
}

fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

pub fn save_settings(path: &Path, settings: &AppSettings) -> io::Result<()> {
    let body = serde_json::to_string_pretty(settings).unwrap_or_else(|_| "{}".into());
    atomic_write(path, &format!("{body}\n"))
}
