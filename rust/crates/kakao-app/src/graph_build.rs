use std::collections::{HashMap, HashSet};

use kakao_core::{Hwnd, WindowGraph, WindowNode};
use kakao_win32::Win32Api;

pub fn build_graph(api: &dyn Win32Api, pids: &[i64]) -> WindowGraph {
    if pids.is_empty() {
        return WindowGraph::empty(Vec::new());
    }
    let pid_set: HashSet<i64> = pids.iter().copied().collect();
    let mut top_level = Vec::new();
    api.enum_windows(&mut |hwnd| {
        let pid = api.get_window_thread_process_id(hwnd);
        if pid_set.contains(&pid) {
            top_level.push((hwnd, pid));
        }
        true
    });
    top_level.sort_unstable_by_key(|&(hwnd, _)| hwnd);
    let mut graph = WindowGraph::empty(pids.to_vec());
    let mut visited = HashSet::new();
    for (hwnd, pid) in top_level {
        let children_of = child_map(api, hwnd);
        load_tree(api, &mut graph, hwnd, None, pid, &mut visited, &children_of);
    }
    graph
}

/// Direct children of every window under `top`, from a single enumeration.
///
/// `EnumChildWindows` returns all descendants. Enumerating again at every
/// node and filtering by `GetParent` made graph building O(nodes × depth) with
/// two `GetParent` calls per visited descendant. One pass with one `GetParent`
/// per descendant yields the same edges: Win32 enumerates depth-first in
/// z-order, so each parent's children keep their relative order.
fn child_map(api: &dyn Win32Api, top: Hwnd) -> HashMap<Hwnd, Vec<Hwnd>> {
    let mut children_of: HashMap<Hwnd, Vec<Hwnd>> = HashMap::new();
    api.enum_descendant_windows(top, &mut |child| {
        children_of
            .entry(api.get_parent(child))
            .or_default()
            .push(child);
        true
    });
    children_of
}

fn load_tree(
    api: &dyn Win32Api,
    graph: &mut WindowGraph,
    hwnd: Hwnd,
    structural_parent: Option<Hwnd>,
    pid: i64,
    visited: &mut HashSet<Hwnd>,
    children_of: &HashMap<Hwnd, Vec<Hwnd>>,
) {
    if !visited.insert(hwnd) || !api.is_window(hwnd) {
        return;
    }
    let class_name = api.get_class_name(hwnd);
    let title = api.get_window_text_result(hwnd);
    let win32_parent = api.get_parent(hwnd);
    let owner = if structural_parent.is_none() && win32_parent != 0 {
        Some(win32_parent)
    } else {
        None
    };
    graph.insert_node(WindowNode {
        hwnd,
        pid,
        class_name,
        title,
        structural_parent,
        owner,
        rect: api.get_window_rect(hwnd),
        visible: api.is_window_visible(hwnd),
    });
    // Keep only windows whose GetParent is this hwnd so graph edges match the
    // real child tree. Owned popups are top-level (not child windows); their
    // owner is recorded on the node above and is not a structural parent edge.
    let children = children_of.get(&hwnd).cloned().unwrap_or_default();
    for &child in &children {
        load_tree(api, graph, child, Some(hwnd), pid, visited, children_of);
    }
    graph.set_children(hwnd, children);
}
