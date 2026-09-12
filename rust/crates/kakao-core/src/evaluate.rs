use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::graph::WindowGraph;
use crate::layout::planned_view_resize;
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdSignalsPayload {
    pub legacy_signature: String,
    pub popup_direct_class: bool,
    pub popup_descendant_class: bool,
    pub popup_match_depth: i64,
    pub chrome_widget_bottom_banner: bool,
    pub subtree_ad_token: bool,
    pub empty_eva_child: bool,
    pub popup_host_guard: String,
}

impl From<&AdSignals> for AdSignalsPayload {
    fn from(signals: &AdSignals) -> Self {
        Self {
            legacy_signature: signals.legacy_signature.clone(),
            popup_direct_class: signals.popup_direct_class,
            popup_descendant_class: signals.popup_descendant_class,
            popup_match_depth: signals.popup_match_depth,
            chrome_widget_bottom_banner: signals.chrome_widget_bottom_banner,
            subtree_ad_token: signals.subtree_ad_token,
            empty_eva_child: signals.empty_eva_child,
            popup_host_guard: signals.popup_host_guard.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MainWindowPayload {
    pub hwnd: Hwnd,
    pub pid: i64,
    pub class: String,
    pub text: String,
    pub structural_candidate: bool,
    pub title_match: bool,
    pub child_signature: bool,
    pub confirmed: bool,
    pub confirmation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidatePayload {
    pub hwnd: Hwnd,
    pub pid: i64,
    pub class: String,
    pub signals: AdSignalsPayload,
    pub decision: String,
    pub action: String,
    pub match_streak: i64,
    pub miss_streak: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ActionLog {
    pub hide: Vec<Hwnd>,
    pub show: Vec<Hwnd>,
    pub close: Vec<Hwnd>,
    pub set_pos: Vec<Vec<i64>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EngineStatePayload {
    pub main_window_count: i64,
    pub candidate_main_window_count: i64,
    pub hidden_windows: i64,
    pub closed_windows: i64,
    pub resized_windows: i64,
    pub popup_close_requests: i64,
    pub popup_hide_fallbacks: i64,
    pub popup_zero_size_fallbacks: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Evaluation {
    pub main_windows: Vec<MainWindowPayload>,
    pub candidates: Vec<CandidatePayload>,
    pub actions: ActionLog,
    pub state: EngineStatePayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoldenFile {
    pub fixture: String,
    pub settings: LayoutSettings,
    pub rules_overrides: serde_json::Value,
    pub expected: Evaluation,
}

struct MutationLog {
    hide: Vec<Hwnd>,
    show: Vec<Hwnd>,
    close: Vec<Hwnd>,
    set_pos: Vec<Vec<i64>>,
    hidden: i64,
    closed: i64,
    resized: i64,
    popup_close_requests: i64,
    popup_hide_fallbacks: i64,
    popup_zero_size_fallbacks: i64,
    visible: HashMap<Hwnd, bool>,
}

impl MutationLog {
    fn new(graph: &WindowGraph) -> Self {
        Self {
            hide: Vec::new(),
            show: Vec::new(),
            close: Vec::new(),
            set_pos: Vec::new(),
            hidden: 0,
            closed: 0,
            resized: 0,
            popup_close_requests: 0,
            popup_hide_fallbacks: 0,
            popup_zero_size_fallbacks: 0,
            visible: graph
                .nodes
                .iter()
                .map(|(hwnd, node)| (*hwnd, node.visible))
                .collect(),
        }
    }

    fn hide_window(&mut self, hwnd: Hwnd) {
        self.hide.push(hwnd);
        self.visible.insert(hwnd, false);
    }

    fn set_pos(&mut self, hwnd: Hwnd, x: i32, y: i32, width: i32, height: i32) {
        self.set_pos.push(vec![
            hwnd,
            i64::from(x),
            i64::from(y),
            i64::from(width),
            i64::from(height),
        ]);
    }

    /// Empty `EVA_ChildWindow` close request. Popup dismissals are counted by
    /// `popup_close_requests` instead, so `closed` stays specific to this path.
    fn send_close(&mut self, hwnd: Hwnd) {
        self.close.push(hwnd);
        self.closed += 1;
    }

    fn send_popup_close(&mut self, hwnd: Hwnd) {
        self.close.push(hwnd);
        self.popup_close_requests += 1;
    }

    fn unique_sorted(mut values: Vec<Hwnd>) -> Vec<Hwnd> {
        values.sort_unstable();
        values.dedup();
        values
    }
}

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

fn collect_top_level(graph: &WindowGraph) -> Vec<&WindowNode> {
    let pids: HashSet<i64> = graph.pids.iter().copied().collect();
    graph
        .enum_windows()
        .into_iter()
        .filter_map(|hwnd| graph.get(hwnd))
        .filter(|node| pids.contains(&node.pid))
        .collect()
}

fn inspect_main_windows(graph: &WindowGraph, rules: &LayoutRules) -> Vec<MainWindowPayload> {
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

fn candidate_handles(
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

fn store_update(
    store: &mut HashMap<WindowIdentity, CandidateState>,
    identity: WindowIdentity,
    decision: &AdDecision,
    ticks: i64,
) -> (CandidateState, bool) {
    let state = store.entry(identity.clone()).or_default();
    let confirmed = update_candidate_state(state, decision, ticks);
    (state.clone(), confirmed)
}

fn inspect_candidates(
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

fn apply_once(
    graph: &WindowGraph,
    settings: &LayoutSettings,
    rules: &LayoutRules,
    main_handles: &[Hwnd],
    confirmed_set: &HashSet<Hwnd>,
    candidates: &[Hwnd],
    states: &mut HashMap<WindowIdentity, CandidateState>,
) -> MutationLog {
    let mut log = MutationLog::new(graph);
    let ticks = rules.weak_signal_confirm_ticks;
    let kakao_pids: HashSet<i64> = graph.pids.iter().copied().collect();

    for wnd in main_handles {
        let Some(parent) = graph.get(*wnd) else {
            continue;
        };
        if !kakao_pids.contains(&parent.pid) {
            continue;
        }
        let Some(parent_rect) = parent.rect else {
            continue;
        };
        let parent_text = parent.text().to_string();
        let children = graph.enum_children(*wnd);
        let mut main_window_has_ad_signal = false;
        let mut child_contexts = Vec::new();

        for child in &children {
            let Some(node) = graph.get(*child) else {
                continue;
            };
            if node.win32_parent() != *wnd {
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
                node.rect,
            ));
        }

        for (
            child,
            identity,
            class_name,
            window_text,
            child_rect,
            aggressive_decision,
            current_rect,
        ) in child_contexts
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
                if close_decision.matched() || states.contains_key(&identity) {
                    let (_state, close_confirmed) =
                        store_update(states, identity.clone(), &close_decision, ticks);
                    if close_confirmed {
                        log.send_close(child);
                    }
                } else if close_decision.signals.has_relevant_signal() {
                    store_update(states, identity.clone(), &close_decision, ticks);
                }
            }

            if let Some((x, y, width, height)) =
                planned_view_resize(rules, &window_text, parent_rect, current_rect)
            {
                log.set_pos(child, x, y, width, height);
                log.resized += 1;
            }

            if !settings.aggressive_mode || child_rect.is_none() {
                continue;
            }
            if aggressive_decision.matched() || states.contains_key(&identity) {
                let (_state, aggressive_confirmed) =
                    store_update(states, identity.clone(), &aggressive_decision, ticks);
                if aggressive_confirmed
                    && aggressive_decision.action == crate::model::ActionKind::Hide
                {
                    log.hide_window(child);
                    log.hidden += 1;
                }
            }
        }
    }

    for wnd in candidates {
        let Some(node) = graph.get(*wnd) else {
            continue;
        };
        if !kakao_pids.contains(&node.pid) {
            continue;
        }
        let identity = node.identity();
        let legacy_kind = legacy_signature_kind(graph, rules, *wnd);
        let legacy_decision = legacy_hide_decision(&legacy_kind);
        if legacy_decision.matched() || states.contains_key(&identity) {
            let (_state, legacy_confirmed) =
                store_update(states, identity, &legacy_decision, ticks);
            if legacy_confirmed && legacy_decision.action == crate::model::ActionKind::Hide {
                log.hide_window(*wnd);
                log.hidden += 1;
            }
        }
    }

    let mut handled = HashSet::new();
    for item in collect_top_level(graph) {
        if item.win32_parent() != 0 {
            continue;
        }
        if confirmed_set.contains(&item.hwnd) {
            continue;
        }
        let popup_guard = popup_host_guard_status(rules, item.text(), item.text_known());
        // Hidden hosts/descendants must still match so a refused WM_CLOSE
        // fallback is not treated as a vanished ad signal.
        for (child, depth, _class_name) in find_popup_matches(graph, rules, item.hwnd, false) {
            let popup_decision = popup_dismiss_decision(popup_guard, depth);
            if let Some(child_node) = graph.get(child) {
                store_update(states, child_node.identity(), &popup_decision, ticks);
            }
            if popup_guard != PopupGuard::Allow {
                continue;
            }
            if !handled.contains(&item.hwnd) {
                retain_popup(&mut log, item.hwnd, item.visible);
                handled.insert(item.hwnd);
            }
            if !handled.contains(&child) {
                let child_visible = graph.get(child).map(|node| node.visible).unwrap_or(false);
                retain_popup(&mut log, child, child_visible);
                handled.insert(child);
            }
        }
    }

    log
}

fn retain_popup(log: &mut MutationLog, hwnd: Hwnd, visible: bool) {
    if visible {
        dismiss_popup(log, hwnd);
    } else {
        keep_hidden_popup(log, hwnd);
    }
}

/// A currently visible popup ad: request `WM_CLOSE`, then plan the hide and
/// zero-size fallbacks that keep it suppressed if the window refuses to close.
/// `retain_popup` only routes visible popups here, so both fallbacks are always
/// planned — the engine logs whether `WM_CLOSE` actually destroyed the window.
fn dismiss_popup(log: &mut MutationLog, hwnd: Hwnd) {
    log.send_popup_close(hwnd);
    log.hide_window(hwnd);
    log.set_pos(hwnd, 0, 0, 0, 0);
    log.popup_hide_fallbacks += 1;
    log.popup_zero_size_fallbacks += 1;
    log.hidden += 1;
}

fn keep_hidden_popup(log: &mut MutationLog, hwnd: Hwnd) {
    log.hide_window(hwnd);
    log.set_pos(hwnd, 0, 0, 0, 0);
}

pub fn evaluate_graph(
    graph: &WindowGraph,
    settings: &LayoutSettings,
    rules: &LayoutRules,
) -> Evaluation {
    let mut states = HashMap::new();
    evaluate_graph_with_states(graph, settings, rules, &mut states)
}

/// Full evaluation including the `candidates` diagnostic payload. Used by
/// `--dump-tree`, `--dump-tree-series` and `--shadow`.
pub fn evaluate_graph_with_states(
    graph: &WindowGraph,
    settings: &LayoutSettings,
    rules: &LayoutRules,
    states: &mut HashMap<WindowIdentity, CandidateState>,
) -> Evaluation {
    evaluate_inner(graph, settings, rules, states, true)
}

/// Apply-only evaluation for the engine worker's hot loop.
///
/// Building the `candidates` payload costs a second full recursive traversal
/// (`subtree_contains_ad_token`, `class_name_starts_with`, `find_popup_matches`,
/// `legacy_signature_kind`) plus a clone of the whole candidate-state map, and
/// the worker discards the result. `actions` and `state` are byte-identical to
/// `evaluate_graph_with_states`; only `candidates` is left empty.
pub fn evaluate_graph_for_apply(
    graph: &WindowGraph,
    settings: &LayoutSettings,
    rules: &LayoutRules,
    states: &mut HashMap<WindowIdentity, CandidateState>,
) -> Evaluation {
    evaluate_inner(graph, settings, rules, states, false)
}

fn evaluate_inner(
    graph: &WindowGraph,
    settings: &LayoutSettings,
    rules: &LayoutRules,
    states: &mut HashMap<WindowIdentity, CandidateState>,
    with_candidates: bool,
) -> Evaluation {
    let main_windows = inspect_main_windows(graph, rules);
    let confirmed: Vec<Hwnd> = main_windows
        .iter()
        .filter(|item| item.confirmed)
        .map(|item| item.hwnd)
        .collect();
    let confirmed_set: HashSet<Hwnd> = confirmed.iter().copied().collect();
    let candidates = candidate_handles(graph, rules, &confirmed_set);
    let candidate_payloads = if with_candidates {
        let mut preview_states = states.clone();
        inspect_candidates(
            graph,
            settings,
            rules,
            &confirmed,
            &confirmed_set,
            &candidates,
            &mut preview_states,
        )
    } else {
        Vec::new()
    };
    let log = apply_once(
        graph,
        settings,
        rules,
        &confirmed,
        &confirmed_set,
        &candidates,
        states,
    );
    let candidate_main_window_count = main_windows.len() as i64;
    Evaluation {
        main_windows,
        candidates: candidate_payloads,
        actions: ActionLog {
            hide: MutationLog::unique_sorted(log.hide),
            show: MutationLog::unique_sorted(log.show),
            close: MutationLog::unique_sorted(log.close),
            set_pos: log.set_pos,
        },
        state: EngineStatePayload {
            main_window_count: confirmed.len() as i64,
            candidate_main_window_count,
            hidden_windows: log.hidden,
            closed_windows: log.closed,
            resized_windows: log.resized,
            popup_close_requests: log.popup_close_requests,
            popup_hide_fallbacks: log.popup_hide_fallbacks,
            popup_zero_size_fallbacks: log.popup_zero_size_fallbacks,
        },
    }
}

pub fn evaluate_dump(
    dump_json: &str,
    settings: &LayoutSettings,
    rules: &LayoutRules,
) -> Result<Evaluation, serde_json::Error> {
    let graph = WindowGraph::from_dump_json(dump_json)?;
    Ok(evaluate_graph(&graph, settings, rules))
}
