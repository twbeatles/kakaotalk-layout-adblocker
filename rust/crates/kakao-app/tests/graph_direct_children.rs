use kakao_app::graph_build::build_graph;
use kakao_core::LayoutRules;
use kakao_core::{LayoutSettings, evaluate_graph};
use kakao_win32::FakeWin32;

const NESTED_TREE: &str = r#"{
    "pids": [42],
    "windows": [{
        "hwnd": 1,
        "class": "EVA_Window",
        "text": "",
        "pid": 42,
        "visible": true,
        "rect": [0, 0, 100, 100],
        "children": [{
            "hwnd": 2,
            "class": "ChildA",
            "text": "",
            "pid": 42,
            "visible": true,
            "rect": [0, 0, 80, 80],
            "children": [{
                "hwnd": 3,
                "class": "ChildB",
                "text": "",
                "pid": 42,
                "visible": true,
                "rect": [0, 0, 60, 60],
                "children": [{
                    "hwnd": 4,
                    "class": "AdFitWebView",
                    "text": "",
                    "pid": 42,
                    "visible": true,
                    "rect": [0, 0, 40, 40],
                    "children": []
                }]
            }]
        }]
    }]
}"#;

#[test]
fn flattened_enum_child_windows_still_builds_direct_edges() {
    let api = FakeWin32::from_dump_json(NESTED_TREE).unwrap();
    api.set_flatten_enum_children(true);
    let graph = build_graph(&api, &[42]);

    assert_eq!(graph.enum_children(1), vec![2]);
    assert_eq!(graph.enum_children(2), vec![3]);
    assert_eq!(graph.enum_children(3), vec![4]);
    assert!(graph.enum_children(4).is_empty());

    let depth2 = graph.enum_descendants(1, 2);
    assert_eq!(depth2, vec![(2, 1), (3, 2)]);
    assert!(!depth2.iter().any(|(hwnd, _)| *hwnd == 4));

    let rules = LayoutRules::default();
    let settings = LayoutSettings {
        enabled: true,
        aggressive_mode: true,
    };
    let evaluation = evaluate_graph(&graph, &settings, &rules);
    assert!(
        !evaluation.actions.close.contains(&4),
        "AdFit at depth 3 must stay outside popup_search_depth=2"
    );
    assert!(
        !evaluation.actions.hide.contains(&4),
        "AdFit at depth 3 must not be hidden via popup dismiss"
    );
}
