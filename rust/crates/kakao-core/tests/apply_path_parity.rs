//! The engine worker uses `evaluate_graph_for_apply`, which skips the
//! `candidates` diagnostic payload. That shortcut is only safe while the
//! applied `actions` and `state` stay byte-identical to the full evaluation,
//! so every fixture is checked both ways here.

use std::fs;
use std::path::PathBuf;

use kakao_core::{
    evaluate_graph_for_apply, evaluate_graph_with_states, GoldenFile, LayoutRules, WindowGraph,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repo root")
}

fn goldens() -> Vec<GoldenFile> {
    let dir = repo_root().join("tests/fixtures/golden");
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .expect("golden dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no golden files found");
    paths
        .into_iter()
        .map(|path| {
            let text = fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
            serde_json::from_str(&text)
                .unwrap_or_else(|err| panic!("parse {}: {err}", path.display()))
        })
        .collect()
}

fn load_graph(fixture: &str) -> WindowGraph {
    let path = repo_root()
        .join("tests/fixtures/window_dumps")
        .join(fixture);
    let dump =
        fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    WindowGraph::from_dump_json(&dump).unwrap_or_else(|err| panic!("{fixture}: {err}"))
}

#[test]
fn apply_only_path_matches_full_evaluation_actions_and_state() {
    for golden in goldens() {
        let graph = load_graph(&golden.fixture);
        let rules = LayoutRules::default().overlay(&golden.rules_overrides);

        let mut full_states = std::collections::HashMap::new();
        let full = evaluate_graph_with_states(&graph, &golden.settings, &rules, &mut full_states);

        let mut apply_states = std::collections::HashMap::new();
        let apply = evaluate_graph_for_apply(&graph, &golden.settings, &rules, &mut apply_states);

        assert_eq!(
            serde_json::to_value(&apply.actions).unwrap(),
            serde_json::to_value(&full.actions).unwrap(),
            "actions diverged for {}",
            golden.fixture
        );
        assert_eq!(
            serde_json::to_value(&apply.state).unwrap(),
            serde_json::to_value(&full.state).unwrap(),
            "state diverged for {}",
            golden.fixture
        );
        assert_eq!(
            serde_json::to_value(&apply.main_windows).unwrap(),
            serde_json::to_value(&full.main_windows).unwrap(),
            "main_windows diverged for {}",
            golden.fixture
        );
        assert!(
            apply.candidates.is_empty(),
            "apply-only path must not build the diagnostic payload for {}",
            golden.fixture
        );
    }
}

#[test]
fn apply_only_path_keeps_candidate_state_transitions() {
    // Weak signals need `weak_signal_confirm_ticks` consecutive matches. The
    // apply path owns that streak, so repeated ticks must converge the same way
    // with and without the diagnostic payload.
    for golden in goldens() {
        let graph = load_graph(&golden.fixture);
        let rules = LayoutRules::default().overlay(&golden.rules_overrides);

        let mut full_states = std::collections::HashMap::new();
        let mut apply_states = std::collections::HashMap::new();
        for tick in 0..4 {
            let full =
                evaluate_graph_with_states(&graph, &golden.settings, &rules, &mut full_states);
            let apply =
                evaluate_graph_for_apply(&graph, &golden.settings, &rules, &mut apply_states);
            assert_eq!(
                serde_json::to_value(&apply.actions).unwrap(),
                serde_json::to_value(&full.actions).unwrap(),
                "actions diverged for {} on tick {tick}",
                golden.fixture
            );
            assert_eq!(
                serde_json::to_value(&apply.state).unwrap(),
                serde_json::to_value(&full.state).unwrap(),
                "state diverged for {} on tick {tick}",
                golden.fixture
            );
        }
    }
}
