use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use kakao_core::{Rect, WindowText};
use serde::Deserialize;

use crate::api::{SW_HIDE, SW_SHOW, WM_CLOSE, Win32Api};

/// What a per-window `WM_CLOSE` injection does. The default
/// (`DeliveredButAlive`) preserves the historical fake behavior — delivery
/// succeeds but the window survives, so the hide/zero-size fallback path is
/// exercised — while tests opt individual windows into the other outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloseBehavior {
    #[default]
    DeliveredButAlive,
    /// `WM_CLOSE` destroys the window: it disappears from the fake desktop.
    Destroy,
    /// Deterministic virtual timeout: delivery fails without sleeping.
    Timeout,
    /// The message is refused synchronously.
    DeliveryFailure,
}

/// One observed Win32 mutation attempt, in call order. Tests assert on this
/// log — not just the final window state — so a forbidden `WM_CLOSE` against
/// a healthy window fails even when the end state looks innocent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiCall {
    pub target: i64,
    pub method: &'static str,
    pub detail: String,
    pub succeeded: bool,
}

impl ApiCall {
    fn new(target: i64, method: &'static str, detail: String, succeeded: bool) -> Self {
        Self {
            target,
            method,
            detail,
            succeeded,
        }
    }

    /// `true` for calls that change or destroy windows.
    pub fn is_mutation(&self) -> bool {
        matches!(
            self.method,
            "show_window" | "set_window_pos" | "send_message_timeout"
        )
    }
}

/// Observable window state for before/after assertions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSnapshot {
    pub visible: bool,
    pub rect: Option<Rect>,
    pub pid: i64,
    pub class_name: String,
}

#[derive(Debug, Deserialize)]
struct DumpFile {
    pids: Vec<i64>,
    windows: Vec<DumpNode>,
}

#[derive(Debug, Deserialize)]
struct DumpNode {
    hwnd: i64,
    class: String,
    text: String,
    pid: i64,
    visible: bool,
    rect: Option<[i32; 4]>,
    #[serde(default)]
    owner: Option<i64>,
    #[serde(default)]
    children: Vec<DumpNode>,
}

#[derive(Debug, Clone)]
struct WindowRec {
    pid: i64,
    class_name: String,
    text: String,
    parent: i64,
    owner: i64,
    rect: Option<Rect>,
    visible: bool,
}

#[derive(Debug)]
struct Inner {
    close_behavior: HashMap<i64, CloseBehavior>,
    api_calls: Vec<ApiCall>,
    text_override: HashMap<i64, WindowText>,
    windows: HashMap<i64, WindowRec>,
    children: HashMap<i64, Vec<i64>>,
    pids: Vec<i64>,
    last_error: u32,
    flatten_enum_children: bool,
    fail_set_pos: HashSet<i64>,
    fail_show: HashSet<i64>,
    restore_attempts: HashMap<i64, u64>,
    restore_order: Vec<i64>,
    hung: HashSet<i64>,
    ancestor_visibility: bool,
    rect_queries: HashMap<i64, u64>,
}

pub struct FakeWin32 {
    inner: Mutex<Inner>,
    /// Lets tests exercise the worker's panic guard. Checked before the inner
    /// lock is taken so a deliberate panic cannot poison it.
    panic_on_enum_windows: AtomicBool,
}

impl FakeWin32 {
    pub fn from_dump_json(text: &str) -> Result<Self, serde_json::Error> {
        let dump: DumpFile = serde_json::from_str(text)?;
        let mut inner = Inner {
            windows: HashMap::new(),
            children: HashMap::new(),
            pids: dump.pids,
            last_error: 0,
            flatten_enum_children: false,
            fail_set_pos: HashSet::new(),
            fail_show: HashSet::new(),
            restore_attempts: HashMap::new(),
            restore_order: Vec::new(),
            hung: HashSet::new(),
            ancestor_visibility: false,
            close_behavior: HashMap::new(),
            api_calls: Vec::new(),
            text_override: HashMap::new(),
            rect_queries: HashMap::new(),
        };
        for node in dump.windows {
            load_node(&mut inner, node, 0);
        }
        Ok(Self {
            inner: Mutex::new(inner),
            panic_on_enum_windows: AtomicBool::new(false),
        })
    }

    pub fn pids(&self) -> Vec<i64> {
        self.inner.lock().expect("fake lock").pids.clone()
    }

    pub fn set_text(&self, hwnd: i64, text: &str) {
        self.with(|inner| {
            if let Some(rec) = inner.windows.get_mut(&hwnd) {
                rec.text = text.to_string();
            }
        });
    }

    pub fn set_pid(&self, hwnd: i64, pid: i64) {
        self.with(|inner| {
            if let Some(rec) = inner.windows.get_mut(&hwnd) {
                rec.pid = pid;
            }
        });
    }

    pub fn set_class_name(&self, hwnd: i64, class_name: &str) {
        self.with(|inner| {
            if let Some(rec) = inner.windows.get_mut(&hwnd) {
                rec.class_name = class_name.to_string();
            }
        });
    }

    pub fn set_flatten_enum_children(&self, flatten: bool) {
        self.with(|inner| inner.flatten_enum_children = flatten);
    }

    pub fn set_fail_set_window_pos(&self, hwnd: i64, fail: bool) {
        self.with(|inner| {
            if fail {
                inner.fail_set_pos.insert(hwnd);
            } else {
                inner.fail_set_pos.remove(&hwnd);
            }
        });
    }

    /// How many times a restore was attempted on this window (SW_SHOW or a
    /// non-zero SetWindowPos). Lets tests assert the retry backoff.
    pub fn restore_attempts(&self, hwnd: i64) -> u64 {
        self.with(|inner| inner.restore_attempts.get(&hwnd).copied().unwrap_or(0))
    }

    pub fn reset_restore_attempts(&self) {
        self.with(|inner| inner.restore_attempts.clear());
    }

    /// Hwnds in the order a restore was attempted on them (SW_SHOW). Lets
    /// tests assert restore ordering (e.g. shutdown restores healthy
    /// windows before already-failing ones).
    pub fn take_restore_order(&self) -> Vec<i64> {
        self.with(|inner| std::mem::take(&mut inner.restore_order))
    }

    pub fn set_fail_show_window(&self, hwnd: i64, fail: bool) {
        self.with(|inner| {
            if fail {
                inner.fail_show.insert(hwnd);
            } else {
                inner.fail_show.remove(&hwnd);
            }
        });
    }

    pub fn set_panic_on_enum_windows(&self, enabled: bool) {
        self.panic_on_enum_windows.store(enabled, Ordering::SeqCst);
    }

    /// Mark a window as belonging to a hung (not responding) thread.
    pub fn set_hung(&self, hwnd: i64, hung: bool) {
        self.with(|inner| {
            if hung {
                inner.hung.insert(hwnd);
            } else {
                inner.hung.remove(&hwnd);
            }
        });
    }

    /// Opt in to real `IsWindowVisible` semantics (a window counts as visible
    /// only when every ancestor is visible too). Off by default so existing
    /// fixtures keep their recorded per-node visibility.
    pub fn set_ancestor_visibility(&self, enabled: bool) {
        self.with(|inner| inner.ancestor_visibility = enabled);
    }

    /// Force a window's own visibility flag, bypassing `show_window`.
    pub fn set_visible(&self, hwnd: i64, visible: bool) {
        self.with(|inner| {
            if let Some(rec) = inner.windows.get_mut(&hwnd) {
                rec.visible = visible;
            }
        });
    }

    /// How many times `get_window_rect` was called for this window.
    pub fn rect_queries(&self, hwnd: i64) -> u64 {
        self.with(|inner| inner.rect_queries.get(&hwnd).copied().unwrap_or(0))
    }

    /// Override what `WM_CLOSE` does to one window. Windows without an
    /// override keep the historical `DeliveredButAlive` behavior, so existing
    /// tests are unaffected.
    pub fn set_close_behavior(&self, hwnd: i64, behavior: CloseBehavior) {
        self.with(|inner| {
            inner.close_behavior.insert(hwnd, behavior);
        });
    }

    pub fn clear_close_behavior(&self, hwnd: i64) {
        self.with(|inner| {
            inner.close_behavior.remove(&hwnd);
        });
    }

    /// Ordered Win32 mutation attempts since the last call. Tests assert on
    /// this log — not just the final window state — so a forbidden `WM_CLOSE`
    /// against a healthy window fails even when the end state looks innocent.
    pub fn take_api_calls(&self) -> Vec<ApiCall> {
        self.with(|inner| std::mem::take(&mut inner.api_calls))
    }

    /// What an observer currently sees for one window (`None` when gone).
    pub fn snapshot_window_state(&self, hwnd: i64) -> Option<WindowSnapshot> {
        self.with(|inner| {
            inner.windows.get(&hwnd).map(|rec| WindowSnapshot {
                visible: rec.visible,
                rect: rec.rect,
                pid: rec.pid,
                class_name: rec.class_name.clone(),
            })
        })
    }

    /// Inject a `get_window_text_result` outcome (`Known`, `Unknown`,
    /// `Truncated`) for one window without touching its stored text.
    pub fn set_window_text_result(&self, hwnd: i64, text: WindowText) {
        self.with(|inner| {
            inner.text_override.insert(hwnd, text);
        });
    }

    pub fn clear_window_text_result(&self, hwnd: i64) {
        self.with(|inner| {
            inner.text_override.remove(&hwnd);
        });
    }

    /// Simulate the window disappearing (a destroyed popup, a closed app).
    /// Removes the window and its whole subtree.
    pub fn remove_window(&self, hwnd: i64) {
        self.with(|inner| remove_subtree(inner, hwnd));
    }

    /// Test-only complement of `remove_window`: (re)create a window, e.g. a
    /// reappearing ad or an HWND reused by another process.
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_window(
        &self,
        hwnd: i64,
        pid: i64,
        class_name: &str,
        text: &str,
        parent: i64,
        rect: Option<Rect>,
        visible: bool,
    ) {
        self.with(|inner| {
            if parent != 0
                && let Some(siblings) = inner.children.get_mut(&parent)
                && !siblings.contains(&hwnd)
            {
                siblings.push(hwnd);
            }
            inner.children.entry(hwnd).or_default();
            inner.windows.insert(
                hwnd,
                WindowRec {
                    pid,
                    class_name: class_name.to_string(),
                    text: text.to_string(),
                    parent,
                    owner: 0,
                    rect,
                    visible,
                },
            );
            if !inner.pids.contains(&pid) {
                inner.pids.push(pid);
            }
        });
    }

    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        let mut inner = self.inner.lock().expect("fake lock");
        f(&mut inner)
    }
}

fn remove_subtree(inner: &mut Inner, hwnd: i64) {
    let mut queue = vec![hwnd];
    let mut index = 0;
    while index < queue.len() {
        let current = queue[index];
        index += 1;
        if let Some(children) = inner.children.remove(&current) {
            queue.extend(children);
        }
        inner.windows.remove(&current);
        inner.fail_set_pos.remove(&current);
        inner.fail_show.remove(&current);
        inner.hung.remove(&current);
        inner.close_behavior.remove(&current);
        inner.text_override.remove(&current);
        inner.restore_attempts.remove(&current);
    }
    for siblings in inner.children.values_mut() {
        siblings.retain(|sibling| *sibling != hwnd);
    }
}

fn collect_descendants(inner: &Inner, parent: i64) -> Vec<i64> {
    let mut out = Vec::new();
    let mut queue = inner.children.get(&parent).cloned().unwrap_or_default();
    let mut index = 0;
    while index < queue.len() {
        let hwnd = queue[index];
        index += 1;
        out.push(hwnd);
        if let Some(children) = inner.children.get(&hwnd) {
            queue.extend(children.iter().copied());
        }
    }
    out
}

fn load_node(inner: &mut Inner, node: DumpNode, parent: i64) {
    let hwnd = node.hwnd;
    let owner = node.owner.unwrap_or(0);
    let children: Vec<i64> = node.children.iter().map(|child| child.hwnd).collect();
    inner.windows.insert(
        hwnd,
        WindowRec {
            pid: node.pid,
            class_name: node.class,
            text: node.text,
            parent,
            owner,
            rect: node.rect.map(Rect::from_xyxy),
            visible: node.visible,
        },
    );
    inner.children.insert(hwnd, children);
    for child in node.children {
        load_node(inner, child, hwnd);
    }
}

impl Win32Api for FakeWin32 {
    fn enum_windows(&self, cb: &mut dyn FnMut(i64) -> bool) -> bool {
        assert!(
            !self.panic_on_enum_windows.load(Ordering::SeqCst),
            "FakeWin32: injected enum_windows panic"
        );
        let hwnds = self.with(|inner| {
            let mut hwnds: Vec<i64> = inner
                .windows
                .iter()
                .filter(|(_, rec)| rec.parent == 0)
                .map(|(hwnd, _)| *hwnd)
                .collect();
            hwnds.sort_unstable();
            hwnds
        });
        for hwnd in hwnds {
            if !cb(hwnd) {
                break;
            }
        }
        true
    }

    fn enum_child_windows(&self, parent: i64, cb: &mut dyn FnMut(i64) -> bool) -> bool {
        let children = self.with(|inner| {
            if inner.flatten_enum_children {
                collect_descendants(inner, parent)
            } else {
                inner.children.get(&parent).cloned().unwrap_or_default()
            }
        });
        for hwnd in children {
            if !cb(hwnd) {
                break;
            }
        }
        true
    }

    fn enum_descendant_windows(&self, parent: i64, cb: &mut dyn FnMut(i64) -> bool) -> bool {
        let descendants = self.with(|inner| collect_descendants(inner, parent));
        for hwnd in descendants {
            if !cb(hwnd) {
                break;
            }
        }
        true
    }

    fn get_window_thread_process_id(&self, hwnd: i64) -> i64 {
        self.with(|inner| inner.windows.get(&hwnd).map(|rec| rec.pid).unwrap_or(0))
    }

    fn get_class_name(&self, hwnd: i64) -> String {
        self.with(|inner| {
            inner
                .windows
                .get(&hwnd)
                .map(|rec| rec.class_name.clone())
                .unwrap_or_default()
        })
    }

    fn get_window_text_result(&self, hwnd: i64) -> WindowText {
        self.with(|inner| {
            let injected = inner.text_override.get(&hwnd).cloned();
            match (injected, inner.windows.get(&hwnd)) {
                (Some(text), _) => text,
                (None, Some(rec)) => WindowText::Known(rec.text.clone()),
                (None, None) => WindowText::Unknown { error_code: 0 },
            }
        })
    }

    fn get_parent(&self, hwnd: i64) -> i64 {
        self.with(|inner| {
            inner
                .windows
                .get(&hwnd)
                .map(|rec| {
                    if rec.owner != 0 {
                        rec.owner
                    } else {
                        rec.parent
                    }
                })
                .unwrap_or(0)
        })
    }

    fn get_window_rect(&self, hwnd: i64) -> Option<Rect> {
        self.with(|inner| {
            *inner.rect_queries.entry(hwnd).or_insert(0) += 1;
            inner.windows.get(&hwnd).and_then(|rec| rec.rect)
        })
    }

    fn get_client_rect(&self, hwnd: i64) -> Option<Rect> {
        self.get_window_rect(hwnd).map(|rect| Rect {
            left: 0,
            top: 0,
            right: rect.width(),
            bottom: rect.height(),
        })
    }

    fn is_window(&self, hwnd: i64) -> bool {
        self.with(|inner| inner.windows.contains_key(&hwnd))
    }

    fn is_window_visible(&self, hwnd: i64) -> bool {
        self.with(|inner| {
            let mut current = hwnd;
            loop {
                let Some(rec) = inner.windows.get(&current) else {
                    return false;
                };
                if !rec.visible {
                    return false;
                }
                if !inner.ancestor_visibility || rec.parent == 0 {
                    return true;
                }
                current = rec.parent;
            }
        })
    }

    fn has_visible_style(&self, hwnd: i64) -> bool {
        self.with(|inner| {
            inner
                .windows
                .get(&hwnd)
                .map(|rec| rec.visible)
                .unwrap_or(false)
        })
    }

    fn is_hung_app_window(&self, hwnd: i64) -> bool {
        self.with(|inner| inner.hung.contains(&hwnd))
    }

    fn show_window(&self, hwnd: i64, cmd: i32) -> bool {
        self.with(|inner| {
            if cmd == SW_SHOW {
                *inner.restore_attempts.entry(hwnd).or_insert(0) += 1;
                inner.restore_order.push(hwnd);
            }
            let succeeded = !inner.fail_show.contains(&hwnd) && inner.windows.contains_key(&hwnd);
            if succeeded && let Some(rec) = inner.windows.get_mut(&hwnd) {
                if cmd == SW_HIDE {
                    rec.visible = false;
                } else if cmd == SW_SHOW {
                    rec.visible = true;
                }
            }
            inner.api_calls.push(ApiCall::new(
                hwnd,
                "show_window",
                format!("cmd={cmd}"),
                succeeded,
            ));
            succeeded
        })
    }

    fn set_window_pos(
        &self,
        hwnd: i64,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> bool {
        self.with(|inner| {
            let succeeded =
                !inner.fail_set_pos.contains(&hwnd) && inner.windows.contains_key(&hwnd);
            if succeeded && let Some(rec) = inner.windows.get_mut(&hwnd) {
                let nomove = flags & crate::api::SWP_NOMOVE != 0;
                let nosize = flags & crate::api::SWP_NOSIZE != 0;
                if let Some(rect) = rec.rect.as_mut() {
                    if !nomove {
                        let w = rect.width();
                        let h = rect.height();
                        rect.left = x;
                        rect.top = y;
                        rect.right = x + w;
                        rect.bottom = y + h;
                    }
                    if !nosize {
                        rect.right = rect.left + width;
                        rect.bottom = rect.top + height;
                    }
                } else {
                    rec.rect = Some(Rect {
                        left: x,
                        top: y,
                        right: x + width,
                        bottom: y + height,
                    });
                }
            }
            inner.api_calls.push(ApiCall::new(
                hwnd,
                "set_window_pos",
                format!("x={x} y={y} w={width} h={height} flags={flags:#x}"),
                succeeded,
            ));
            succeeded
        })
    }

    fn send_message_timeout(
        &self,
        hwnd: i64,
        msg: u32,
        _wparam: usize,
        _lparam: isize,
        _timeout_ms: u32,
    ) -> (bool, isize) {
        self.with(|inner| {
            if !inner.windows.contains_key(&hwnd) {
                inner.api_calls.push(ApiCall::new(
                    hwnd,
                    "send_message_timeout",
                    format!("msg={msg:#x} outcome=missing-window"),
                    false,
                ));
                return (false, 0);
            }
            if msg == WM_CLOSE {
                let behavior = inner.close_behavior.get(&hwnd).copied().unwrap_or_default();
                let (delivered, result, outcome) = match behavior {
                    CloseBehavior::Destroy => {
                        remove_subtree(inner, hwnd);
                        (true, 1, "destroyed")
                    }
                    // Deterministic virtual timeout: no 500ms x N sleeping.
                    CloseBehavior::Timeout => (false, 0, "timeout"),
                    CloseBehavior::DeliveryFailure => (false, 0, "delivery-failure"),
                    // Match FixtureAPI: the window stays alive so close
                    // confirmation fails and the hide fallback applies.
                    CloseBehavior::DeliveredButAlive => (true, 1, "delivered-but-alive"),
                };
                inner.api_calls.push(ApiCall::new(
                    hwnd,
                    "send_message_timeout",
                    format!("msg=WM_CLOSE outcome={outcome}"),
                    delivered,
                ));
                return (delivered, result);
            }
            inner.api_calls.push(ApiCall::new(
                hwnd,
                "send_message_timeout",
                format!("msg={msg:#x} outcome=delivered"),
                true,
            ));
            (true, 1)
        })
    }

    fn update_window(&self, hwnd: i64) -> bool {
        self.is_window(hwnd)
    }

    fn get_last_error(&self) -> u32 {
        self.with(|inner| inner.last_error)
    }
}
