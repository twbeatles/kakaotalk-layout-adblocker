//! Deterministic fuzz invariants (no extra dependencies).
//!
//! A tiny seeded xorshift generator builds hostile window graphs — extreme
//! rects, hostile text, foreign PIDs, storms of siblings — and every seed
//! must satisfy the safety contract: no panic, no touch of foreign or
//! confirmed-healthy windows, byte-identical re-evaluation. Seeds are fixed,
//! so any failure reproduces exactly.

use std::collections::{HashMap, HashSet};

use kakao_core::{
    CandidateState, LayoutRules, LayoutSettings, Rect, WindowGraph, WindowIdentity, WindowNode,
    WindowText, evaluate_graph_with_states,
};

// --- minimal deterministic RNG (xorshift64*) --------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

// --- hostile graph generation -------------------------------------------------

const CLASSES: &[&str] = &[
    "EVA_Window_Dblclk",
    "EVA_Window",
    "EVA_ChildWindow",
    "Chrome_WidgetWin_0",
    "Chrome_WidgetWin_1",
    "Chrome_RenderWidgetHostHWND",
    "AdFitWebView",
    "Static",
    "Pane",
    "_EVA_CustomScroll",
    "Button",
    "Edit",
    "UnknownClass",
];

const TEXTS: &[&str] = &[
    "",
    "카카오톡",
    "KakaoTalk",
    "OnlineMainView_0x1",
    "LockModeView",
    "Chrome Legacy Window",
    "xx Chrome Legacy Window yy",
    "Ad",
    "ad",
    "AD",
    "AdFit",
    "Advertisement",
    "광고",
    "무광고상태",
    "Adobe Reader",
    "Address book",
    "add",
    "친구 목록",
    "사진 보기",
    "Footer Panel",
    " ",
    "　",
    "\u{0}",
    "a",
];

fn random_rect(rng: &mut Rng) -> Option<Rect> {
    match rng.below(8) {
        0 => None,
        1 => Some(Rect::from_xyxy([0, 0, 0, 0])),
        2 => {
            // Extreme coordinates, including adversarial orderings.
            let coords = [
                i32::MIN,
                -1_000_000,
                -1,
                0,
                1,
                500,
                700,
                1_000_000,
                i32::MAX,
            ];
            Some(Rect::from_xyxy([
                *rng.pick(&coords),
                *rng.pick(&coords),
                *rng.pick(&coords),
                *rng.pick(&coords),
            ]))
        }
        _ => {
            let l = rng.below(600) as i32;
            let t = rng.below(800) as i32;
            Some(Rect::from_xyxy([
                l,
                t,
                l + rng.below(600) as i32,
                t + rng.below(800) as i32,
            ]))
        }
    }
}

fn random_text(rng: &mut Rng) -> String {
    let base = rng.pick(TEXTS).to_string();
    match rng.below(6) {
        0 => base + &"가".repeat(rng.below(300) as usize),
        1 => "x".repeat(rng.below(2000) as usize) + &base,
        _ => base,
    }
}

fn gen_graph(seed: u64, node_count: usize) -> WindowGraph {
    let mut rng = Rng(seed.wrapping_add(0x9E37_79B9_7F4A_7C15));
    let mut graph = WindowGraph::empty(vec![42]);
    for index in 0..node_count {
        let hwnd = 1000 + index as i64;
        let parent = if index == 0 || rng.below(3) == 0 {
            None
        } else {
            Some(1000 + rng.below(index as u64) as i64)
        };
        let pid = if rng.below(6) == 0 { 9999 } else { 42 };
        let mut node = WindowNode {
            hwnd,
            pid,
            class_name: rng.pick(CLASSES).to_string(),
            title: WindowText::Known(random_text(&mut rng)),
            structural_parent: parent,
            owner: None,
            rect: random_rect(&mut rng),
            visible: rng.below(2) == 0,
        };
        if rng.below(20) == 0 {
            node.title = WindowText::Unknown { error_code: 5 };
        } else if rng.below(20) == 0 {
            node.title = WindowText::Truncated(random_text(&mut rng));
        }
        graph.insert_node(node);
    }
    // Rebuild child lists from the parent links (insertion order is shuffled
    // deterministically to also cover order variance).
    let mut order: Vec<i64> = graph.nodes.keys().copied().collect();
    for index in (1..order.len()).rev() {
        let swap = Rng(seed ^ (index as u64)).below(index as u64 + 1) as usize;
        order.swap(index, swap);
    }
    let mut children: HashMap<i64, Vec<i64>> = HashMap::new();
    for hwnd in &order {
        let parent = graph.get(*hwnd).and_then(|node| node.structural_parent);
        children.entry(*hwnd).or_default();
        if let Some(parent) = parent {
            children.entry(parent).or_default().push(*hwnd);
        }
    }
    for (parent, list) in children {
        graph.set_children(parent, list);
    }
    graph
}

fn settings_set(aggressive: bool) -> LayoutSettings {
    LayoutSettings {
        enabled: true,
        aggressive_mode: aggressive,
    }
}

fn zero_size_targets(eval: &kakao_core::Evaluation) -> HashSet<i64> {
    eval.actions
        .set_pos
        .iter()
        .filter(|pos| pos.len() >= 5 && pos[3] == 0 && pos[4] == 0)
        .map(|pos| pos[0])
        .collect()
}

#[test]
fn fuzz_never_panics_and_never_touches_foreign_windows() {
    for seed in 0..96u64 {
        for size in [0usize, 1, 5, 40] {
            let graph = gen_graph(seed * 1000 + size as u64, size);
            for aggressive in [false, true] {
                let rules = LayoutRules::default();
                let mut states: HashMap<WindowIdentity, CandidateState> = HashMap::new();
                for _ in 0..3 {
                    let eval = evaluate_graph_with_states(
                        &graph,
                        &settings_set(aggressive),
                        &rules,
                        &mut states,
                    );
                    for hwnd in eval
                        .actions
                        .hide
                        .iter()
                        .chain(eval.actions.close.iter())
                        .chain(zero_size_targets(&eval).iter())
                    {
                        // Layered contract: the core verdict may name a
                        // renderer-owned descendant (real KakaoTalk hosts CEF
                        // children under foreign renderer PIDs — see
                        // owned_popup_legacy_ad.json), but it must always
                        // reference a real node attached to a KakaoTalk tree.
                        // The engine's pid-gated precheck (covered at the
                        // tick level by f20) decides what Win32 really touches.
                        let mut cursor = graph.get(*hwnd);
                        let mut attached = false;
                        while let Some(node) = cursor {
                            if graph.pids.contains(&node.pid) {
                                attached = true;
                                break;
                            }
                            cursor = node.structural_parent.and_then(|parent| graph.get(parent));
                        }
                        assert!(
                            attached,
                            "seed {seed} size {size}: action targets detached hwnd {hwnd}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn fuzz_evaluation_is_deterministic() {
    for seed in 0..32u64 {
        let graph = gen_graph(7000 + seed, 25);
        let rules = LayoutRules::default();
        let mut first_states = HashMap::new();
        let mut second_states = HashMap::new();
        let first =
            evaluate_graph_with_states(&graph, &settings_set(true), &rules, &mut first_states);
        let second =
            evaluate_graph_with_states(&graph, &settings_set(true), &rules, &mut second_states);
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(&second).unwrap(),
            "seed {seed}: same input must evaluate identically"
        );
    }
}

#[test]
fn fuzz_sibling_storm_and_empty_graph_stay_stable() {
    let empty = WindowGraph::empty(vec![42]);
    let mut states = HashMap::new();
    let eval = evaluate_graph_with_states(
        &empty,
        &settings_set(true),
        &LayoutRules::default(),
        &mut states,
    );
    assert!(eval.actions.hide.is_empty());
    assert!(eval.actions.close.is_empty());

    let storm = gen_graph(12345, 300);
    let mut states = HashMap::new();
    for _ in 0..3 {
        let eval = evaluate_graph_with_states(
            &storm,
            &settings_set(true),
            &LayoutRules::default(),
            &mut states,
        );
        for hwnd in eval.actions.hide.iter().chain(eval.actions.close.iter()) {
            assert!(
                storm.get(*hwnd).is_some(),
                "storm action targets missing hwnd {hwnd}"
            );
        }
    }
}

#[test]
fn fuzz_confirmed_main_windows_are_never_hidden_or_closed() {
    // The P0 safety contract under adversarial input: whatever the noise, a
    // confirmed KakaoTalk main window itself is never a hide/close target.
    // (A main that literally contains ad text is a pathological separate
    // case; the generator keeps main titles clean so a hit here is a real FP.)
    for seed in 0..64u64 {
        let mut graph = gen_graph(seed, 12);
        // Plant one clean, confirmed main window.
        graph.insert_node(WindowNode {
            hwnd: 50,
            pid: 42,
            class_name: "EVA_Window_Dblclk".to_string(),
            title: WindowText::Known("카카오톡".to_string()),
            structural_parent: None,
            owner: None,
            rect: Some(Rect::from_xyxy([0, 0, 500, 700])),
            visible: true,
        });
        graph.insert_node(WindowNode {
            hwnd: 51,
            pid: 42,
            class_name: "EVA_ChildWindow".to_string(),
            title: WindowText::Known("OnlineMainView".to_string()),
            structural_parent: Some(50),
            owner: None,
            rect: Some(Rect::from_xyxy([0, 0, 500, 669])),
            visible: true,
        });
        let kids = {
            let mut list = graph.enum_children(50);
            if !list.contains(&51) {
                list.push(51);
            }
            list
        };
        graph.set_children(50, kids);

        let rules = LayoutRules::default();
        let mut states = HashMap::new();
        for _ in 0..3 {
            let eval = evaluate_graph_with_states(&graph, &settings_set(true), &rules, &mut states);
            let confirmed: HashSet<i64> = eval
                .main_windows
                .iter()
                .filter(|item| item.confirmed)
                .map(|item| item.hwnd)
                .collect();
            assert!(
                confirmed.contains(&50),
                "seed {seed}: planted main must confirm"
            );
            for hwnd in eval.actions.hide.iter().chain(eval.actions.close.iter()) {
                assert!(
                    !confirmed.contains(hwnd),
                    "seed {seed}: confirmed main window {hwnd} must never be hidden/closed"
                );
            }
        }
    }
}

#[test]
fn fuzz_hostile_config_overlays_fail_safe() {
    let hostile = [
        serde_json::json!({"banner_min_height_px": "tall"}),
        serde_json::json!({"banner_min_height_px": null}),
        serde_json::json!({"banner_min_height_px": -999999}),
        serde_json::json!({"banner_min_height_px": 999999}),
        serde_json::json!({"banner_min_width_ratio": "wide"}),
        serde_json::json!({"banner_min_width_ratio": f64::NAN}),
        serde_json::json!({"weak_signal_confirm_ticks": "two"}),
        serde_json::json!({"weak_signal_confirm_ticks": -5}),
        serde_json::json!({"popup_search_depth": "deep"}),
        serde_json::json!({"popup_search_depth": -100}),
        serde_json::json!({"aggressive_ad_tokens": "Ad"}),
        serde_json::json!({"aggressive_ad_tokens": [1, 2, 3]}),
        serde_json::json!({"no_such_field": true}),
        serde_json::json!("just a string"),
        serde_json::json!([1, 2, 3]),
        serde_json::json!(null),
    ];
    for (index, overlay) in hostile.iter().enumerate() {
        let (rules, warnings) = LayoutRules::default().overlay_with_warnings(overlay);
        // Never panics; wrong-typed values keep defaults with a warning.
        let _ = evaluate_graph_with_states(
            &gen_graph(index as u64, 8),
            &settings_set(true),
            &rules,
            &mut HashMap::new(),
        );
        let _ = warnings;
    }
    // Wrong-typed scalar keeps the default and warns (the fail-safe contract).
    let (rules, warnings) = LayoutRules::default()
        .overlay_with_warnings(&serde_json::json!({"banner_min_height_px": "tall"}));
    assert_eq!(rules.banner_min_height_px, 40);
    assert!(!warnings.is_empty());

    // Malformed dumps are errors, never panics.
    for bad in [
        "not json at all",
        "{\"pids\": \"nope\"}",
        "{\"pids\": [42], \"windows\": [{\"hwnd\": \"x\"}]}",
        "",
    ] {
        assert!(
            WindowGraph::from_dump_json(bad).is_err(),
            "malformed dump must be Err: {bad:?}"
        );
    }
}
