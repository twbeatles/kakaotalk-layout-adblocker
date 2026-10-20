use std::collections::HashMap;

use kakao_core::{
    CandidateState, LayoutRules, LayoutSettings, Rect, WindowGraph, WindowIdentity, WindowNode,
    WindowText, evaluate_graph_with_states,
};

#[test]
fn weak_substring_legacy_signal_requires_two_ticks_to_confirm() {
    let mut graph = WindowGraph::empty(vec![1000]);

    // Main window (top-level)
    let main_node = WindowNode {
        hwnd: 100,
        pid: 1000,
        class_name: "EVA_Window_Dblclk".into(),
        title: WindowText::Known("카카오톡".into()),
        rect: Some(Rect {
            left: 100,
            top: 100,
            right: 500,
            bottom: 700,
        }),
        structural_parent: None,
        owner: None,
        visible: true,
    };
    graph.insert_node(main_node);

    // OnlineMainView child required for main window confirmation
    let view_node = WindowNode {
        hwnd: 150,
        pid: 1000,
        class_name: "EVA_ChildWindow".into(),
        title: WindowText::Known("OnlineMainView".into()),
        rect: Some(Rect {
            left: 100,
            top: 130,
            right: 500,
            bottom: 600,
        }),
        structural_parent: Some(100),
        owner: None,
        visible: true,
    };
    graph.insert_node(view_node);
    graph.set_children(100, vec![150]);

    // Legacy ad candidate top-level window with substring signature
    // "Prefix Chrome Legacy Window Suffix" matches chrome_legacy_title_contains as substring (Weak)
    let ad_node = WindowNode {
        hwnd: 200,
        pid: 1000,
        class_name: "EVA_Window".into(),
        title: WindowText::Known("Prefix Chrome Legacy Window Suffix".into()),
        rect: Some(Rect {
            left: 100,
            top: 600,
            right: 500,
            bottom: 700,
        }),
        structural_parent: None,
        owner: None,
        visible: true,
    };
    graph.insert_node(ad_node);

    let settings = LayoutSettings {
        enabled: true,
        aggressive_mode: false,
    };
    let rules = LayoutRules {
        weak_signal_confirm_ticks: 2,
        ..LayoutRules::default()
    };

    let mut states: HashMap<WindowIdentity, CandidateState> = HashMap::new();

    // Tick 1: Weak signal should NOT be confirmed yet
    let eval1 = evaluate_graph_with_states(&graph, &settings, &rules, &mut states);
    assert!(
        !eval1.actions.hide.contains(&200),
        "Tick 1: weak signal should not trigger hide immediately"
    );
    let identity = WindowIdentity {
        hwnd: 200,
        pid: 1000,
        class_name: "EVA_Window".into(),
    };
    let state1 = states.get(&identity).expect("state should be recorded");
    assert_eq!(state1.match_streak, 1);

    // Tick 2: Second consecutive match should confirm weak signal and trigger hide
    let eval2 = evaluate_graph_with_states(&graph, &settings, &rules, &mut states);
    assert!(
        eval2.actions.hide.contains(&200),
        "Tick 2: weak signal should be confirmed and trigger hide"
    );
    let state2 = states.get(&identity).expect("state should be recorded");
    assert_eq!(state2.match_streak, 2);

    // Tick 3: If text changes to non-ad, hide action is removed
    if let Some(node) = graph.nodes.get_mut(&200) {
        node.title = WindowText::Known("친구 목록".into());
    }
    let eval3 = evaluate_graph_with_states(&graph, &settings, &rules, &mut states);
    assert!(
        !eval3.actions.hide.contains(&200),
        "Tick 3: non-ad text should not trigger hide"
    );
}
