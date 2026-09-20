use std::collections::{HashMap, HashSet};

use crate::graph::WindowGraph;
use crate::model::{
    AdDecision, AdSignals, CandidateState, Hwnd, PopupGuard, Rect, WindowIdentity, WindowNode,
};
use crate::rules::{LayoutRules, LayoutSettings};
use crate::signals::{
    aggressive_hide_decision, class_name_starts_with, empty_eva_close_decision, find_popup_matches,
    has_main_view_signature, is_main_title, legacy_hide_decision, legacy_signature_kind,
    payload_action, popup_dismiss_decision, popup_host_guard_status,
    structural_main_window_candidate, subtree_contains_ad_token, update_candidate_state,
};

use super::payloads::{AdSignalsPayload, CandidatePayload, MainWindowPayload};

fn candidate_payload(
    identity: &WindowIdentity,
    decision: &AdDecision,
    state: &CandidateState,
    confirmed: bool,
) -> CandidatePayload {
    CandidatePayload {
        hwnd: identity.hwnd,
        pid: identity.pid,
        class: identity.class_name.clone(),
        signals: AdSignalsPayload::from(&decision.signals),
        decision: decision.decision.as_str().to_string(),
        action: payload_action(decision, confirmed),
        match_streak: state.match_streak,
        miss_streak: state.miss_streak,
    }
}

fn main_window_debug_payload(
    graph: &WindowGraph,
    rules: &LayoutRules,
    node: &WindowNode,
) -> MainWindowPayload {
    let structural_candidate =
        structural_main_window_candidate(rules, &node.class_name, node.win32_parent());
    let title_match = is_main_title(rules, node.text());
    let child_signature = structural_candidate && has_main_view_signature(graph, rules, node.hwnd);
    let confirmed = structural_candidate && child_signature;
    let confirmation = if confirmed {
        if title_match {
            "title-and-child-signature"
        } else {
            "child-signature-fallback"
        }
    } else {
        "rejected"
    };
    MainWindowPayload {
        hwnd: node.hwnd,
        pid: node.pid,
        class: node.class_name.clone(),
        text: node.text().to_string(),
        structural_candidate,
        title_match,
        child_signature,
        confirmed,
        confirmation: confirmation.to_string(),
    }
}

pub(super) fn collect_top_level(graph: &WindowGraph) -> Vec<&WindowNode> {
    let pids: HashSet<i64> = graph.pids.iter().copied().collect();
    graph
        .enum_windows()
        .into_iter()
        .filter_map(|hwnd| graph.get(hwnd))
        .filter(|node| pids.contains(&node.pid))
        .collect()
}

pub(super) fn inspect_main_windows(
    graph: &WindowGraph,
    rules: &LayoutRules,
) -> Vec<MainWindowPayload> {
    let mut payloads: Vec<MainWindowPayload> = collect_top_level(graph)
        .into_iter()
        .filter(|node| {
            structural_main_window_candidate(rules, &node.class_name, node.win32_parent())
        })
        .map(|node| main_window_debug_payload(graph, rules, node))
        .collect();
    payloads.sort_by_key(|item| item.hwnd);
    payloads
}

pub(super) fn candidate_handles(
    graph: &WindowGraph,
    rules: &LayoutRules,
    main_handles: &HashSet<Hwnd>,
) -> Vec<Hwnd> {
    let mut candidates = Vec::new();
    for node in collect_top_level(graph) {
        if !rules
            .ad_candidate_classes
            .iter()
            .any(|cls| cls == &node.class_name)
        {
            continue;
        }
        if main_handles.contains(&node.win32_parent()) {
            if node.text().is_empty() {
                candidates.push(node.hwnd);
            }
            continue;
        }
        if node.win32_parent() == 0 && !legacy_signature_kind(graph, rules, node.hwnd).is_empty() {
            candidates.push(node.hwnd);
        }
    }
    candidates
}

pub(super) fn store_update(
    store: &mut HashMap<WindowIdentity, CandidateState>,
    identity: WindowIdentity,
    decision: &AdDecision,
    ticks: i64,
) -> (CandidateState, bool) {
    let state = store.entry(identity.clone()).or_default();
    let confirmed = update_candidate_state(state, decision, ticks);
    (state.clone(), confirmed)
}

pub(super) fn inspect_candidates(
    graph: &WindowGraph,
    settings: &LayoutSettings,
    rules: &LayoutRules,
    confirmed_main: &[Hwnd],
    confirmed_set: &HashSet<Hwnd>,
    candidates: &[Hwnd],
    preview_states: &mut HashMap<WindowIdentity, CandidateState>,
) -> Vec<CandidatePayload> {
    let mut payloads = Vec::new();
    let ticks = rules.weak_signal_confirm_ticks;

    for &wnd in confirmed_main {
        let Some(parent) = graph.get(wnd) else {
            continue;
        };
        let Some(parent_rect) = parent.rect else {
            continue;
        };
        let parent_text = parent.text().to_string();
        let children = graph.enum_children(wnd);
        let mut main_window_has_ad_signal = false;
        let mut child_contexts: Vec<(
            Hwnd,
            WindowIdentity,
            String,
            String,
            Option<Rect>,
            AdDecision,
        )> = Vec::new();

        for child in &children {
            let Some(node) = graph.get(*child) else {
                continue;
            };
            if node.win32_parent() != wnd {
                continue;
            }
            let identity = node.identity();
            let mut child_rect = None;
            let mut aggressive_decision = AdDecision::none(AdSignals::blank());
            if settings.aggressive_mode {
                child_rect = node.rect;
                if let Some(rect) = child_rect {
                    let has_ad_token = subtree_contains_ad_token(graph, rules, *child, 8);
                    aggressive_decision = aggressive_hide_decision(
                        rules,
                        &node.class_name,
                        Some(rect),
                        parent_rect,
                        has_ad_token,
                    );
                }
            }
            let legacy_kind = if rules.close_empty_eva_child_requires_ad_signal {
                legacy_signature_kind(graph, rules, *child)
            } else {
                String::new()
            };
            if !legacy_kind.is_empty() || aggressive_decision.matched() {
                main_window_has_ad_signal = true;
            }
            child_contexts.push((
                *child,
                identity,
                node.class_name.clone(),
                node.text().to_string(),
                child_rect,
                aggressive_decision,
            ));
        }

        for (child, identity, class_name, window_text, child_rect, aggressive_decision) in
            child_contexts
        {
            if class_name == rules.eva_child_class
                && window_text.is_empty()
                && !parent_text.is_empty()
            {
                let has_custom_scroll =
                    class_name_starts_with(graph, child, &rules.custom_scroll_prefix, 8);
                let close_decision = empty_eva_close_decision(
                    rules,
                    &class_name,
                    &window_text,
                    &parent_text,
                    has_custom_scroll,
                    main_window_has_ad_signal,
                );
                if close_decision.matched()
                    || preview_states.contains_key(&identity)
                    || close_decision.signals.has_relevant_signal()
                {
                    let (state, confirmed) =
                        store_update(preview_states, identity.clone(), &close_decision, ticks);
                    payloads.push(candidate_payload(
                        &identity,
                        &close_decision,
                        &state,
                        confirmed,
                    ));
                }
            }

            if !settings.aggressive_mode || child_rect.is_none() {
                continue;
            }
            if aggressive_decision.matched()
                || preview_states.contains_key(&identity)
                || aggressive_decision.signals.has_relevant_signal()
            {
                let (state, confirmed) = store_update(
                    preview_states,
                    identity.clone(),
                    &aggressive_decision,
                    ticks,
                );
                payloads.push(candidate_payload(
                    &identity,
                    &aggressive_decision,
                    &state,
                    confirmed,
                ));
            }
        }
    }

    for &wnd in candidates {
        let Some(node) = graph.get(wnd) else {
            continue;
        };
        let identity = node.identity();
        let legacy_kind = legacy_signature_kind(graph, rules, wnd);
        let legacy_decision = legacy_hide_decision(&legacy_kind);
        if legacy_decision.matched()
            || preview_states.contains_key(&identity)
            || legacy_decision.signals.has_relevant_signal()
        {
            let (state, confirmed) =
                store_update(preview_states, identity.clone(), &legacy_decision, ticks);
            payloads.push(candidate_payload(
                &identity,
                &legacy_decision,
                &state,
                confirmed,
            ));
        }
    }

    for item in collect_top_level(graph) {
        if item.win32_parent() != 0 {
            continue;
        }
        if confirmed_set.contains(&item.hwnd) {
            continue;
        }
        let popup_guard = popup_host_guard_status(rules, item.text(), item.text_known());
        for (child, depth, class_name) in find_popup_matches(graph, rules, item.hwnd, false) {
            let child_pid = graph.get(child).map(|node| node.pid).unwrap_or(0);
            let identity = WindowIdentity {
                hwnd: child,
                pid: child_pid,
                class_name,
            };
            let host_identity = item.identity();
            let popup_decision = popup_dismiss_decision(popup_guard, depth);
            if popup_guard == PopupGuard::Allow {
                let (host_state, host_confirmed) = store_update(
                    preview_states,
                    host_identity.clone(),
                    &popup_decision,
                    ticks,
                );
                payloads.push(candidate_payload(
                    &host_identity,
                    &popup_decision,
                    &host_state,
                    host_confirmed,
                ));
            }
            let (popup_state, popup_confirmed) =
                store_update(preview_states, identity.clone(), &popup_decision, ticks);
            payloads.push(candidate_payload(
                &identity,
                &popup_decision,
                &popup_state,
                popup_confirmed,
            ));
        }
    }

    payloads.sort_by(|left, right| {
        left.hwnd
            .cmp(&right.hwnd)
            .then_with(|| left.action.cmp(&right.action))
            .then_with(|| left.decision.cmp(&right.decision))
    });
    payloads
}
