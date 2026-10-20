//! Rule boundary tests.
//!
//! Every edge below is computed from the live `LayoutRules::default()`
//! values — not from copied constants — so a rule change forces these tests
//! to move with it (or fail loudly). Each test also kills a concrete mutant:
//! `>=`↔`>`, `&&`↔`||`, guard removal, or confirm-tick bypass.

use std::collections::HashMap;

use kakao_core::layout::{
    contains_ad_token, is_bottom_banner_candidate, is_chrome_widget_class, planned_view_resize,
    should_close_empty_eva_child,
};
use kakao_core::model::PopupGuard;
use kakao_core::signals::{
    class_name_starts_with, empty_eva_close_decision, find_popup_matches, has_main_view_signature,
    has_window_text, has_window_text_contains, legacy_hide_decision, popup_host_guard_status,
    popup_host_text_matches, structural_main_window_candidate, subtree_contains_ad_token,
    update_candidate_state,
};
use kakao_core::{
    CandidateState, LayoutRules, LayoutSettings, Rect, WindowGraph, WindowNode, WindowText,
    evaluate_graph_with_states,
};

fn rect(l: i32, t: i32, r: i32, b: i32) -> Rect {
    Rect::from_xyxy([l, t, r, b])
}

fn node(
    hwnd: i64,
    pid: i64,
    class: &str,
    text: &str,
    parent: Option<i64>,
    rect: Option<Rect>,
) -> WindowNode {
    WindowNode {
        hwnd,
        pid,
        class_name: class.to_string(),
        title: WindowText::Known(text.to_string()),
        structural_parent: parent,
        owner: None,
        rect,
        visible: true,
    }
}

#[test]
fn banner_geometry_edges_come_from_live_rule_values() {
    let rules = LayoutRules::default();
    let parent = rect(0, 0, 500, 700);
    assert_eq!(rules.banner_min_height_px, 40);
    assert_eq!(rules.banner_max_height_px, 260);

    // Height edges: exactly on the boundary counts, one pixel outside does not.
    // (Kills the `height < min` ↔ `height <= min` mutant both ways.)
    let full_width = |height: i32| rect(0, 700 - height, 500, 700);
    assert!(!is_bottom_banner_candidate(
        &rules,
        full_width(rules.banner_min_height_px - 1),
        parent
    ));
    assert!(is_bottom_banner_candidate(
        &rules,
        full_width(rules.banner_min_height_px),
        parent
    ));
    assert!(is_bottom_banner_candidate(
        &rules,
        full_width(rules.banner_max_height_px),
        parent
    ));
    assert!(!is_bottom_banner_candidate(
        &rules,
        full_width(rules.banner_max_height_px + 1),
        parent
    ));

    // Width-ratio edges at exactly 0.75 of a 500px parent.
    let bottom_strip = |width: i32| rect(0, 620, width, 700);
    assert!(!is_bottom_banner_candidate(
        &rules,
        bottom_strip(374),
        parent
    ));
    assert!(is_bottom_banner_candidate(
        &rules,
        bottom_strip(375),
        parent
    ));
    assert!(is_bottom_banner_candidate(
        &rules,
        bottom_strip(376),
        parent
    ));

    // Bottom-margin edges: |child.bottom - parent.bottom| of 40 is in, 41 out.
    let margin = |bottom: i32| rect(0, bottom - 80, 500, bottom);
    assert!(is_bottom_banner_candidate(&rules, margin(660), parent));
    assert!(is_bottom_banner_candidate(&rules, margin(700), parent));
    assert!(is_bottom_banner_candidate(&rules, margin(740), parent));
    assert!(!is_bottom_banner_candidate(&rules, margin(659), parent));
    assert!(!is_bottom_banner_candidate(&rules, margin(741), parent));

    // Degenerate parent: no division, no banner.
    let zero_parent = rect(0, 0, 0, 700);
    assert!(!is_bottom_banner_candidate(
        &rules,
        rect(0, 620, 0, 700),
        zero_parent
    ));
}

fn banner_graph(banner_height: i32, token_text: &str) -> WindowGraph {
    let mut graph = WindowGraph::empty(vec![42]);
    graph.insert_node(node(
        100,
        42,
        "EVA_Window_Dblclk",
        "카카오톡",
        None,
        Some(rect(0, 0, 500, 700)),
    ));
    graph.insert_node(node(
        101,
        42,
        "EVA_ChildWindow",
        "OnlineMainView",
        Some(100),
        Some(rect(0, 0, 500, 669)),
    ));
    graph.insert_node(node(
        102,
        42,
        "Chrome_WidgetWin_1",
        "",
        Some(100),
        Some(rect(0, 700 - banner_height, 500, 700)),
    ));
    graph.insert_node(node(
        103,
        42,
        "Static",
        token_text,
        Some(102),
        Some(rect(0, 700 - banner_height, 500, 700)),
    ));
    graph.set_children(100, vec![101, 102]);
    graph.set_children(102, vec![103]);
    graph
}

fn aggressive_on() -> LayoutSettings {
    LayoutSettings {
        enabled: true,
        aggressive_mode: true,
    }
}

#[test]
fn banner_hides_at_min_height_and_weak_confirms_below_it() {
    let rules = LayoutRules::default();
    let settings = aggressive_on();

    let mut states = HashMap::new();
    let at_min =
        evaluate_graph_with_states(&banner_graph(40, "광고"), &settings, &rules, &mut states);
    assert!(
        at_min.actions.hide.contains(&102),
        "exact min-height banner with a token is Strong: hide on tick 1"
    );

    // One pixel below min height the Strong banner path is gone, but the
    // chrome-widget + token Weak path still confirms on tick 2.
    let mut states = HashMap::new();
    for tick in 1..=3 {
        let below =
            evaluate_graph_with_states(&banner_graph(39, "광고"), &settings, &rules, &mut states);
        if tick == 1 {
            assert!(
                !below.actions.hide.contains(&102),
                "below-min banner must stay pending on tick 1"
            );
        } else {
            assert!(
                below.actions.hide.contains(&102),
                "below-min banner with a token confirms via the Weak path (tick {tick})"
            );
        }
    }

    // The same geometry without any ad token never hides.
    let mut states = HashMap::new();
    for tick in 1..=3 {
        let clean = evaluate_graph_with_states(
            &banner_graph(39, "친구 목록"),
            &settings,
            &rules,
            &mut states,
        );
        assert!(
            !clean.actions.hide.contains(&102),
            "tokenless geometry must never hide (tick {tick})"
        );
    }
}

#[test]
fn weak_signal_confirm_ticks_and_miss_reset() {
    let weak = legacy_hide_decision("substring");
    assert!(weak.matched());

    // confirm_ticks = 1: immediate.
    let mut state = CandidateState::default();
    assert!(update_candidate_state(&mut state, &weak, 1));
    assert_eq!(state.match_streak, 1);

    // confirm_ticks = 2: pending, then confirmed; a miss resets the streak.
    let mut state = CandidateState::default();
    assert!(!update_candidate_state(&mut state, &weak, 2));
    assert!(update_candidate_state(&mut state, &weak, 2));
    let none = legacy_hide_decision("");
    assert!(!update_candidate_state(&mut state, &none, 2));
    assert_eq!(state.match_streak, 0);
    assert_eq!(state.miss_streak, 1);
    assert!(!update_candidate_state(&mut state, &weak, 2));
    assert!(update_candidate_state(&mut state, &weak, 2));

    // confirm_ticks = 3: needs three consecutive matches.
    let mut state = CandidateState::default();
    assert!(!update_candidate_state(&mut state, &weak, 3));
    assert!(!update_candidate_state(&mut state, &weak, 3));
    assert!(update_candidate_state(&mut state, &weak, 3));
}

fn popup_chain(ad_visible: bool) -> WindowGraph {
    let mut graph = WindowGraph::empty(vec![42]);
    graph.insert_node(node(200, 42, "EVA_Window", "", None, None));
    graph.insert_node(node(201, 42, "Pane", "", Some(200), None));
    graph.insert_node(node(202, 42, "Pane", "", Some(201), None));
    let mut ad = node(203, 42, "AdFitWebView", "", Some(202), None);
    ad.visible = ad_visible;
    graph.insert_node(ad);
    graph.set_children(200, vec![201]);
    graph.set_children(201, vec![202]);
    graph.set_children(202, vec![203]);
    graph
}

#[test]
fn popup_search_depth_edges_and_visibility() {
    let graph = popup_chain(true);
    let depth0 = LayoutRules::default().overlay(&serde_json::json!({"popup_search_depth": 0}));
    assert!(
        find_popup_matches(&graph, &depth0, 200, false).is_empty(),
        "depth 0 must be blind even with an AdFit descendant"
    );

    let depth2 = LayoutRules::default();
    assert_eq!(depth2.popup_search_depth, 2);
    assert!(
        find_popup_matches(&graph, &depth2, 200, false).is_empty(),
        "depth-3 descendant must stay invisible at the default depth 2"
    );

    let depth3 = LayoutRules::default().overlay(&serde_json::json!({"popup_search_depth": 3}));
    assert_eq!(
        find_popup_matches(&graph, &depth3, 200, false),
        vec![(203, 3, "AdFitWebView".to_string())]
    );

    // Visibility filter: a hidden AdFit node is skipped when required.
    let hidden = popup_chain(false);
    assert!(
        find_popup_matches(&hidden, &depth3, 200, true).is_empty(),
        "require_visible must skip a hidden popup match"
    );
}

#[test]
fn popup_host_text_guard_edges() {
    let rules = LayoutRules::default();
    assert!(rules.popup_host_require_empty_text);
    assert_eq!(
        popup_host_guard_status(&rules, "", true),
        PopupGuard::Allow,
        "empty host title passes the guard"
    );
    assert_eq!(
        popup_host_guard_status(&rules, "사진 보기", true),
        PopupGuard::Blocked,
        "a titled viewer host must block dismissal"
    );
    assert_eq!(
        popup_host_guard_status(&rules, "", false),
        PopupGuard::Blocked,
        "unknown text never passes, even when empty"
    );

    let lenient = LayoutRules::default()
        .overlay(&serde_json::json!({"popup_host_require_empty_text": false}));
    assert_eq!(
        popup_host_guard_status(&lenient, "사진 보기", true),
        PopupGuard::Allow,
        "flipping require_empty_text must visibly change the verdict"
    );

    let listed =
        LayoutRules::default().overlay(&serde_json::json!({"popup_host_text_contains": ["보기"]}));
    assert_eq!(
        popup_host_guard_status(&listed, "사진 보기", true),
        PopupGuard::Allow,
        "listed host text passes without relaxing the empty-text rule"
    );
}

#[test]
fn short_ad_token_needs_a_word_boundary() {
    let rules = LayoutRules::default();
    assert!(rules.aggressive_ad_tokens.iter().any(|t| t == "Ad"));

    assert!(contains_ad_token(&rules, "Ad"));
    assert!(contains_ad_token(&rules, "sponsored Ad here"));
    assert!(contains_ad_token(&rules, "AdFit NAS Advertisement"));
    assert!(contains_ad_token(&rules, "오늘의 광고"));
    assert!(contains_ad_token(&rules, "무광고상태"));

    // Kills the word-boundary-removal mutant: without it "Adobe" matches "Ad".
    assert!(!contains_ad_token(&rules, "Adobe Reader"));
    assert!(!contains_ad_token(&rules, "Address book"));
    assert!(!contains_ad_token(&rules, "add"));
    assert!(!contains_ad_token(&rules, ""));
    assert!(!contains_ad_token(&rules, "친구 목록"));
}

#[test]
fn tokenless_aggressive_rules_cannot_manufacture_a_banner_match() {
    // Removing every token must not increase matches: the banner geometry
    // alone stays inert without the explicit tokenless opt-in.
    let dump = std::fs::read_to_string(
        repo_root().join("tests/fixtures/contract/positives/bottom_banner_with_token.json"),
    )
    .unwrap();
    let graph = WindowGraph::from_dump_json(&dump).unwrap();
    let tokenless =
        LayoutRules::default().overlay(&serde_json::json!({"aggressive_ad_tokens": []}));
    let mut states = HashMap::new();
    let eval = evaluate_graph_with_states(&graph, &aggressive_on(), &tokenless, &mut states);
    assert!(
        !eval.actions.hide.contains(&1302),
        "without tokens the banner geometry alone must not hide"
    );
}

fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repo root")
}

// --- mutant-killing contract pins (cargo-mutants 27.1.0 survivors, 2026-10-12) ---

#[test]
fn chrome_widget_class_gate_is_not_universal() {
    let rules = LayoutRules::default();
    assert!(is_chrome_widget_class(&rules, "Chrome_WidgetWin_1"));
    // Kills `is_chrome_widget_class -> true`: ordinary windows must stay out.
    assert!(!is_chrome_widget_class(&rules, "EVA_ChildWindow"));
    assert!(!is_chrome_widget_class(&rules, "EVA_Window_Dblclk"));
    assert!(!is_chrome_widget_class(&rules, ""));
}

#[test]
fn long_ascii_tokens_still_match_as_substrings() {
    let rules = LayoutRules::default();
    // Kills `&&` -> `||` in the short-token gate: routing every ASCII token
    // through the word-boundary path would miss "Advertisement" inside a
    // larger word (a false negative the fixtures never show).
    assert!(contains_ad_token(&rules, "MyAdvertisementX"));
    assert!(contains_ad_token(&rules, "XXAdFitYY"));
}

#[test]
fn empty_eva_close_needs_every_arm() {
    let rules = LayoutRules::default();
    let eva = rules.eva_child_class.clone();
    let yes = || should_close_empty_eva_child(&rules, &eva, "", "카카오톡", false, true);
    assert!(yes(), "all arms true with a signal must close");

    // Each single-false arm must veto, even with a signal present.
    assert!(!should_close_empty_eva_child(
        &rules,
        "Other",
        "",
        "카카오톡",
        false,
        true
    ));
    assert!(!should_close_empty_eva_child(
        &rules,
        &eva,
        "x",
        "카카오톡",
        false,
        true
    ));
    assert!(!should_close_empty_eva_child(
        &rules, &eva, "", "", false, true
    ));
    assert!(!should_close_empty_eva_child(
        &rules,
        &eva,
        "",
        "카카오톡",
        true,
        true
    ));
    // A match without an ad signal must not close under the default gate.
    assert!(!should_close_empty_eva_child(
        &rules,
        &eva,
        "",
        "카카오톡",
        false,
        false
    ));
    // A non-match with a signal must not close either.
    assert!(!should_close_empty_eva_child(
        &rules,
        "Other",
        "",
        "카카오톡",
        false,
        true
    ));

    // Relaxing the gate still requires a match.
    let lax = LayoutRules::default()
        .overlay(&serde_json::json!({"close_empty_eva_child_requires_ad_signal": false}));
    assert!(should_close_empty_eva_child(
        &lax,
        &eva,
        "",
        "카카오톡",
        false,
        false
    ));
    assert!(!should_close_empty_eva_child(
        &lax,
        "Other",
        "",
        "카카오톡",
        false,
        false
    ));
}

#[test]
fn empty_eva_decision_sets_signal_only_on_match() {
    let rules = LayoutRules::default();
    let eva = rules.eva_child_class.clone();
    let hit = empty_eva_close_decision(&rules, &eva, "", "카카오톡", false, true);
    assert!(hit.matched());
    assert!(hit.signals.empty_eva_child);

    // Kills `&&` -> `||` in the decision mirror: a non-EVA class must leave
    // the signal flag clear so no candidate state is recorded for it.
    let miss = empty_eva_close_decision(&rules, "Other", "", "카카오톡", false, true);
    assert!(!miss.matched());
    assert!(!miss.signals.empty_eva_child);
    assert!(!miss.signals.has_relevant_signal());
}

#[test]
fn planned_view_resize_rejects_degenerate_widths() {
    let rules = LayoutRules::default();
    // Parent width 500 -> planned 498x669.
    assert!(planned_view_resize(&rules, "OnlineMainView", rect(0, 0, 500, 700), None).is_some());
    // Parent widths 3/2/1 -> planned widths 1/0/-1: only 1 survives.
    // Kills `< 1` -> `== 1`/`<= 1` both ways.
    assert!(planned_view_resize(&rules, "OnlineMainView", rect(0, 0, 3, 700), None).is_some());
    assert!(planned_view_resize(&rules, "OnlineMainView", rect(0, 0, 2, 700), None).is_none());
    assert!(planned_view_resize(&rules, "OnlineMainView", rect(0, 0, 1, 700), None).is_none());
    // Height side (padding 31): parent heights 32/31/30 -> planned 1/0/-1.
    // Kills `height < 1` -> `== 1`/`<= 1` both ways.
    assert!(planned_view_resize(&rules, "OnlineMainView", rect(0, 0, 500, 32), None).is_some());
    assert!(planned_view_resize(&rules, "OnlineMainView", rect(0, 0, 500, 31), None).is_none());
    assert!(planned_view_resize(&rules, "OnlineMainView", rect(0, 0, 500, 30), None).is_none());
    // Non-view text never resizes, whatever the rect.
    assert!(planned_view_resize(&rules, "친구 목록", rect(0, 0, 500, 700), None).is_none());
}

#[test]
fn empty_listed_host_token_does_not_match_everything() {
    // Kills `&&` -> `||` in popup_host_text_matches: an empty listed token
    // must stay inert instead of allowing every host.
    let rules =
        LayoutRules::default().overlay(&serde_json::json!({"popup_host_text_contains": [""]}));
    assert!(!popup_host_text_matches(&rules, "사진 보기", true));
    // An empty host passes through the dedicated empty-text arm, never
    // through the empty listed token.
    assert!(popup_host_text_matches(&rules, "", true));
}

fn deep_chain(depth: usize, leaf_text: &str, leaf_class: &str) -> WindowGraph {
    // Host 500 with a single-file descendant chain 501..; the leaf carries
    // the signal at exactly `depth` edges below the host.
    let mut graph = WindowGraph::empty(vec![42]);
    graph.insert_node(node(500, 42, "EVA_Window", "", None, None));
    let mut parent = 500;
    for level in 1..=depth {
        let hwnd = 500 + level as i64;
        let last = level == depth;
        graph.insert_node(node(
            hwnd,
            42,
            if last { leaf_class } else { "Pane" },
            if last { leaf_text } else { "" },
            Some(parent),
            None,
        ));
        let mut kids = graph.enum_children(parent);
        kids.push(hwnd);
        graph.set_children(parent, kids);
        parent = hwnd;
    }
    graph
}

#[test]
fn subtree_walkers_stop_exactly_at_the_depth_limit() {
    let rules = LayoutRules::default();
    // Signal exactly at the documented limit (8) is found …
    let at_limit = deep_chain(8, "Chrome Legacy Window", "Static");
    assert!(has_window_text(&at_limit, 500, "Chrome Legacy Window", 8));
    assert!(has_window_text_contains(&at_limit, 500, "chrome legacy", 8));
    let token = deep_chain(8, "광고", "Static");
    assert!(subtree_contains_ad_token(&token, &rules, 500, 8));
    let scroll = deep_chain(8, "", "_EVA_Scroll");
    assert!(class_name_starts_with(&scroll, 500, "_EVA_", 8));
    // … one edge deeper is out of reach. Kills `< 0` -> `==`/`<=` (which go
    // blind at the limit) and `- 1` -> `+ 1` (which search past it).
    let past_limit = deep_chain(9, "Chrome Legacy Window", "Static");
    assert!(!has_window_text(
        &past_limit,
        500,
        "Chrome Legacy Window",
        8
    ));
    assert!(!has_window_text_contains(
        &past_limit,
        500,
        "chrome legacy",
        8
    ));
    let past_token = deep_chain(9, "광고", "Static");
    assert!(!subtree_contains_ad_token(&past_token, &rules, 500, 8));
    let past_scroll = deep_chain(9, "", "_EVA_Scroll");
    assert!(!class_name_starts_with(&past_scroll, 500, "_EVA_", 8));
}

#[test]
fn popup_search_requires_visibility_only_when_asked() {
    let graph = popup_chain(true);
    let depth3 = LayoutRules::default().overlay(&serde_json::json!({"popup_search_depth": 3}));
    // Kills `&&` -> `||` in the visibility filter: with visibility required,
    // a visible match must still be returned.
    assert_eq!(
        find_popup_matches(&graph, &depth3, 200, true),
        vec![(203, 3, "AdFitWebView".to_string())]
    );
}

#[test]
fn main_view_signature_and_candidacy_reject_lookalikes() {
    let rules = LayoutRules::default();
    let mut graph = WindowGraph::empty(vec![42]);
    graph.insert_node(node(600, 42, "EVA_Window_Dblclk", "카카오톡", None, None));
    graph.insert_node(node(
        601,
        42,
        "EVA_ChildWindow",
        "친구 목록",
        Some(600),
        None,
    ));
    graph.set_children(600, vec![601]);
    // Kills `&&` -> `||`: an EVA child without the view prefix is not a signal.
    assert!(!has_main_view_signature(&graph, &rules, 600));

    // Kills `==` -> `!=`: an EVA-class window with a real parent is not a
    // top-level main candidate.
    assert!(structural_main_window_candidate(&rules, "EVA_Window", 0));
    assert!(!structural_main_window_candidate(&rules, "EVA_Window", 600));
    assert!(!structural_main_window_candidate(
        &rules,
        "Chrome_WidgetWin_1",
        0
    ));
}
