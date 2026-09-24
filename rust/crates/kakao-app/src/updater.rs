// Pure-move split of the former monolithic `updater.rs` (SRP).
//
// - `error`: `UpdateError`
// - `model`: manifest/staged types + endpoint/size/timeout constants
// - `version`: version-tuple parsing + `is_newer`
// - `canonical`: canonical JSON payload encoding (incl. Python-compatible)
// - `manifest`: signed-manifest fetch-shape verification
// - `http`: download/verify + update check over HTTP
// - `staging`: single-flight guard, helper resolution, staging, launch
//
// No verification/download semantics changed; this file only re-exports the
// previous public surface.
mod canonical;
mod error;
mod http;
mod manifest;
mod model;
mod staging;
mod version;

pub use canonical::{canonical_payload, canonical_payload_python};
pub use error::UpdateError;
pub use http::{check_for_update, download_and_verify, sha256_hex};
pub use manifest::{expected_artifact_url, is_valid_artifact_url, parse_and_verify_manifest};
pub use model::{
    StagedUpdate, UpdateManifest, LEGACY_RELEASE_DOWNLOAD_PREFIX, MANIFEST_URL,
    RELEASE_DOWNLOAD_PREFIX, USER_AGENT,
};
pub use staging::{
    apply_update, discard_staged, end_update, launch_helper, prepare_update, relaunch_args_from,
    resolve_helper, stage_helper, try_begin_update, unique_staging_path, unix_now,
    update_in_progress,
};
pub use version::{is_newer, version_tuple};
