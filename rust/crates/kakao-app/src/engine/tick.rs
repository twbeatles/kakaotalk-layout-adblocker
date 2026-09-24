use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::time::Duration;

use kakao_core::{
    evaluate_graph_for_apply, evaluate_graph_with_states, Evaluation, LayoutRules, WindowGraph,
    WindowIdentity,
};
use kakao_win32::api::Win32Api;
use tracing::info;

use super::apply::apply_evaluation;
use super::caches::EngineCaches;
use super::flags::SharedFlags;
use super::restore::restore_stale_hidden;
use crate::config::AppSettings;
use crate::graph_build::build_graph;

pub fn tick(
    api: &dyn Win32Api,
    pids: &[i64],
    settings: &AppSettings,
    rules: &LayoutRules,
    caches: &mut EngineCaches,
    flags: &SharedFlags,
) -> Evaluation {
    let mut core = settings.to_core();
    core.enabled = flags.enabled.load(Ordering::SeqCst);
    core.aggressive_mode = flags.aggressive.load(Ordering::SeqCst);
    if pids.is_empty() && caches.snapshots.is_empty() {
        return Evaluation::default();
    }
    let graph = build_graph(api, pids);
    let apply_mode = flags.apply.load(Ordering::SeqCst) && core.enabled;
    // The diagnostic `candidates` payload costs a second full traversal and is
    // discarded on the apply path, so only build it when something reads it.
    let evaluation = if apply_mode {
        evaluate_graph_for_apply(&graph, &core, rules, &mut caches.states)
    } else {
        evaluate_graph_with_states(&graph, &core, rules, &mut caches.states)
    };
    flags.main_windows.store(
        u32::try_from(evaluation.state.main_window_count).unwrap_or(0),
        Ordering::SeqCst,
    );

    let cleanup_interval =
        Duration::from_millis(u64::from(settings.cache_cleanup_interval_ms.max(100)));
    if caches.cleanup_due(cleanup_interval) {
        prune_gone_identities(&graph, caches);
    }

    if apply_mode {
        let pid_set: HashSet<i64> = pids.iter().copied().collect();
        apply_evaluation(
            api,
            &graph,
            &evaluation,
            &mut caches.snapshots,
            &pid_set,
            flags,
        );
        if !caches.snapshots.is_empty() {
            let matched = matched_identities(&graph, &evaluation);
            let (failures, err) = restore_stale_hidden(api, caches, &matched);
            flags.report_restore_failures(failures, &err);
        }
    } else {
        for candidate in &evaluation.candidates {
            info!(
                hwnd = candidate.hwnd,
                pid = candidate.pid,
                class = %candidate.class,
                decision = %candidate.decision,
                action = %candidate.action,
                "shadow"
            );
        }
    }
    evaluation
}

fn prune_gone_identities(graph: &WindowGraph, caches: &mut EngineCaches) {
    let live: HashSet<WindowIdentity> = graph.nodes.values().map(|node| node.identity()).collect();
    let EngineCaches {
        snapshots,
        states,
        stale,
        ..
    } = caches;
    states.retain(|id, _| live.contains(id) || snapshots.contains_key(id));
    stale.retain(|id, _| live.contains(id) || snapshots.contains_key(id));
}

fn matched_identities(graph: &WindowGraph, evaluation: &Evaluation) -> HashSet<WindowIdentity> {
    let mut matched = HashSet::new();
    let mut push = |hwnd: i64| {
        if let Some(node) = graph.get(hwnd) {
            matched.insert(node.identity());
        }
    };
    for hwnd in &evaluation.actions.hide {
        push(*hwnd);
    }
    for hwnd in &evaluation.actions.close {
        push(*hwnd);
    }
    for pos in &evaluation.actions.set_pos {
        if pos.len() >= 5 && (pos[3] <= 0 || pos[4] <= 0) {
            push(pos[0]);
        }
    }
    matched
}
