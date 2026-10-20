use std::fs;
use std::path::{Path, PathBuf};

use kakao_core::{GoldenFile, LayoutRules, evaluate_dump};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repo root")
}

fn golden_dir() -> PathBuf {
    repo_root().join("tests/fixtures/golden")
}

fn dump_dir() -> PathBuf {
    repo_root().join("tests/fixtures/window_dumps")
}

fn load_golden(path: &Path) -> GoldenFile {
    let text =
        fs::read_to_string(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|err| panic!("parse {}: {err}", path.display()))
}

#[test]
fn python_golden_fixtures_match_rust_evaluation() {
    let mut goldens: Vec<PathBuf> = fs::read_dir(golden_dir())
        .expect("golden dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect();
    goldens.sort();
    assert!(!goldens.is_empty(), "no golden files found");

    for path in goldens {
        let golden = load_golden(&path);
        let dump_path = dump_dir().join(&golden.fixture);
        let dump = fs::read_to_string(&dump_path)
            .unwrap_or_else(|err| panic!("read {}: {err}", dump_path.display()));
        let rules = LayoutRules::default().overlay(&golden.rules_overrides);
        let actual = evaluate_dump(&dump, &golden.settings, &rules)
            .unwrap_or_else(|err| panic!("{}: {err}", golden.fixture));
        let actual_value = serde_json::to_value(&actual).expect("serialize actual");
        let expected_value = serde_json::to_value(&golden.expected).expect("serialize expected");
        assert_eq!(
            actual_value, expected_value,
            "parity mismatch for {}",
            golden.fixture
        );
    }
}
