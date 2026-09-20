use std::io::Read;

use sha2::{Digest, Sha256};

use super::error::UpdateError;
use super::manifest::parse_and_verify_manifest;
use super::model::{
    UpdateManifest, HTTP_CONNECT_TIMEOUT, HTTP_READ_TIMEOUT, HTTP_TOTAL_TIMEOUT, MANIFEST_URL,
    USER_AGENT,
};
use crate::config::VERSION;

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

pub fn check_for_update() -> Result<UpdateManifest, UpdateError> {
    let body = http_agent()
        .get(MANIFEST_URL)
        .call()
        .map_err(|err| UpdateError::Message(format!("업데이트 정보 다운로드 실패: {err}")))?
        .into_string()
        .map_err(|err| UpdateError::Message(format!("업데이트 정보 다운로드 실패: {err}")))?;
    parse_and_verify_manifest(body.as_bytes(), VERSION)
}
