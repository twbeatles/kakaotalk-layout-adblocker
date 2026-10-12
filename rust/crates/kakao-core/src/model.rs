use serde::{Deserialize, Serialize};

pub type Hwnd = i64;
pub type Pid = i64;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WindowIdentity {
    pub hwnd: Hwnd,
    pub pid: Pid,
    pub class_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn from_xyxy(values: [i32; 4]) -> Self {
        Self {
            left: values[0],
            top: values[1],
            right: values[2],
            bottom: values[3],
        }
    }

    /// Saturating: hostile or corrupt coordinates must never panic the evaluator.
    pub fn width(self) -> i32 {
        self.right.saturating_sub(self.left)
    }

    pub fn height(self) -> i32 {
        self.bottom.saturating_sub(self.top)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowText {
    Known(String),
    Unknown { error_code: u32 },
    Truncated(String),
}

impl WindowText {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Known(text) | Self::Truncated(text) => text,
            Self::Unknown { .. } => "",
        }
    }

    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowNode {
    pub hwnd: Hwnd,
    pub pid: Pid,
    pub class_name: String,
    pub title: WindowText,
    pub structural_parent: Option<Hwnd>,
    pub owner: Option<Hwnd>,
    pub rect: Option<Rect>,
    pub visible: bool,
}

impl WindowNode {
    pub fn identity(&self) -> WindowIdentity {
        WindowIdentity {
            hwnd: self.hwnd,
            pid: self.pid,
            class_name: self.class_name.clone(),
        }
    }

    pub fn text(&self) -> &str {
        self.title.as_str()
    }

    pub fn text_known(&self) -> bool {
        self.title.is_known()
    }

    pub fn win32_parent(&self) -> Hwnd {
        self.owner.or(self.structural_parent).unwrap_or(0)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AdSignals {
    pub legacy_signature: String,
    pub popup_direct_class: bool,
    pub popup_descendant_class: bool,
    pub popup_match_depth: i64,
    pub chrome_widget_bottom_banner: bool,
    pub subtree_ad_token: bool,
    pub empty_eva_child: bool,
    pub popup_host_guard: String,
}

impl AdSignals {
    pub fn blank() -> Self {
        Self {
            popup_host_guard: "n/a".to_string(),
            ..Self::default()
        }
    }

    pub fn has_relevant_signal(&self) -> bool {
        !self.legacy_signature.is_empty()
            || self.popup_direct_class
            || self.popup_descendant_class
            || self.chrome_widget_bottom_banner
            || self.subtree_ad_token
            || self.empty_eva_child
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionStrength {
    None,
    Weak,
    Strong,
}

impl DecisionStrength {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Weak => "weak",
            Self::Strong => "strong",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionKind {
    None,
    Hide,
    Close,
    DismissPopup,
}

impl ActionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Hide => "hide",
            Self::Close => "close",
            Self::DismissPopup => "dismiss_popup",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdDecision {
    pub signals: AdSignals,
    pub decision: DecisionStrength,
    pub action: ActionKind,
}

impl AdDecision {
    pub fn none(signals: AdSignals) -> Self {
        Self {
            signals,
            decision: DecisionStrength::None,
            action: ActionKind::None,
        }
    }

    pub fn hide(decision: DecisionStrength, signals: AdSignals) -> Self {
        Self {
            signals,
            decision,
            action: ActionKind::Hide,
        }
    }

    pub fn close(decision: DecisionStrength, signals: AdSignals) -> Self {
        Self {
            signals,
            decision,
            action: ActionKind::Close,
        }
    }

    pub fn dismiss_popup(signals: AdSignals) -> Self {
        Self {
            signals,
            decision: DecisionStrength::Strong,
            action: ActionKind::DismissPopup,
        }
    }

    pub fn matched(&self) -> bool {
        self.decision != DecisionStrength::None && self.action != ActionKind::None
    }
}

#[derive(Debug, Clone, Default)]
pub struct CandidateState {
    pub match_streak: i64,
    pub miss_streak: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupGuard {
    Na,
    Allow,
    Blocked,
}

impl PopupGuard {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Na => "n/a",
            Self::Allow => "allow",
            Self::Blocked => "blocked",
        }
    }
}
