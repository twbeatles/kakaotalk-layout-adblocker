//! Independent contract oracle.
//!
//! The Python-Rust golden fixtures prove *equivalence* of two
//! implementations, not that either verdict is right. This test proves the
//! verdicts themselves: every case in
//! `tests/fixtures/contract/manifest.json` carries a human-reviewed label
//! written from the documented decision contract, and the loader refuses
//! unknown fields, unknown case kinds, and expectations that reference HWNDs
//! absent from the fixture — so a typo can never silently pass.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use kakao_core::{
    CandidateState, LayoutRules, LayoutSettings, WindowGraph, WindowIdentity,
    evaluate_graph_with_states,
};
use serde::Deserialize;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repo root")
}

fn contract_dir() -> PathBuf {
    repo_root().join("tests/fixtures/contract")
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContractManifest {
    version: u32,
    cases: Vec<ContractCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContractCase {
    id: String,
    kind: String,
    fixture: String,
    settings: LayoutSettings,
    rules_overrides: serde_json::Value,
    ticks: u64,
    expect: ContractExpect,
    label_basis: String,
    reviewed_by: String,
    reviewed_at: String,
    capture_note: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContractExpect {
    required_hide: Vec<i64>,
    required_close: Vec<i64>,
    must_not_hide: Vec<i64>,
    must_not_close: Vec<i64>,
    must_not_zero_size: Vec<i64>,
    #[serde(default)]
    pending_hide: Vec<i64>,
    #[serde(default)]
    pending_close: Vec<i64>,
}

fn load_manifest() -> ContractManifest {
    let text = fs::read_to_string(contract_dir().join("manifest.json")).expect("manifest.json");
    serde_json::from_str(&text).expect("manifest must parse with strict schema")
}

fn is_zero_size_entry(pos: &[i64], hwnd: i64) -> bool {
    pos.len() >= 5 && pos[0] == hwnd && pos[3] == 0 && pos[4] == 0
}

fn graph_hwnds(graph: &WindowGraph) -> HashSet<i64> {
    graph.nodes.keys().copied().collect()
}

#[test]
fn contract_manifest_metadata_is_complete_and_unique() {
    let manifest = load_manifest();
    assert_eq!(manifest.version, 1, "bump the loader with the schema");
    assert!(!manifest.cases.is_empty(), "no contract cases");
    let mut ids = HashSet::new();
    for case in &manifest.cases {
        assert!(
            ids.insert(case.id.clone()),
            "duplicate contract case id: {}",
            case.id
        );
        assert!(
            case.kind == "positive" || case.kind == "negative",
            "case {} has unknown kind {:?}",
            case.id,
            case.kind
        );
        assert!(
            case.ticks >= 1,
            "case {} must run at least one tick",
            case.id
        );
        for field in [
            &case.label_basis,
            &case.reviewed_by,
            &case.reviewed_at,
            &case.capture_note,
        ] {
            assert!(
                !field.trim().is_empty(),
                "case {} has empty metadata",
                case.id
            );
        }
        let date = case.reviewed_at.trim();
        let valid_date = date.len() == 10
            && date.as_bytes()[4] == b'-'
            && date.as_bytes()[7] == b'-'
            && date[..4].chars().all(|ch| ch.is_ascii_digit())
            && date[5..7].chars().all(|ch| ch.is_ascii_digit())
            && date[8..].chars().all(|ch| ch.is_ascii_digit());
        assert!(
            valid_date,
            "case {} reviewed_at must be YYYY-MM-DD, got {:?}",
            case.id, case.reviewed_at
        );
    }
}

#[test]
fn contract_oracle_expectations_hold() {
    let manifest = load_manifest();
    for case in &manifest.cases {
        let dump_path = contract_dir().join(&case.fixture);
        let dump = fs::read_to_string(&dump_path)
            .unwrap_or_else(|err| panic!("case {}: read {}: {err}", case.id, dump_path.display()));
        let graph = WindowGraph::from_dump_json(&dump)
            .unwrap_or_else(|err| panic!("case {}: parse dump: {err}", case.id));
        let live = graph_hwnds(&graph);

        // Every referenced HWND must really exist: a stale expectation that
        // names a removed window must fail loudly instead of passing vacuously.
        for hwnd in case
            .expect
            .required_hide
            .iter()
            .chain(&case.expect.required_close)
            .chain(&case.expect.must_not_hide)
            .chain(&case.expect.must_not_close)
            .chain(&case.expect.must_not_zero_size)
        {
            assert!(
                live.contains(hwnd),
                "case {} references hwnd {hwnd} absent from {}",
                case.id,
                case.fixture
            );
        }

        let rules = LayoutRules::default().overlay(&case.rules_overrides);
        let mut states: HashMap<WindowIdentity, CandidateState> = HashMap::new();
        let mut tick_hides: Vec<HashSet<i64>> = Vec::new();
        let mut tick_closes: Vec<HashSet<i64>> = Vec::new();
        let mut tick_zero_sized: Vec<HashSet<i64>> = Vec::new();
        for _ in 0..case.ticks {
            let eval = evaluate_graph_with_states(&graph, &case.settings, &rules, &mut states);
            tick_hides.push(eval.actions.hide.iter().copied().collect());
            tick_closes.push(eval.actions.close.iter().copied().collect());
            tick_zero_sized.push(
                live.iter()
                    .copied()
                    .filter(|hwnd| {
                        eval.actions
                            .set_pos
                            .iter()
                            .any(|pos| is_zero_size_entry(pos, *hwnd))
                    })
                    .collect(),
            );
        }
        let last = usize::try_from(case.ticks).unwrap() - 1;

        for hwnd in &case.expect.required_hide {
            assert!(
                tick_hides[last].contains(hwnd),
                "case {} ({}): hwnd {hwnd} must be hidden on the last tick (basis: {})",
                case.id,
                case.fixture,
                case.label_basis
            );
        }
        for hwnd in &case.expect.required_close {
            assert!(
                tick_closes[last].contains(hwnd),
                "case {} ({}): hwnd {hwnd} must be close-requested on the last tick (basis: {})",
                case.id,
                case.fixture,
                case.label_basis
            );
        }
        // Confirmation duty: pending verdicts stay quiet on tick 1 (a weak
        // signal) and land on the last tick. Strong verdicts in
        // `required_*` may act immediately, so they are not pending.
        for hwnd in &case.expect.pending_hide {
            assert!(
                !tick_hides[0].contains(hwnd),
                "case {}: hwnd {hwnd} hid on tick 1 but needs {}-tick confirmation",
                case.id,
                case.ticks
            );
            assert!(
                tick_hides[last].contains(hwnd),
                "case {}: hwnd {hwnd} never hid after {} ticks",
                case.id,
                case.ticks
            );
        }
        for hwnd in &case.expect.pending_close {
            assert!(
                !tick_closes[0].contains(hwnd),
                "case {}: hwnd {hwnd} closed on tick 1 but needs {}-tick confirmation",
                case.id,
                case.ticks
            );
            assert!(
                tick_closes[last].contains(hwnd),
                "case {}: hwnd {hwnd} never closed after {} ticks",
                case.id,
                case.ticks
            );
        }
        // Negative assertions hold on EVERY tick, not just the last one.
        for (index, hides) in tick_hides.iter().enumerate() {
            for hwnd in &case.expect.must_not_hide {
                assert!(
                    !hides.contains(hwnd),
                    "case {}: hwnd {hwnd} hidden on tick {} — forbidden ({})",
                    case.id,
                    index + 1,
                    case.label_basis
                );
            }
        }
        for (index, closes) in tick_closes.iter().enumerate() {
            for hwnd in &case.expect.must_not_close {
                assert!(
                    !closes.contains(hwnd),
                    "case {}: hwnd {hwnd} close-requested on tick {} — forbidden ({})",
                    case.id,
                    index + 1,
                    case.label_basis
                );
            }
        }
        for (index, zeroed) in tick_zero_sized.iter().enumerate() {
            for hwnd in &case.expect.must_not_zero_size {
                assert!(
                    !zeroed.contains(hwnd),
                    "case {}: hwnd {hwnd} zero-sized on tick {} — forbidden ({})",
                    case.id,
                    index + 1,
                    case.label_basis
                );
            }
        }
    }
}
