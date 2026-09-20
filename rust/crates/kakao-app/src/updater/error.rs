use thiserror::Error;

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("{0}")]
    Message(String),
    #[error("현재 최신 버전을 사용 중입니다.")]
    NoUpdate,
}
