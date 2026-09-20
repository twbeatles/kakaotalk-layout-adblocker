use std::path::PathBuf;
use std::time::Duration;

pub const MANIFEST_URL: &str =
    "https://github.com/twbeatles/kakaotalk-layout-adblocker/releases/latest/download/update.json";
pub const USER_AGENT: &str = "KakaoTalkLayoutAdBlocker-Updater";
pub const RELEASE_DOWNLOAD_PREFIX: &str =
    "https://github.com/twbeatles/kakaotalk-layout-adblocker/releases/download/";
pub const LEGACY_RELEASE_DOWNLOAD_PREFIX: &str =
    "https://github.com/twbeatles/kakaotalk-pc-adblock-py/releases/download/";
pub(super) const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub(super) const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
pub(super) const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const HTTP_READ_TIMEOUT: Duration = Duration::from_secs(60);
pub(super) const HTTP_TOTAL_TIMEOUT: Duration = Duration::from_secs(90);
pub(super) static UPDATE_IN_PROGRESS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
pub(super) static STAGING_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateManifest {
    pub version: String,
    pub tag: String,
    pub artifact_url: String,
    pub sha256: String,
    pub size: u64,
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
