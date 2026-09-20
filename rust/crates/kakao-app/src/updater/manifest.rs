use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde_json::Value;

use super::canonical::canonical_payload_python;
use super::error::UpdateError;
use super::model::{
    UpdateManifest, LEGACY_RELEASE_DOWNLOAD_PREFIX, MAX_ARTIFACT_BYTES, MAX_MANIFEST_BYTES,
    RELEASE_DOWNLOAD_PREFIX,
};
use super::version::is_newer;
use crate::config::UPDATE_PUBLIC_KEY_B64;

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

#[cfg(test)]
mod tests {
    use super::*;

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
