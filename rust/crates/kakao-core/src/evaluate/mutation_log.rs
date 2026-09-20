use std::collections::HashMap;

use crate::graph::WindowGraph;
use crate::model::Hwnd;

pub(super) struct MutationLog {
    pub(super) hide: Vec<Hwnd>,
    pub(super) show: Vec<Hwnd>,
    pub(super) close: Vec<Hwnd>,
    pub(super) set_pos: Vec<Vec<i64>>,
    pub(super) hidden: i64,
    pub(super) closed: i64,
    pub(super) resized: i64,
    pub(super) popup_close_requests: i64,
    pub(super) popup_hide_fallbacks: i64,
    pub(super) popup_zero_size_fallbacks: i64,
    visible: HashMap<Hwnd, bool>,
}

impl MutationLog {
    pub(super) fn new(graph: &WindowGraph) -> Self {
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

    pub(super) fn hide_window(&mut self, hwnd: Hwnd) {
        self.hide.push(hwnd);
        self.visible.insert(hwnd, false);
    }

    pub(super) fn set_pos(&mut self, hwnd: Hwnd, x: i32, y: i32, width: i32, height: i32) {
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
    pub(super) fn send_close(&mut self, hwnd: Hwnd) {
        self.close.push(hwnd);
        self.closed += 1;
    }

    pub(super) fn send_popup_close(&mut self, hwnd: Hwnd) {
        self.close.push(hwnd);
        self.popup_close_requests += 1;
    }

    pub(super) fn unique_sorted(mut values: Vec<Hwnd>) -> Vec<Hwnd> {
        values.sort_unstable();
        values.dedup();
        values
    }
}
