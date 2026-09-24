use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{SystemTime, UNIX_EPOCH};

use kakao_core::LayoutRules;
use serde_json::Value;

use super::paths::RuntimePaths;
use super::settings::AppSettings;

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

pub(super) fn load_json_value(path: &Path, label: &str) -> (Value, Vec<String>) {
    let mut warnings = Vec::new();
    cleanup_broken_backups(path);
    if !path.is_file() {
        return (Value::Object(Default::default()), warnings);
    }
    match fs::read_to_string(path) {
        // Windows PowerShell 5.1 `Set-Content -Encoding utf8` and Notepad's
        // "UTF-8 (BOM)" prepend U+FEFF, which serde_json rejects. Without this
        // a hand-edited but valid file was "healed" back to defaults.
        Ok(text) => {
            match serde_json::from_str::<Value>(text.strip_prefix('\u{feff}').unwrap_or(&text)) {
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
            }
        }
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
        // flush() only reaches the OS cache; without sync_all a power loss
        // right after the rename can leave an empty file that self-heals to
        // defaults on the next launch.
        file.sync_all()?;
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

pub fn ensure_runtime_files(paths: &RuntimePaths) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Err(err) = fs::create_dir_all(&paths.appdata_dir) {
        warnings.push(format!("APPDATA 디렉터리 생성 실패: {err}"));
        return warnings;
    }
    if !paths.settings_file.is_file() {
        match super::settings::save_settings(&paths.settings_file, &AppSettings::default()) {
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
    let mut backups: Vec<std::path::PathBuf> = fs::read_dir(parent)
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
