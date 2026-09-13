use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::config::{UPDATE_PUBLIC_KEY_B64, VERSION};

pub const MANIFEST_URL: &str =
    "https://github.com/twbeatles/kakaotalk-layout-adblocker/releases/latest/download/update.json";
pub const USER_AGENT: &str = "KakaoTalkLayoutAdBlocker-Updater";
pub const RELEASE_DOWNLOAD_PREFIX: &str =
    "https://github.com/twbeatles/kakaotalk-layout-adblocker/releases/download/";
pub const LEGACY_RELEASE_DOWNLOAD_PREFIX: &str =
    "https://github.com/twbeatles/kakaotalk-pc-adblock-py/releases/download/";
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const HTTP_READ_TIMEOUT: Duration = Duration::from_secs(60);
const HTTP_TOTAL_TIMEOUT: Duration = Duration::from_secs(90);
static UPDATE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static STAGING_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("{0}")]
    Message(String),
    #[error("현재 최신 버전을 사용 중입니다.")]
    NoUpdate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateManifest {
    pub version: String,
    pub tag: String,
    pub artifact_url: String,
    pub sha256: String,
    pub size: u64,
}

pub fn version_tuple(value: &str) -> Result<Vec<u32>, UpdateError> {
    let parts: Vec<&str> = value.trim().split('.').collect();
    if parts.is_empty()
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()))
    {
        return Err(UpdateError::Message(format!(
            "Invalid update version: {value}"
        )));
    }
    Ok(parts.iter().map(|part| part.parse().unwrap_or(0)).collect())
}

pub fn is_newer(candidate: &str, current: &str) -> Result<bool, UpdateError> {
    let mut left = version_tuple(candidate)?;
    let mut right = version_tuple(current)?;
    let width = left.len().max(right.len());
    left.resize(width, 0);
    right.resize(width, 0);
    Ok(left > right)
}

pub fn canonical_payload(payload: &Value) -> Result<Vec<u8>, UpdateError> {
    serde_json::to_vec(payload).map_err(|err| UpdateError::Message(err.to_string()))
}

/// Match Python json.dumps(..., ensure_ascii=False, sort_keys=True, separators=(",", ":"))
pub fn canonical_payload_python(payload: &Value) -> Result<Vec<u8>, UpdateError> {
    let dumped = pythonish_dumps(payload);
    Ok(dumped.into_bytes())
}

fn pythonish_dumps(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(true) => "true".into(),
        Value::Bool(false) => "false".into(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("\"{}\"", escape_json_string(s)),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(pythonish_dumps).collect();
            format!("[{}]", inner.join(","))
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let inner: Vec<String> = keys
                .into_iter()
                .map(|k| format!("\"{}\":{}", escape_json_string(k), pythonish_dumps(&map[k])))
                .collect();
            format!("{{{}}}", inner.join(","))
        }
    }
}

fn escape_json_string(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub fn expected_artifact_url(tag: &str) -> String {
    format!("{RELEASE_DOWNLOAD_PREFIX}{tag}/KakaoTalkLayoutAdBlocker_v11.exe")
}

pub fn is_valid_artifact_url(artifact_url: &str, tag: &str) -> bool {
    if !artifact_url.starts_with("https://") {
        return false;
    }
    let expected_rust = expected_artifact_url(tag);
    let expected_legacy =
        format!("{LEGACY_RELEASE_DOWNLOAD_PREFIX}{tag}/KakaoTalkLayoutAdBlocker_v11.exe");
    artifact_url == expected_rust || artifact_url == expected_legacy
}

pub fn parse_and_verify_manifest(
    document: &[u8],
    current_version: &str,
) -> Result<UpdateManifest, UpdateError> {
    if document.len() > MAX_MANIFEST_BYTES {
        return Err(UpdateError::Message(
            "업데이트 매니페스트가 올바르지 않습니다.".into(),
        ));
    }
    let parsed: Value = serde_json::from_slice(document)
        .map_err(|_| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?;
    let payload = parsed.get("payload").cloned().ok_or_else(|| {
        UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into())
    })?;
    let signature_b64 = parsed
        .get("signature")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into())
        })?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(signature_b64)
        .map_err(|_| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?;
    let public = base64::engine::general_purpose::STANDARD
        .decode(UPDATE_PUBLIC_KEY_B64)
        .map_err(|_| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?;
    let key_bytes: [u8; 32] = public
        .as_slice()
        .try_into()
        .map_err(|_| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?;
    let key = VerifyingKey::from_bytes(&key_bytes)
        .map_err(|_| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?;
    let sig_bytes: [u8; 64] = signature
        .as_slice()
        .try_into()
        .map_err(|_| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?;
    let sig = Signature::from_bytes(&sig_bytes);
    let canonical = canonical_payload_python(&payload)?;
    key.verify(&canonical, &sig)
        .map_err(|_| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?;

    let version = payload
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(|| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?
        .to_string();
    let tag = payload
        .get("tag")
        .and_then(Value::as_str)
        .ok_or_else(|| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?
        .to_string();
    let artifact_url = payload
        .get("artifact_url")
        .and_then(Value::as_str)
        .ok_or_else(|| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?
        .to_string();
    let sha256 = payload
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| UpdateError::Message("업데이트 파일 정보가 올바르지 않습니다.".into()))?
        .to_lowercase();
    let size = payload
        .get("size")
        .and_then(Value::as_u64)
        .ok_or_else(|| UpdateError::Message("업데이트 파일 정보가 올바르지 않습니다.".into()))?;
    if !tag.starts_with('v') || tag[1..] != version {
        return Err(UpdateError::Message(
            "업데이트 태그 정보가 올바르지 않습니다.".into(),
        ));
    }
    if !is_valid_artifact_url(&artifact_url, &tag) {
        return Err(UpdateError::Message(
            "업데이트 파일 위치가 올바르지 않습니다.".into(),
        ));
    }
    if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(UpdateError::Message(
            "업데이트 파일 정보가 올바르지 않습니다.".into(),
        ));
    }
    if size == 0 || size > MAX_ARTIFACT_BYTES {
        return Err(UpdateError::Message(
            "업데이트 파일 크기가 허용 범위를 벗어났습니다.".into(),
        ));
    }
    if let Some(expires_at_str) = payload.get("expires_at").and_then(Value::as_str) {
        if let Some(exp_ts) = parse_rfc3339_timestamp(expires_at_str) {
            let now_ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if exp_ts <= now_ts {
                return Err(UpdateError::Message(
                    "업데이트 매니페스트가 만료되었습니다.".into(),
                ));
            }
        }
    }
    if !is_newer(&version, current_version)? {
        return Err(UpdateError::NoUpdate);
    }
    Ok(UpdateManifest {
        version,
        tag,
        artifact_url,
        sha256,
        size,
    })
}

fn parse_rfc3339_timestamp(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.len() < 19 {
        return None;
    }
    let year: u64 = s[0..4].parse().ok()?;
    let month: u64 = s[5..7].parse().ok()?;
    let day: u64 = s[8..10].parse().ok()?;
    let hour: u64 = s[11..13].parse().ok()?;
    let min: u64 = s[14..16].parse().ok()?;
    let sec: u64 = s[17..19].parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || min > 59 || sec > 60 {
        return None;
    }
    let mut days = 0;
    for y in 1970..year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }
    let days_in_months = if is_leap_year(year) {
        [0, 31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    for m in 1..month {
        days += days_in_months[m as usize];
    }
    days += day - 1;
    Some(days * 86400 + hour * 3600 + min * 60 + sec)
}

fn is_leap_year(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn http_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(HTTP_CONNECT_TIMEOUT)
        .timeout_read(HTTP_READ_TIMEOUT)
        .timeout(HTTP_TOTAL_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
}

pub fn try_begin_update() -> bool {
    UPDATE_IN_PROGRESS
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

pub fn end_update() {
    UPDATE_IN_PROGRESS.store(false, Ordering::SeqCst);
}

pub fn update_in_progress() -> bool {
    UPDATE_IN_PROGRESS.load(Ordering::SeqCst)
}

#[derive(Debug, Clone)]
pub struct StagedUpdate {
    pub helper: PathBuf,
    pub current_exe: PathBuf,
    pub replacement: PathBuf,
    /// Verified at download time and re-checked by the helper right before the
    /// swap, since the staged file waits in %TEMP% until this process exits.
    pub sha256: String,
    /// Flags to restore on the relaunched instance.
    pub relaunch_args: Vec<String>,
}

/// Launch flags worth carrying across an update. Diagnostic/smoke-test flags
/// (`--startup-trace`, `--exit-after-startup-ms`) are deliberately dropped.
pub fn relaunch_args_from<I, S>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .filter(|arg| matches!(arg.as_ref(), "--startup-launch" | "--minimized"))
        .map(|arg| arg.as_ref().to_string())
        .collect()
}

pub fn download_and_verify(
    manifest: &UpdateManifest,
    dest: &std::path::Path,
) -> Result<(), UpdateError> {
    let response = http_agent()
        .get(&manifest.artifact_url)
        .call()
        .map_err(|err| UpdateError::Message(format!("업데이트 다운로드 실패: {err}")))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(manifest.size.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|err| UpdateError::Message(format!("업데이트 다운로드 실패: {err}")))?;
    if bytes.len() as u64 != manifest.size {
        return Err(UpdateError::Message(
            "업데이트 파일 크기가 허용 범위를 벗어났습니다.".into(),
        ));
    }
    if sha256_hex(&bytes) != manifest.sha256 {
        return Err(UpdateError::Message(
            "업데이트 파일 해시가 일치하지 않습니다.".into(),
        ));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| UpdateError::Message(format!("업데이트 저장 실패: {err}")))?;
    }
    std::fs::write(dest, bytes)
        .map_err(|err| UpdateError::Message(format!("업데이트 저장 실패: {err}")))?;
    Ok(())
}

pub fn resolve_helper(current_exe: &Path) -> Result<PathBuf, UpdateError> {
    let current_dir = current_exe.parent().unwrap_or_else(|| Path::new("."));
    let mut candidates = vec![current_dir.join("kakao-updater.exe")];
    candidates.extend([
        current_dir.join("target/release/kakao-updater.exe"),
        current_dir.join("target/debug/kakao-updater.exe"),
        current_dir.join("../target/release/kakao-updater.exe"),
        current_dir.join("../target/debug/kakao-updater.exe"),
        current_dir.join("../../target/release/kakao-updater.exe"),
        current_dir.join("../../target/debug/kakao-updater.exe"),
    ]);
    for cand in &candidates {
        if helper_looks_valid(cand) {
            return Ok(cand.clone());
        }
    }
    Err(UpdateError::Message(
        "kakao-updater.exe 헬퍼 바이너리를 찾을 수 없습니다. 앱과 같은 폴더에 함께 두세요.".into(),
    ))
}

fn helper_looks_valid(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    bytes.len() >= 2 && bytes[0] == b'M' && bytes[1] == b'Z'
}

pub fn unique_staging_path(prefix: &str, version: &str) -> PathBuf {
    let seq = STAGING_SEQ.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "{prefix}_{version}_{}_{}_{}.exe",
        std::process::id(),
        unix_now(),
        seq
    ))
}

pub fn stage_helper(src: &Path) -> Result<PathBuf, UpdateError> {
    let dest = unique_staging_path("kakao-updater", "helper");
    std::fs::copy(src, &dest)
        .map_err(|err| UpdateError::Message(format!("업데이트 헬퍼 준비 실패: {err}")))?;
    Ok(dest)
}

pub fn prepare_update(manifest: &UpdateManifest) -> Result<StagedUpdate, UpdateError> {
    let current_exe = std::env::current_exe()
        .map_err(|e| UpdateError::Message(format!("현재 실행 파일 경로 확인 실패: {e}")))?;
    let helper_src = resolve_helper(&current_exe)?;
    let helper = stage_helper(&helper_src)?;
    let replacement = unique_staging_path("KakaoTalkLayoutAdBlocker_v11_update", &manifest.version);
    if let Err(err) = download_and_verify(manifest, &replacement) {
        let _ = std::fs::remove_file(&helper);
        let _ = std::fs::remove_file(&replacement);
        return Err(err);
    }
    Ok(StagedUpdate {
        helper,
        current_exe,
        replacement,
        sha256: manifest.sha256.clone(),
        relaunch_args: relaunch_args_from(std::env::args().skip(1)),
    })
}

pub fn launch_helper(staged: &StagedUpdate) -> Result<(), UpdateError> {
    let pid = std::process::id();
    let mut cmd = std::process::Command::new(&staged.helper);
    cmd.arg("--pid")
        .arg(pid.to_string())
        .arg("--current")
        .arg(&staged.current_exe)
        .arg("--replacement")
        .arg(&staged.replacement);
    if !staged.sha256.is_empty() {
        cmd.arg("--sha256").arg(&staged.sha256);
    }
    for arg in &staged.relaunch_args {
        cmd.arg("--relaunch-arg").arg(arg);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    cmd.spawn()
        .map_err(|e| UpdateError::Message(format!("업데이트 헬퍼 실행 실패: {e}")))?;
    Ok(())
}

pub fn apply_update(manifest: &UpdateManifest) -> Result<(), UpdateError> {
    let staged = prepare_update(manifest)?;
    launch_helper(&staged)
}

pub fn check_for_update() -> Result<UpdateManifest, UpdateError> {
    let body = http_agent()
        .get(MANIFEST_URL)
        .call()
        .map_err(|err| UpdateError::Message(format!("업데이트 정보 다운로드 실패: {err}")))?
        .into_string()
        .map_err(|err| UpdateError::Message(format!("업데이트 정보 다운로드 실패: {err}")))?;
    parse_and_verify_manifest(body.as_bytes(), VERSION)
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn newer_version_compares_componentwise() {
        assert!(is_newer("11.0.2", "11.0.1").unwrap());
        assert!(!is_newer("11.0.1", "11.0.1").unwrap());
        assert!(!is_newer("10.9.9", "11.0.1").unwrap());
    }

    #[test]
    fn bumped_package_version_is_newer_than_current() {
        let mut parts: Vec<u32> = VERSION
            .split('.')
            .map(|part| part.parse().expect("VERSION digits"))
            .collect();
        *parts.last_mut().expect("VERSION parts") += 1;
        let newer = parts
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(".");
        assert!(is_newer(&newer, VERSION).unwrap());
        assert!(!is_newer(VERSION, VERSION).unwrap());
        assert!(!is_newer(VERSION, &newer).unwrap());
    }

    #[test]
    fn canonical_payload_sorts_keys() {
        let payload = json!({"b": 1, "a": "카카오"});
        assert_eq!(
            String::from_utf8(canonical_payload_python(&payload).unwrap()).unwrap(),
            "{\"a\":\"카카오\",\"b\":1}"
        );
    }

    #[test]
    fn artifact_url_is_pinned() {
        assert_eq!(
            expected_artifact_url("v11.0.2"),
            "https://github.com/twbeatles/kakaotalk-layout-adblocker/releases/download/v11.0.2/KakaoTalkLayoutAdBlocker_v11.exe"
        );
        assert!(is_valid_artifact_url(
            "https://github.com/twbeatles/kakaotalk-layout-adblocker/releases/download/v11.1.0/KakaoTalkLayoutAdBlocker_v11.exe",
            "v11.1.0"
        ));
        assert!(is_valid_artifact_url(
            "https://github.com/twbeatles/kakaotalk-pc-adblock-py/releases/download/v11.1.0/KakaoTalkLayoutAdBlocker_v11.exe",
            "v11.1.0"
        ));
        assert!(!is_valid_artifact_url(
            "https://malicious.example.com/KakaoTalkLayoutAdBlocker_v11.exe",
            "v11.1.0"
        ));
    }

    #[test]
    fn relaunch_args_keep_launch_flags_and_drop_smoke_flags() {
        assert_eq!(
            relaunch_args_from(["--startup-launch", "--minimized"]),
            vec!["--startup-launch".to_string(), "--minimized".to_string()]
        );
        assert!(relaunch_args_from(["--startup-trace", "C:\t.json"]).is_empty());
        assert!(relaunch_args_from(["--exit-after-startup-ms", "500"]).is_empty());
        assert!(relaunch_args_from(Vec::<&str>::new()).is_empty());
    }

    #[test]
    fn unique_staging_paths_differ() {
        let left = unique_staging_path("KakaoTalkLayoutAdBlocker_v11_update", "11.1.2");
        let right = unique_staging_path("KakaoTalkLayoutAdBlocker_v11_update", "11.1.2");
        assert_ne!(left, right);
        assert!(left
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("11.1.2"));
    }

    #[test]
    fn resolve_helper_rejects_temp_placeholder_and_accepts_mz() {
        let dir = std::env::temp_dir().join(format!("kakao_helper_resolve_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake_app = dir.join("app.exe");
        std::fs::write(&fake_app, b"MZ-app").unwrap();
        assert!(resolve_helper(&fake_app).is_err());

        let helper = dir.join("kakao-updater.exe");
        std::fs::write(&helper, b"MZ-helper-bytes").unwrap();
        let found = resolve_helper(&fake_app).unwrap();
        assert_eq!(found, helper);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn try_begin_update_is_single_flight() {
        end_update();
        assert!(try_begin_update());
        assert!(!try_begin_update());
        end_update();
        assert!(try_begin_update());
        end_update();
    }

    #[test]
    fn verifies_published_v11_1_0_manifest() {
        let doc = r#"{
    "payload": {
        "artifact_url": "https://github.com/twbeatles/kakaotalk-pc-adblock-py/releases/download/v11.1.0/KakaoTalkLayoutAdBlocker_v11.exe",
        "expires_at": "2027-09-02T13:41:52Z",
        "sha256": "7dad779564b43d7d7009f40367e78891a5f56dc1e8cab7d149de90318d26d28d",
        "size": 4316672,
        "tag": "v11.1.0",
        "version": "11.1.0"
    },
    "signature": "KGAgTr2SgsgtMNrE6kvZklJOr9IjU9PtMOhRcGi2bipnN7jod9+0Vs6UujH/dd9unffdvu5+Kfh8ArgyBNhvCQ=="
}"#;
        // Against an older version, update should be available
        let manifest = parse_and_verify_manifest(doc.as_bytes(), "11.0.1").unwrap();
        assert_eq!(manifest.version, "11.1.0");

        // Against current version 11.1.0, it should recognize it as latest (NoUpdate)
        let err = parse_and_verify_manifest(doc.as_bytes(), "11.1.0").unwrap_err();
        assert!(matches!(err, UpdateError::NoUpdate));
    }
}
