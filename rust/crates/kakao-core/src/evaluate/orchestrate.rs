use std::collections::{HashMap, HashSet};

use crate::graph::WindowGraph;
use crate::model::{CandidateState, Hwnd, WindowIdentity};
use crate::rules::{LayoutRules, LayoutSettings};

use super::apply::apply_once;
use super::inspect::{candidate_handles, inspect_candidates, inspect_main_windows};
use super::mutation_log::MutationLog;
use super::payloads::{ActionLog, EngineStatePayload, Evaluation};

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
