//! Read-only timing probe for the reconciliation hot path against the live
//! desktop. Ignored by default (machine-dependent, needs KakaoTalk running):
//! `cargo test --release -p kakao-app --test perf_probe -- --ignored --nocapture`
//!
//! It only enumerates and reads windows; nothing is hidden, moved or closed.

#[cfg(windows)]
#[test]
#[ignore]
fn reconciliation_cost_breakdown() {
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::time::Instant;

    use kakao_app::graph_build::build_graph;
    use kakao_core::{evaluate_graph_for_apply, LayoutRules, LayoutSettings};
    use kakao_win32::{RealWin32, Win32Api};

    let api = RealWin32::new();
    let pids: Vec<i64> = kakao_win32::process::kakaotalk_pids().into_iter().collect();
    if pids.is_empty() {
        eprintln!("KakaoTalk is not running; nothing to measure");
        return;
    }
    let rounds = 300u32;
    let per_round = |started: Instant| started.elapsed().as_secs_f64() * 1e6 / f64::from(rounds);

    let pid_set: HashSet<i64> = pids.iter().copied().collect();
    let started = Instant::now();
    let mut top_level = 0usize;
    let mut desktop = 0usize;
    for _ in 0..rounds {
        top_level = 0;
        desktop = 0;
        api.enum_windows(&mut |hwnd| {
            desktop += 1;
            if pid_set.contains(&api.get_window_thread_process_id(hwnd)) {
                top_level += 1;
            }
            true
        });
    }
    let enum_us = per_round(started);

    let started = Instant::now();
    let mut nodes = 0usize;
    for _ in 0..rounds {
        nodes = build_graph(&api, &pids).nodes.len();
    }
    let graph_us = per_round(started);

    let graph = build_graph(&api, &pids);
    let rules = LayoutRules::default();
    let settings = LayoutSettings {
        enabled: true,
        aggressive_mode: true,
    };
    let mut states = HashMap::new();
    let started = Instant::now();
    for _ in 0..rounds {
        let _ = evaluate_graph_for_apply(&graph, &settings, &rules, &mut states);
    }
    let eval_us = per_round(started);

    let started = Instant::now();
    for _ in 0..rounds {
        let _ = kakao_win32::process::kakaotalk_pids();
    }
    let scan_us = per_round(started);

    eprintln!(
        "desktop_top_level={desktop} kakao_top_level={top_level} kakao_nodes={nodes}\n\
         enum_windows_pass_us={enum_us:.1} build_graph_us={graph_us:.1} \
         (enum share {:.0}%) evaluate_us={eval_us:.1} toolhelp_scan_us={scan_us:.1}",
        100.0 * enum_us / graph_us
    );
}
