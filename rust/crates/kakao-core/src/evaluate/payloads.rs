use serde::{Deserialize, Serialize};

use crate::model::{AdSignals, Hwnd};
use crate::rules::LayoutSettings;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdSignalsPayload {
    pub legacy_signature: String,
    pub popup_direct_class: bool,
    pub popup_descendant_class: bool,
    pub popup_match_depth: i64,
    pub chrome_widget_bottom_banner: bool,
    pub subtree_ad_token: bool,
    pub empty_eva_child: bool,
    pub popup_host_guard: String,
}

impl From<&AdSignals> for AdSignalsPayload {
    fn from(signals: &AdSignals) -> Self {
        Self {
            legacy_signature: signals.legacy_signature.clone(),
            popup_direct_class: signals.popup_direct_class,
            popup_descendant_class: signals.popup_descendant_class,
            popup_match_depth: signals.popup_match_depth,
            chrome_widget_bottom_banner: signals.chrome_widget_bottom_banner,
            subtree_ad_token: signals.subtree_ad_token,
            empty_eva_child: signals.empty_eva_child,
            popup_host_guard: signals.popup_host_guard.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MainWindowPayload {
    pub hwnd: Hwnd,
    pub pid: i64,
    pub class: String,
    pub text: String,
    pub structural_candidate: bool,
    pub title_match: bool,
    pub child_signature: bool,
    pub confirmed: bool,
    pub confirmation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidatePayload {
    pub hwnd: Hwnd,
    pub pid: i64,
    pub class: String,
    pub signals: AdSignalsPayload,
    pub decision: String,
    pub action: String,
    pub match_streak: i64,
    pub miss_streak: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ActionLog {
    pub hide: Vec<Hwnd>,
    pub show: Vec<Hwnd>,
    pub close: Vec<Hwnd>,
    pub set_pos: Vec<Vec<i64>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EngineStatePayload {
    pub main_window_count: i64,
    pub candidate_main_window_count: i64,
    pub hidden_windows: i64,
    pub closed_windows: i64,
    pub resized_windows: i64,
    pub popup_close_requests: i64,
    pub popup_hide_fallbacks: i64,
    pub popup_zero_size_fallbacks: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Evaluation {
    pub main_windows: Vec<MainWindowPayload>,
    pub candidates: Vec<CandidatePayload>,
    pub actions: ActionLog,
    pub state: EngineStatePayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoldenFile {
    pub fixture: String,
    pub settings: LayoutSettings,
    pub rules_overrides: serde_json::Value,
    pub expected: Evaluation,
}
