use crate::graph::WindowGraph;
use crate::layout::{
    contains_any_ad_token, is_bottom_banner_candidate, is_chrome_widget_class,
    should_close_empty_eva_child,
};
use crate::model::{
    AdDecision, AdSignals, CandidateState, DecisionStrength, Hwnd, PopupGuard, Rect,
};
use crate::rules::LayoutRules;

pub fn popup_host_text_matches(rules: &LayoutRules, text: &str, text_known: bool) -> bool {
    if !text_known {
        return false;
    }
    let normalized = text.trim();
    if normalized.is_empty() {
        return true;
    }
    let text_lc = normalized.to_lowercase();
    if rules
        .popup_host_text_contains
        .iter()
        .any(|token| !token.is_empty() && text_lc.contains(&token.to_lowercase()))
    {
        return true;
    }
    !rules.popup_host_require_empty_text
}

pub fn popup_host_guard_status(rules: &LayoutRules, text: &str, text_known: bool) -> PopupGuard {
    if popup_host_text_matches(rules, text, text_known) {
        PopupGuard::Allow
    } else {
        PopupGuard::Blocked
    }
}

pub fn popup_dismiss_decision(guard: PopupGuard, depth: i32) -> AdDecision {
    let mut signals = AdSignals::blank();
    signals.popup_direct_class = depth == 1;
    signals.popup_descendant_class = true;
    signals.popup_match_depth = i64::from(depth);
    signals.popup_host_guard = guard.as_str().to_string();
    if guard == PopupGuard::Allow {
        AdDecision::dismiss_popup(signals)
    } else {
        AdDecision::none(signals)
    }
}

pub fn legacy_hide_decision(kind: &str) -> AdDecision {
    let mut signals = AdSignals::blank();
    signals.legacy_signature = kind.to_string();
    match kind {
        "exact" => AdDecision::hide(DecisionStrength::Strong, signals),
        "substring" => AdDecision::hide(DecisionStrength::Weak, signals),
        _ => AdDecision::none(signals),
    }
}

pub fn aggressive_hide_decision(
    rules: &LayoutRules,
    class_name: &str,
    child_rect: Option<Rect>,
    parent_rect: Rect,
    has_ad_token: bool,
) -> AdDecision {
    let mut signals = AdSignals::blank();
    signals.subtree_ad_token = has_ad_token;
    let is_chrome_widget = is_chrome_widget_class(rules, class_name);
    let is_bottom_banner = child_rect
        .map(|rect| is_chrome_widget && is_bottom_banner_candidate(rules, rect, parent_rect))
        .unwrap_or(false);
    signals.chrome_widget_bottom_banner = is_bottom_banner;
    if is_bottom_banner && has_ad_token {
        return AdDecision::hide(DecisionStrength::Strong, signals);
    }
    if is_bottom_banner && rules.hide_bottom_banner_without_token {
        return AdDecision::hide(DecisionStrength::Strong, signals);
    }
    if is_chrome_widget && has_ad_token {
        return AdDecision::hide(DecisionStrength::Weak, signals);
    }
    AdDecision::none(signals)
}

pub fn empty_eva_close_decision(
    rules: &LayoutRules,
    class_name: &str,
    window_text: &str,
    parent_text: &str,
    has_custom_scroll: bool,
    has_ad_signal: bool,
) -> AdDecision {
    let mut signals = AdSignals::blank();
    if class_name == rules.eva_child_class
        && window_text.is_empty()
        && !parent_text.is_empty()
        && !has_custom_scroll
    {
        signals.empty_eva_child = true;
    }
    if should_close_empty_eva_child(
        rules,
        class_name,
        window_text,
        parent_text,
        has_custom_scroll,
        has_ad_signal,
    ) {
        AdDecision::close(DecisionStrength::Weak, signals)
    } else {
        AdDecision::none(signals)
    }
}

pub fn subtree_contains_ad_token(
    graph: &WindowGraph,
    rules: &LayoutRules,
    hwnd: Hwnd,
    max_depth: i32,
) -> bool {
    subtree_has_ad_token(graph, &rules.aggressive_ad_tokens_lc(), hwnd, max_depth)
}

fn subtree_has_ad_token(
    graph: &WindowGraph,
    tokens_lc: &[String],
    hwnd: Hwnd,
    max_depth: i32,
) -> bool {
    if max_depth < 0 {
        return false;
    }
    let Some(node) = graph.get(hwnd) else {
        return false;
    };
    if contains_any_ad_token(tokens_lc, node.text()) {
        return true;
    }
    graph
        .children_of(hwnd)
        .iter()
        .any(|&child| subtree_has_ad_token(graph, tokens_lc, child, max_depth - 1))
}

pub fn class_name_starts_with(
    graph: &WindowGraph,
    hwnd: Hwnd,
    prefix: &str,
    max_depth: i32,
) -> bool {
    if max_depth < 0 {
        return false;
    }
    let Some(node) = graph.get(hwnd) else {
        return false;
    };
    if node.class_name.starts_with(prefix) {
        return true;
    }
    graph
        .children_of(hwnd)
        .iter()
        .any(|&child| class_name_starts_with(graph, child, prefix, max_depth - 1))
}

pub fn has_window_text(graph: &WindowGraph, hwnd: Hwnd, target: &str, max_depth: i32) -> bool {
    if max_depth < 0 {
        return false;
    }
    let Some(node) = graph.get(hwnd) else {
        return false;
    };
    if node.text() == target {
        return true;
    }
    graph
        .children_of(hwnd)
        .iter()
        .any(|&child| has_window_text(graph, child, target, max_depth - 1))
}

pub fn has_window_text_contains(
    graph: &WindowGraph,
    hwnd: Hwnd,
    target: &str,
    max_depth: i32,
) -> bool {
    let needle = target.to_lowercase();
    if needle.is_empty() {
        return false;
    }
    subtree_text_contains(graph, hwnd, &needle, max_depth)
}

/// Recursive part of `has_window_text_contains` with the needle lowercased
/// once by the caller instead of at every node.
fn subtree_text_contains(graph: &WindowGraph, hwnd: Hwnd, needle_lc: &str, max_depth: i32) -> bool {
    if max_depth < 0 {
        return false;
    }
    let Some(node) = graph.get(hwnd) else {
        return false;
    };
    let text = node.text();
    if !text.is_empty() && text.to_lowercase().contains(needle_lc) {
        return true;
    }
    graph
        .children_of(hwnd)
        .iter()
        .any(|&child| subtree_text_contains(graph, child, needle_lc, max_depth - 1))
}

pub fn legacy_signature_kind(graph: &WindowGraph, rules: &LayoutRules, hwnd: Hwnd) -> String {
    if has_window_text(graph, hwnd, &rules.chrome_legacy_title, 8) {
        return "exact".to_string();
    }
    for token in &rules.chrome_legacy_title_contains {
        if token.is_empty() {
            continue;
        }
        if has_window_text_contains(graph, hwnd, token, 8) {
            return "substring".to_string();
        }
    }
    String::new()
}

pub fn update_candidate_state(
    state: &mut CandidateState,
    decision: &AdDecision,
    weak_signal_confirm_ticks: i64,
) -> bool {
    if decision.matched() {
        state.match_streak += 1;
        state.miss_streak = 0;
    } else {
        state.match_streak = 0;
        state.miss_streak += 1;
    }
    decision.matched()
        && (decision.decision == DecisionStrength::Strong
            || state.match_streak >= weak_signal_confirm_ticks.max(1))
}

pub fn payload_action(decision: &AdDecision, confirmed: bool) -> String {
    if decision.matched() && !confirmed {
        format!("pending_{}", decision.action.as_str())
    } else {
        decision.action.as_str().to_string()
    }
}

pub fn find_popup_matches(
    graph: &WindowGraph,
    rules: &LayoutRules,
    host_hwnd: Hwnd,
    require_visible: bool,
) -> Vec<(Hwnd, i32, String)> {
    let mut matches = Vec::new();
    for (hwnd, depth) in graph.enum_descendants(host_hwnd, rules.popup_search_depth) {
        let Some(node) = graph.get(hwnd) else {
            continue;
        };
        if require_visible && !node.visible {
            continue;
        }
        if !rules
            .popup_ad_classes
            .iter()
            .any(|cls| cls == &node.class_name)
        {
            continue;
        }
        matches.push((hwnd, depth, node.class_name.clone()));
    }
    matches
}

pub fn is_main_title(rules: &LayoutRules, text: &str) -> bool {
    !text.is_empty() && rules.main_window_titles.iter().any(|title| title == text)
}

pub fn has_main_view_signature(
    graph: &WindowGraph,
    rules: &LayoutRules,
    parent_hwnd: Hwnd,
) -> bool {
    graph.enum_children(parent_hwnd).into_iter().any(|hwnd| {
        graph.get(hwnd).is_some_and(|node| {
            node.class_name == rules.eva_child_class
                && (node.text().starts_with(&rules.main_view_prefix)
                    || node.text().starts_with(&rules.lock_view_prefix))
        })
    })
}

pub fn structural_main_window_candidate(
    rules: &LayoutRules,
    class_name: &str,
    win32_parent: Hwnd,
) -> bool {
    rules
        .main_window_classes
        .iter()
        .any(|cls| cls == class_name)
        && win32_parent == 0
}
