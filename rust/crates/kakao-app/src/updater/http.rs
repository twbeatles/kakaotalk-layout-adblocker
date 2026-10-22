use std::io::Read;

use sha2::{Digest, Sha256};

use super::error::UpdateError;
use super::manifest::parse_and_verify_manifest;
use super::model::{
    HTTP_CONNECT_TIMEOUT, HTTP_READ_TIMEOUT, HTTP_TOTAL_TIMEOUT, MANIFEST_URL, MAX_MANIFEST_BYTES,
    USER_AGENT, UpdateManifest,
};
use crate::config::VERSION;

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn http_agent() -> ureq::Agent {
    let config = ureq::config::Config::builder()
        .timeout_connect(Some(HTTP_CONNECT_TIMEOUT))
        .timeout_recv_response(Some(HTTP_READ_TIMEOUT))
        .timeout_recv_body(Some(HTTP_READ_TIMEOUT))
        .timeout_global(Some(HTTP_TOTAL_TIMEOUT))
        .user_agent(USER_AGENT)
        .build();
    ureq::Agent::new_with_config(config)
}

fn transient(err: &ureq::Error) -> bool {
    use std::io::ErrorKind;
    match err {
        ureq::Error::StatusCode(408 | 429 | 500 | 502 | 503 | 504)
        | ureq::Error::Timeout(_)
        | ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed => true,
        ureq::Error::Io(err) => matches!(
            err.kind(),
            ErrorKind::TimedOut
                | ErrorKind::Interrupted
                | ErrorKind::WouldBlock
                | ErrorKind::ConnectionReset
                | ErrorKind::ConnectionRefused
                | ErrorKind::ConnectionAborted
                | ErrorKind::UnexpectedEof
                | ErrorKind::NotConnected
        ),
        _ => false,
    }
}

fn retry<T>(
    mut request: impl FnMut() -> Result<T, ureq::Error>,
    mut sleep: impl FnMut(std::time::Duration),
) -> Result<T, ureq::Error> {
    for delay in [200, 500] {
        match request() {
            Err(err) if transient(&err) => sleep(std::time::Duration::from_millis(delay)),
            result => return result,
        }
    }
    request()
}

fn fetch(url: &str, limit: u64) -> Result<Vec<u8>, UpdateError> {
    retry(
        || {
            let response = http_agent().get(url).call()?;
            let mut bytes = Vec::new();
            response
                .into_body()
                .into_reader()
                .take(limit.saturating_add(1))
                .read_to_end(&mut bytes)
                .map_err(ureq::Error::Io)?;
            Ok(bytes)
        },
        std::thread::sleep,
    )
    .map_err(|err| UpdateError::Message(format!("업데이트 다운로드 실패: {err}")))
}

pub fn download_and_verify(
    manifest: &UpdateManifest,
    dest: &std::path::Path,
) -> Result<(), UpdateError> {
    let bytes = fetch(&manifest.artifact_url, manifest.size)?;
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
    let body = fetch(MANIFEST_URL, MAX_MANIFEST_BYTES as u64)?;
    parse_and_verify_manifest(&body, VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retries_transient_failures_but_not_invalid_requests() {
        let mut attempts = 0;
        let mut delays = Vec::new();
        let result = retry(
            || {
                attempts += 1;
                if attempts < 3 {
                    Err(ureq::Error::StatusCode(503))
                } else {
                    Ok(42)
                }
            },
            |d| delays.push(d.as_millis()),
        );
        assert_eq!(result.unwrap(), 42);
        assert_eq!(delays, [200, 500]);
        let mut attempts = 0;
        let result: Result<(), _> = retry(
            || {
                attempts += 1;
                Err(ureq::Error::StatusCode(404))
            },
            |_| panic!("must not retry"),
        );
        assert!(result.is_err());
        assert_eq!(attempts, 1);
    }
    #[test]
    fn retries_are_finite_and_integrity_failures_are_not_transport_errors() {
        let mut attempts = 0;
        let result: Result<(), _> = retry(
            || {
                attempts += 1;
                Err(ureq::Error::StatusCode(429))
            },
            |_| {},
        );
        assert!(result.is_err());
        assert_eq!(attempts, 3);
        assert!(!transient(&ureq::Error::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "denied"
        ))));
    }
}
