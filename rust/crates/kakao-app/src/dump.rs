use std::collections::HashMap;

use chrono_lite::now_rfc3339;
use kakao_core::{
    CandidateState, Evaluation, LayoutRules, LayoutSettings, WindowGraph, WindowIdentity,
    WindowText, evaluate_graph_with_states,
};
use serde_json::{Value, json};

use crate::graph_build::build_graph;
use kakao_win32::Win32Api;

mod chrono_lite {
    pub fn now_rfc3339() -> String {
        // Stable enough for dump files; Python uses local isoformat.
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("{secs}")
    }
}

pub fn dump_payload(
    api: &dyn Win32Api,
    pids: &[i64],
    settings: &LayoutSettings,
    rules: &LayoutRules,
) -> Value {
    let mut states = HashMap::new();
    dump_payload_with_states(api, pids, settings, rules, &mut states)
}

pub fn dump_payload_with_states(
    api: &dyn Win32Api,
    pids: &[i64],
    settings: &LayoutSettings,
    rules: &LayoutRules,
    states: &mut HashMap<WindowIdentity, CandidateState>,
) -> Value {
    let graph = build_graph(api, pids);
    let evaluation = evaluate_graph_with_states(&graph, settings, rules, states);
    graph_to_dump(&graph, &evaluation)
}

pub fn graph_to_dump(graph: &WindowGraph, evaluation: &Evaluation) -> Value {
    let mut windows = Vec::new();
    let mut owned_popups = Vec::new();
    for hwnd in graph.enum_windows() {
        let Some(node) = graph.get(hwnd) else {
            continue;
        };
        let mut dumped = dump_node(graph, hwnd, 0, 6);
        if node.owner.unwrap_or(0) != 0 {
            dumped["owner"] = json!(node.owner.unwrap());
            owned_popups.push(dumped);
        } else {
            windows.push(dumped);
        }
    }
    json!({
        "timestamp": now_rfc3339(),
        "pids": graph.pids,
        "main_windows": evaluation.main_windows,
        "windows": windows,
        "owned_popups": owned_popups,
        "candidates": evaluation.candidates,
    })
}

fn dump_node(graph: &WindowGraph, hwnd: i64, depth: i32, max_depth: i32) -> Value {
    let Some(node) = graph.get(hwnd) else {
        return json!({});
    };
    let (text, text_known) = match &node.title {
        WindowText::Known(text) => (text.clone(), true),
        WindowText::Truncated(text) => (text.clone(), true),
        WindowText::Unknown { .. } => (String::new(), false),
    };
    let mut children = Vec::new();
    if depth < max_depth {
        for child in graph.enum_children(hwnd) {
            children.push(dump_node(graph, child, depth + 1, max_depth));
        }
    }
    json!({
        "hwnd": node.hwnd,
        "class": node.class_name,
        "text": text,
        "text_known": text_known,
        "pid": node.pid,
        "visible": node.visible,
        "rect": node.rect.map(|r| [r.left, r.top, r.right, r.bottom]),
        "depth": depth,
        "children": children,
    })
}

pub fn write_json(path: &std::path::Path, value: &Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(value)?)
}
