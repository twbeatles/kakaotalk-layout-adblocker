//! PROJECT_AUDIT 2026-09-24 P3: `build_graph` now enumerates each top-level
//! window once instead of once per node. These tests pin that the resulting
//! graph is identical to the previous per-node algorithm.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::PathBuf;

use kakao_app::graph_build::build_graph;
use kakao_core::{Hwnd, WindowGraph};
use kakao_win32::{FakeWin32, Win32Api};

/// The pre-P3 algorithm, kept verbatim as the reference: enumerate every
/// node's descendants and keep those whose `GetParent` is that node.
fn reference_children(api: &dyn Win32Api, pids: &[i64]) -> BTreeMap<Hwnd, Vec<Hwnd>> {
    let pid_set: HashSet<i64> = pids.iter().copied().collect();
    let mut top_level = Vec::new();
    api.enum_windows(&mut |hwnd| {
        if pid_set.contains(&api.get_window_thread_process_id(hwnd)) {
            top_level.push(hwnd);
        }
        true
    });
    top_level.sort_unstable();
    let mut out = BTreeMap::new();
    let mut visited = HashSet::new();
    let mut stack: Vec<Hwnd> = top_level.into_iter().rev().collect();
    while let Some(hwnd) = stack.pop() {
        if !visited.insert(hwnd) || !api.is_window(hwnd) {
            continue;
        }
        let mut children = Vec::new();
        api.enum_child_windows(hwnd, &mut |child| {
            if api.get_parent(child) == hwnd {
                children.push(child);
            }
            true
        });
        for &child in children.iter().rev() {
            stack.push(child);
        }
        out.insert(hwnd, children);
    }
    out
}

fn graph_children(graph: &WindowGraph) -> BTreeMap<Hwnd, Vec<Hwnd>> {
    graph
        .nodes
        .keys()
        .map(|hwnd| (*hwnd, graph.enum_children(*hwnd)))
        .collect()
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repo root")
}

#[test]
fn single_pass_graph_matches_the_per_node_algorithm_on_every_fixture() {
    let dir = repo_root().join("tests/fixtures/window_dumps");
    let mut fixtures: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    fixtures.sort();
    assert!(!fixtures.is_empty());
    for path in fixtures {
        let api = FakeWin32::from_dump_json(&fs::read_to_string(&path).unwrap()).unwrap();
        // Real EnumChildWindows returns all descendants; the reference must
        // see the same shape to be a fair comparison.
        api.set_flatten_enum_children(true);
        let pids = api.pids();
        let graph = build_graph(&api, &pids);
        assert_eq!(
            graph_children(&graph),
            reference_children(&api, &pids),
            "graph differs for {}",
            path.display()
        );
    }
}

/// Read-only comparison against the live desktop. Ignored by default because
/// windows can appear or vanish between the two passes; run manually with
/// `cargo test -p kakao-app --test graph_build_parity -- --ignored`.
#[cfg(windows)]
#[test]
#[ignore]
fn single_pass_graph_matches_the_per_node_algorithm_on_the_live_desktop() {
    let api = kakao_win32::RealWin32::new();
    let mut pids: HashSet<i64> = HashSet::new();
    api.enum_windows(&mut |hwnd| {
        pids.insert(api.get_window_thread_process_id(hwnd));
        true
    });
    let mut pids: Vec<i64> = pids.into_iter().filter(|pid| *pid > 0).collect();
    pids.sort_unstable();
    let mut compared_nodes = 0usize;
    for attempt in 0..5 {
        let reference = reference_children(&api, &pids);
        let graph = build_graph(&api, &pids);
        let actual = graph_children(&graph);
        if actual == reference {
            compared_nodes = actual.len();
            break;
        }
        assert!(attempt < 4, "live graph differed in 5 consecutive attempts");
    }
    assert!(compared_nodes > 0);
    eprintln!("live desktop parity ok: {compared_nodes} nodes");
}
