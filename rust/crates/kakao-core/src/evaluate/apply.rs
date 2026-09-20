use std::collections::{HashMap, HashSet};

use crate::graph::WindowGraph;
use crate::layout::planned_view_resize;
use crate::model::{AdDecision, AdSignals, CandidateState, Hwnd, PopupGuard, WindowIdentity};
use crate::rules::{LayoutRules, LayoutSettings};
use crate::signals::{
    aggressive_hide_decision, class_name_starts_with, empty_eva_close_decision, find_popup_matches,
    legacy_hide_decision, legacy_signature_kind, popup_dismiss_decision, popup_host_guard_status,
    subtree_contains_ad_token,
};

use super::inspect::{collect_top_level, store_update};
use super::mutation_log::MutationLog;

pub(super) fn apply_once(
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
