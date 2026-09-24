use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use kakao_core::{Rect, WindowText};
use serde::Deserialize;

use crate::api::{Win32Api, SW_HIDE, SW_SHOW, WM_CLOSE};

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
    windows: HashMap<i64, WindowRec>,
    children: HashMap<i64, Vec<i64>>,
    pids: Vec<i64>,
    last_error: u32,
    flatten_enum_children: bool,
    fail_set_pos: HashSet<i64>,
    fail_show: HashSet<i64>,
    restore_attempts: HashMap<i64, u64>,
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
            hung: HashSet::new(),
            ancestor_visibility: false,
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

    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        let mut inner = self.inner.lock().expect("fake lock");
        f(&mut inner)
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
        self.with(|inner| match inner.windows.get(&hwnd) {
            Some(rec) => WindowText::Known(rec.text.clone()),
            None => WindowText::Unknown { error_code: 0 },
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
            }
            if inner.fail_show.contains(&hwnd) {
                return false;
            }
            let Some(rec) = inner.windows.get_mut(&hwnd) else {
                return false;
            };
            if cmd == SW_HIDE {
                rec.visible = false;
            } else if cmd == SW_SHOW {
                rec.visible = true;
            }
            true
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
            if inner.fail_set_pos.contains(&hwnd) {
                return false;
            }
            let Some(rec) = inner.windows.get_mut(&hwnd) else {
                return false;
            };
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
            true
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
                return (false, 0);
            }
            if msg == WM_CLOSE {
                // Match FixtureAPI: the window stays alive so close confirmation fails.
                return (true, 1);
            }
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
