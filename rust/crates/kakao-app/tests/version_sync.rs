//! Recurrence guard for version drift.
//!
//! The version lives in three places. `scripts/build_release.ps1` compares only
//! two of them and only at release-build time, so bumping one place could
//! pass local checks and fail the release build. This test checks all three
//! against `config::VERSION` inside the `rust-core` CI job.

use std::fs;
use std::path::PathBuf;

use kakao_app::config::VERSION;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repo root")
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

#[test]
fn version_is_three_numeric_components() {
    let parts: Vec<&str> = VERSION.split('.').collect();
    assert_eq!(
        parts.len(),
        3,
        "VERSION must be major.minor.patch: {VERSION}"
    );
    for part in parts {
        assert!(
            !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()),
            "VERSION component must be numeric: {VERSION}"
        );
    }
}

#[test]
fn cargo_workspace_version_matches() {
    let manifest = read("rust/Cargo.toml");
    let expected = format!("version = \"{VERSION}\"");
    assert!(
        manifest.contains(&expected),
        "rust/Cargo.toml workspace.package.version must be {VERSION}"
    );
}

#[test]
fn windows_version_resource_matches() {
    let resource = read("packaging/windows_version_info.txt");
    let parts: Vec<&str> = VERSION.split('.').collect();
    let tuple = format!("({}, {}, {}, 0)", parts[0], parts[1], parts[2]);
    for needle in [
        format!("filevers={tuple}"),
        format!("prodvers={tuple}"),
        format!("StringStruct(u\"FileVersion\", u\"{VERSION}.0\")"),
        format!("StringStruct(u\"ProductVersion\", u\"{VERSION}.0\")"),
    ] {
        assert!(
            resource.contains(&needle),
            "packaging/windows_version_info.txt is missing: {needle}"
        );
    }
}

#[test]
fn changelog_documents_the_current_version() {
    let changelog = read("CHANGELOG.md");
    assert!(
        changelog.contains(&format!("## {VERSION} ")),
        "CHANGELOG.md has no '## {VERSION}' section"
    );
}
