//! Engine state-transition tests over an instrumented `FakeWin32`.
//!
//! Mapping to the hardening plan (§5.2): F01 sustained hide, F02 destroy,
//! F03 refused-close fallback, F04 timeout/delivery-failure, F05 aggressive
//! toggle, F06 blocker toggle, F07 benign-turn restores, F08 shutdown
//! healthy-first, F09 HWND reuse, F10 tick-time restore failure, F11
//! SetWindowPos failure, F12 vanished ad, F13 hook→polling (MANUAL_REQUIRED:
//! needs a real Win32 message loop), F14 tick panic, F15 worker exit
//! (MANUAL_REQUIRED: real thread timing), F16 hung shutdown (covered by
//! `restore_regression::hung_window_is_neither_hidden_nor_restored`), F17
//! guarded host, F18 empty EVA child.
//!
//! Every test asserts the final window state AND the ordered Win32 call log,
//! so a forbidden call against a healthy window fails the test even when the
//! end state looks innocent.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use kakao_app::config::AppSettings;
use kakao_app::engine::{tick, EngineCaches, SharedFlags};
use kakao_core::{LayoutRules, WindowIdentity, WindowText};
use kakao_win32::{ApiCall, CloseBehavior, FakeWin32, Win32Api};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repo root")
}

fn load_dump(name: &str) -> FakeWin32 {
    let dump =
        fs::read_to_string(repo_root().join("tests/fixtures/window_dumps").join(name)).unwrap();
    FakeWin32::from_dump_json(&dump).unwrap()
}

fn enabled_settings() -> AppSettings {
    AppSettings {
        enabled: true,
        aggressive_mode: true,
        ..AppSettings::default()
    }
}

struct Harness {
    api: FakeWin32,
    pids: Vec<i64>,
    settings: AppSettings,
    flags: Arc<SharedFlags>,
    rules: LayoutRules,
    caches: EngineCaches,
}

fn boot(name: &str, settings: AppSettings, apply: bool) -> Harness {
    let api = load_dump(name);
    let pids = api.pids();
    let flags = SharedFlags::from_settings(&settings, apply);
    Harness {
        api,
        pids,
        settings,
        flags,
        rules: LayoutRules::default(),
        caches: EngineCaches::new(),
    }
}

fn owned_popup() -> Harness {
    boot("owned_popup_legacy_ad.json", enabled_settings(), true)
}

impl Harness {
    fn tick(&mut self) -> kakao_core::Evaluation {
        tick(
            &self.api,
            &self.pids,
            &self.settings,
            &self.rules,
            &mut self.caches,
            &self.flags,
        )
    }
}

fn mutations_to(calls: &[ApiCall], hwnd: i64) -> Vec<&ApiCall> {
    calls
        .iter()
        .filter(|call| call.target == hwnd && call.is_mutation())
        .collect()
}

fn all_mutations(calls: &[ApiCall]) -> Vec<&ApiCall> {
    calls.iter().filter(|call| call.is_mutation()).collect()
}

const OWNED_AD_HOST: i64 = 527936;

#[test]
fn f01_sustained_hide_keeps_a_single_snapshot() {
    let mut h = owned_popup();
    for _ in 0..6 {
        h.tick();
        assert!(
            !h.api.is_window_visible(OWNED_AD_HOST),
            "sustained ad must stay hidden"
        );
    }
    assert_eq!(
        h.caches.snapshots.len(),
        1,
        "re-hiding every tick must not multiply snapshots"
    );
    assert!(
        h.caches
            .snapshots
            .values()
            .any(|snap| snap.identity.hwnd == OWNED_AD_HOST),
        "the first (pre-hide) snapshot must be the one kept"
    );
}

#[test]
fn f02_close_destroy_counts_closed_and_leaves_no_trace() {
    let mut h = boot("popup_adfit_webview.json", enabled_settings(), true);
    h.api.set_close_behavior(200, CloseBehavior::Destroy);
    h.tick();

    assert!(!h.api.is_window(200), "destroyed host must be gone");
    assert!(!h.api.is_window(201), "destroyed subtree must be gone");
    assert_eq!(
        h.flags.closed_windows.load(Ordering::SeqCst),
        1,
        "exactly the destroyed window is counted"
    );
    assert!(
        !h.caches
            .snapshots
            .values()
            .any(|snap| snap.identity.hwnd == 200 || snap.identity.hwnd == 201),
        "a destroyed window must never gain a restore snapshot"
    );

    // Tick 1 legitimately sends exactly one WM_CLOSE (the destroy itself).
    let first_calls = h.api.take_api_calls();
    let first_sends: Vec<&ApiCall> = first_calls
        .iter()
        .filter(|call| call.method == "send_message_timeout")
        .collect();
    assert_eq!(
        first_sends.len(),
        1,
        "only the destroying close may be sent: {first_sends:?}"
    );
    assert_eq!(first_sends[0].target, 200);
    assert!(first_sends[0].detail.contains("destroyed"));

    // Later ticks must not touch the gone windows at all.
    for _ in 0..3 {
        h.tick();
        let calls = h.api.take_api_calls();
        assert!(
            calls
                .iter()
                .all(|call| call.target != 200 && call.target != 201),
            "no Win32 call may target a destroyed window: {calls:?}"
        );
    }
}

#[test]
fn f03_close_refused_falls_back_to_hide_and_restores_on_off() {
    let mut h = boot("popup_adfit_webview.json", enabled_settings(), true);
    h.tick();
    assert!(!h.api.is_window_visible(200));
    assert!(!h.api.is_window_visible(201));

    h.flags.enabled.store(false, Ordering::SeqCst);
    let (failures, err) = h.caches.drain_restore_all(&h.api);
    assert_eq!((failures, err.as_str()), (0, ""));
    assert!(h.api.is_window_visible(200));
    assert!(h.api.is_window_visible(201));
}

#[test]
fn f04_close_timeout_and_delivery_failure_keep_ticking() {
    let mut h = boot("popup_adfit_webview.json", enabled_settings(), true);
    h.api.set_close_behavior(200, CloseBehavior::Timeout);
    h.api
        .set_close_behavior(201, CloseBehavior::DeliveryFailure);

    for _ in 0..3 {
        h.tick();
    }
    // No worker stall, no panic: the fallback still suppresses the popup.
    assert!(!h.api.is_window_visible(200));
    assert!(!h.api.is_window_visible(201));
    assert_eq!(
        h.flags.restore_failures.load(Ordering::SeqCst),
        0,
        "close-delivery trouble is not a restore failure"
    );

    let calls = h.api.take_api_calls();
    let outcomes: Vec<&str> = calls
        .iter()
        .filter(|call| call.method == "send_message_timeout")
        .map(|call| call.detail.as_str())
        .collect();
    assert!(
        outcomes.iter().any(|detail| detail.contains("timeout")),
        "timeout outcome must be observable: {outcomes:?}"
    );
    assert!(
        outcomes
            .iter()
            .any(|detail| detail.contains("delivery-failure")),
        "delivery-failure outcome must be observable: {outcomes:?}"
    );

    // …and nothing in this path sleeps: the virtual timeout is instant.
    // This checks outcomes/order only; native_close_latency tests a real delayed
    // WndProc. A virtual timeout cannot establish production latency.
}

#[test]
fn f05_aggressive_toggle_restores_then_rehides() {
    let mut h = boot("launch_ad_flash_banner.json", enabled_settings(), true);
    h.tick();
    assert!(!h.api.is_window_visible(102), "banner hidden while ON");

    h.flags.aggressive.store(false, Ordering::SeqCst);
    h.tick();
    assert!(
        !h.api.is_window_visible(102),
        "first miss tick is grace, still hidden"
    );
    h.tick();
    assert!(
        h.api.is_window_visible(102),
        "aggressive OFF must restore after the grace period"
    );
    assert!(
        !h.caches
            .snapshots
            .values()
            .any(|snap| snap.identity.hwnd == 102),
        "restored window leaves no snapshot behind"
    );

    h.flags.aggressive.store(true, Ordering::SeqCst);
    h.tick();
    assert!(
        !h.api.is_window_visible(102),
        "re-enabling re-hides a Strong banner on the next tick"
    );
}

#[test]
fn f06_blocker_off_makes_no_win32_mutations() {
    let mut h = owned_popup();
    h.tick();
    assert!(!h.api.is_window_visible(OWNED_AD_HOST));
    h.api.take_api_calls();

    h.flags.enabled.store(false, Ordering::SeqCst);
    h.tick();
    let calls = h.api.take_api_calls();
    assert!(
        all_mutations(&calls).is_empty(),
        "OFF tick must not mutate any window: {calls:?}"
    );
    assert_eq!(h.caches.snapshots.len(), 1, "OFF keeps existing snapshots");
    assert!(!h.api.is_window_visible(OWNED_AD_HOST));

    h.flags.enabled.store(true, Ordering::SeqCst);
    h.tick();
    assert!(!h.api.is_window_visible(OWNED_AD_HOST));
    assert_eq!(
        h.caches.snapshots.len(),
        1,
        "no duplicate snapshot on re-ON"
    );
}

#[test]
fn f07_banner_text_turns_benign_restores_after_grace() {
    let mut h = boot("launch_ad_flash_banner.json", enabled_settings(), true);
    h.tick();
    assert!(!h.api.is_window_visible(102));

    h.api.set_text(103, "친구 목록");
    h.tick();
    assert!(
        !h.api.is_window_visible(102),
        "first benign tick is grace, still hidden"
    );
    h.tick();
    assert!(
        h.api.is_window_visible(102),
        "banner restores once the ad text is gone for 2 ticks"
    );
    assert!(
        !h.caches
            .snapshots
            .values()
            .any(|snap| snap.identity.hwnd == 102),
        "restored banner leaves no snapshot behind"
    );
}

const TWO_AD_DUMP: &str = r#"{
    "pids": [42],
    "windows": [
        {
            "hwnd": 3000,
            "class": "EVA_Window_Dblclk",
            "text": "카카오톡",
            "pid": 42,
            "visible": true,
            "rect": [0, 0, 500, 700],
            "children": [
                {
                    "hwnd": 3001,
                    "class": "EVA_ChildWindow",
                    "text": "OnlineMainView_0x1",
                    "pid": 42,
                    "visible": true,
                    "rect": [0, 0, 500, 669],
                    "children": []
                }
            ]
        },
        {
            "hwnd": 3100,
            "class": "EVA_Window",
            "text": "",
            "pid": 42,
            "visible": true,
            "rect": [10, 10, 410, 410],
            "children": [
                {
                    "hwnd": 3101,
                    "class": "Chrome_WidgetWin_1",
                    "text": "Chrome Legacy Window",
                    "pid": 42,
                    "visible": true,
                    "rect": [10, 10, 410, 410],
                    "children": []
                }
            ]
        },
        {
            "hwnd": 3200,
            "class": "EVA_Window",
            "text": "",
            "pid": 42,
            "visible": true,
            "rect": [20, 20, 420, 420],
            "children": [
                {
                    "hwnd": 3201,
                    "class": "Chrome_WidgetWin_1",
                    "text": "Chrome Legacy Window",
                    "pid": 42,
                    "visible": true,
                    "rect": [20, 20, 420, 420],
                    "children": []
                }
            ]
        }
    ]
}"#;

fn two_ad_harness() -> Harness {
    let api = FakeWin32::from_dump_json(TWO_AD_DUMP).unwrap();
    let pids = api.pids();
    let settings = enabled_settings();
    let flags = SharedFlags::from_settings(&settings, true);
    Harness {
        api,
        pids,
        settings,
        flags,
        rules: LayoutRules::default(),
        caches: EngineCaches::new(),
    }
}

#[test]
fn f08_shutdown_restores_healthy_windows_before_failing_ones() {
    let mut h = two_ad_harness();
    h.tick();
    assert!(!h.api.is_window_visible(3100));
    assert!(!h.api.is_window_visible(3200));
    assert_eq!(h.caches.snapshots.len(), 2);

    // 3200's restore is already known-bad (simulates a stuck window tracked
    // in `stale`); 3100 is healthy.
    h.api.set_fail_show_window(3200, true);
    h.api.set_fail_set_window_pos(3200, true);
    h.caches.stale.insert(
        WindowIdentity {
            hwnd: 3200,
            pid: 42,
            class_name: "EVA_Window".to_string(),
        },
        kakao_app::engine::StaleState {
            attempts: 3,
            ..Default::default()
        },
    );

    h.api.take_restore_order();
    let (failures, _) = h.caches.drain_restore_all_final(&h.api);
    assert_eq!(failures, 1, "only the stuck window may fail");
    assert!(
        h.api.is_window_visible(3100),
        "healthy window must come back"
    );
    assert!(
        !h.api.is_window_visible(3200),
        "stuck window stays hidden with its snapshot kept"
    );
    assert!(
        h.caches
            .snapshots
            .values()
            .any(|snap| snap.identity.hwnd == 3200),
        "failed shutdown restore keeps the snapshot for a later retry"
    );
    let order = h.api.take_restore_order();
    let healthy_pos = order.iter().position(|hwnd| *hwnd == 3100);
    let stuck_pos = order.iter().position(|hwnd| *hwnd == 3200);
    assert!(
        healthy_pos < stuck_pos,
        "healthy-first shutdown order violated: {order:?}"
    );
}

#[test]
fn f09_recreated_window_with_new_identity_is_not_restored() {
    let mut h = owned_popup();
    h.tick();
    assert!(!h.api.is_window_visible(OWNED_AD_HOST));

    // The ad is destroyed; the OS hands the same HWND number to another app.
    h.api.remove_window(OWNED_AD_HOST);
    h.api.upsert_window(
        OWNED_AD_HOST,
        77777,
        "EVA_Window",
        "다른 앱",
        0,
        Some(kakao_core::Rect::from_xyxy([0, 0, 100, 100])),
        false,
    );
    h.api.take_api_calls();

    let (failures, _) = h.caches.drain_restore_all(&h.api);
    assert_eq!(failures, 0, "identity mismatch is skipped, not a failure");
    assert!(
        !h.api.is_window_visible(OWNED_AD_HOST),
        " another process's window must not be shown by our stale snapshot"
    );
    let calls = h.api.take_api_calls();
    assert!(
        !calls
            .iter()
            .any(|call| call.target == OWNED_AD_HOST && call.succeeded),
        "no successful Win32 call may target the reused HWND: {calls:?}"
    );
}

#[test]
fn f10_tick_time_restore_failure_reports_gauge_and_error_text() {
    let mut h = owned_popup();
    h.tick();
    h.api.set_text(OWNED_AD_HOST, "프로그래밍 토크방");
    h.api.set_fail_show_window(OWNED_AD_HOST, true);
    h.api.set_fail_set_window_pos(OWNED_AD_HOST, true);

    for _ in 0..6 {
        h.tick();
    }
    assert_eq!(
        h.flags.restore_failures.load(Ordering::SeqCst),
        1,
        "one stuck window = gauge 1"
    );
    assert!(
        !h.flags.last_error_text().is_empty(),
        "the tray status must surface the stuck window"
    );
    assert!(
        !h.api.is_window_visible(OWNED_AD_HOST),
        "failed restore stays hidden with its snapshot kept"
    );
    assert!(
        h.caches
            .snapshots
            .values()
            .any(|snap| snap.identity.hwnd == OWNED_AD_HOST),
        "failing restore keeps its snapshot"
    );
}

#[test]
fn f11_setpos_failure_leaves_main_view_layout_intact() {
    let mut h = boot("popup_adfit_webview.json", enabled_settings(), true);
    let main_before = h.api.get_window_rect(100).expect("main rect");
    h.api.set_fail_set_window_pos(200, true);
    h.api.set_fail_set_window_pos(201, true);

    let eval = h.tick();
    // The popup is still suppressed through the show path …
    assert!(!h.api.is_window_visible(200));
    assert!(!h.api.is_window_visible(201));
    // … while the refused zero-size fallback touches nothing else …
    assert_eq!(h.api.get_window_rect(100), Some(main_before));
    // … and the legitimate main-view resize still applies.
    assert!(
        eval.actions
            .set_pos
            .iter()
            .any(|pos| pos.first() == Some(&101)),
        "main-view resize must still be planned"
    );
    assert!(
        h.api
            .snapshot_window_state(200)
            .is_some_and(|snap| !snap.visible),
        "popup stays hidden via ShowWindow even when SetWindowPos fails"
    );
}

#[test]
fn f12_vanished_ad_gets_no_win32_calls_and_snapshot_clears() {
    let mut h = owned_popup();
    h.tick();
    assert!(!h.api.is_window_visible(OWNED_AD_HOST));
    h.api.take_api_calls();

    h.api.remove_window(OWNED_AD_HOST);
    for _ in 0..2 {
        h.tick();
        let calls = h.api.take_api_calls();
        assert!(
            calls.iter().all(|call| call.target != OWNED_AD_HOST),
            "no Win32 call may target a vanished window: {calls:?}"
        );
    }
    assert!(
        h.caches.snapshots.is_empty(),
        "vanished ad's snapshot clears after the miss grace period"
    );
}

#[test]
fn f13_reappearing_ad_is_redetected_without_snapshot_growth() {
    let mut h = owned_popup();
    h.tick();
    assert_eq!(h.caches.snapshots.len(), 1);

    // The ad popup is destroyed …
    h.api.remove_window(OWNED_AD_HOST);
    h.tick();
    // … and an identical-looking ad appears under the same identity.
    h.api.upsert_window(
        OWNED_AD_HOST,
        30948,
        "EVA_Window_Dblclk",
        "",
        0,
        Some(kakao_core::Rect::from_xyxy([1344, 912, 1913, 1025])),
        true,
    );
    h.api.upsert_window(
        5508836,
        34688,
        "Chrome_RenderWidgetHostHWND",
        "Chrome Legacy Window",
        OWNED_AD_HOST,
        Some(kakao_core::Rect::from_xyxy([1344, 912, 1913, 1025])),
        true,
    );
    h.tick();

    assert!(
        !h.api.is_window_visible(OWNED_AD_HOST),
        "reappeared ad must be hidden again"
    );
    assert_eq!(
        h.caches.snapshots.len(),
        1,
        "re-detection reuses the identity slot instead of stacking snapshots"
    );
}

#[test]
fn f14_enum_panic_does_not_poison_the_next_tick() {
    let mut h = owned_popup();
    h.tick();
    assert!(!h.api.is_window_visible(OWNED_AD_HOST));

    h.api.set_panic_on_enum_windows(true);
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| h.tick()));
    assert!(panicked.is_err(), "the injected panic must actually fire");
    h.api.set_panic_on_enum_windows(false);

    h.tick();
    assert!(
        !h.api.is_window_visible(OWNED_AD_HOST),
        "post-panic tick keeps suppressing the ad"
    );
    assert_eq!(h.caches.snapshots.len(), 1, "snapshot survives the panic");
    // The flag lock was never held across the panic: status stays readable.
    let _ = h.flags.last_error_text();
}

#[test]
fn f15_zero_mutation_gates_enabled_apply_stopping() {
    // enabled=false: nothing is evaluated, nothing is touched.
    let mut h = boot(
        "owned_popup_legacy_ad.json",
        AppSettings {
            enabled: false,
            aggressive_mode: true,
            ..AppSettings::default()
        },
        true,
    );
    h.tick();
    assert!(all_mutations(&h.api.take_api_calls()).is_empty());
    assert!(h.caches.snapshots.is_empty());
    assert!(h.api.is_window_visible(OWNED_AD_HOST));

    // apply=false (shadow): verdicts are computed, Win32 stays untouched.
    let mut h = boot("owned_popup_legacy_ad.json", enabled_settings(), false);
    let shadow = h.tick();
    assert!(shadow.actions.hide.contains(&OWNED_AD_HOST));
    assert!(all_mutations(&h.api.take_api_calls()).is_empty());
    assert!(h.caches.snapshots.is_empty());
    assert!(h.api.is_window_visible(OWNED_AD_HOST));

    // stopping=true: in-flight shutdown performs no new mutations.
    let mut h = owned_popup();
    h.tick();
    assert!(!h.api.is_window_visible(OWNED_AD_HOST));
    h.api.take_api_calls();
    h.flags.stopping.store(true, Ordering::SeqCst);
    h.tick();
    assert!(all_mutations(&h.api.take_api_calls()).is_empty());
}

#[test]
fn f16_shadow_verdict_matches_apply_verdict() {
    let mut shadowing = boot("owned_popup_legacy_ad.json", enabled_settings(), false);
    let mut applying = boot("owned_popup_legacy_ad.json", enabled_settings(), true);
    let shadow = shadowing.tick();
    let applied = applying.tick();
    assert_eq!(
        serde_json::to_value(&shadow.actions).unwrap(),
        serde_json::to_value(&applied.actions).unwrap(),
        "shadow and apply must reach the same verdict"
    );
    assert_eq!(
        serde_json::to_value(&shadow.state).unwrap(),
        serde_json::to_value(&applied.state).unwrap()
    );
    assert!(
        !applying.api.is_window_visible(OWNED_AD_HOST),
        "apply path really hid the ad"
    );
    assert!(
        shadowing.api.is_window_visible(OWNED_AD_HOST),
        "shadow path computes the same verdict without touching Win32"
    );
}

#[test]
fn f17_guarded_host_never_touched_with_call_log() {
    let mut h = boot("guarded_popup_adfit_viewer.json", enabled_settings(), true);
    for _ in 0..5 {
        let eval = h.tick();
        assert!(h.api.is_window_visible(200));
        assert!(h.api.is_window_visible(201));
        assert!(!eval.actions.hide.contains(&200));
        assert!(!eval.actions.hide.contains(&201));
        assert!(!eval.actions.close.contains(&200));
        assert!(!eval.actions.close.contains(&201));
        let calls = h.api.take_api_calls();
        assert!(
            mutations_to(&calls, 200).is_empty() && mutations_to(&calls, 201).is_empty(),
            "guarded viewer must never be a Win32 mutation target: {calls:?}"
        );
    }
    assert!(
        h.caches.snapshots.is_empty(),
        "untouched windows leave no snapshots"
    );
}

#[test]
fn f18_empty_eva_child_never_closed_across_late_text() {
    let mut h = boot(
        "empty_eva_child_no_ad_signal.json",
        enabled_settings(),
        true,
    );
    for _ in 0..3 {
        let eval = h.tick();
        assert!(!eval.actions.close.contains(&104));
        let calls = h.api.take_api_calls();
        assert!(
            !calls
                .iter()
                .any(|call| call.method == "send_message_timeout" && call.target == 104),
            "no WM_CLOSE may target the empty EVA child: {calls:?}"
        );
    }

    // Titles arrive late and change shape; still no ad signal, still no close.
    h.api.set_text(104, "OnlineMainView_0x9");
    h.tick();
    h.api.set_text(100, "");
    let eval = h.tick();
    assert!(!eval.actions.close.contains(&104));
    let calls = h.api.take_api_calls();
    assert!(
        !calls
            .iter()
            .any(|call| call.method == "send_message_timeout" && call.target == 104),
        "late text must not manufacture a close: {calls:?}"
    );
}

#[test]
fn f20_engine_never_mutates_foreign_pid_descendants() {
    // The core verdict may name renderer-owned descendants (the owned-popup
    // fixture nests pid-34688 CEF children under the pid-30948 ad), but the
    // engine's pid-gated precheck must keep Win32 away from them: only the
    // KakaoTalk-owned host itself is hidden and snapshotted.
    let mut h = owned_popup();
    let eval = h.tick();
    assert!(eval.actions.hide.contains(&OWNED_AD_HOST));

    let foreign: Vec<i64> = [2032986, 5508836, 4657842]
        .into_iter()
        .filter(|hwnd| h.api.get_window_thread_process_id(*hwnd) != h.pids[0])
        .collect();
    assert!(
        !foreign.is_empty(),
        "fixture must contain renderer-pid children"
    );
    let calls = h.api.take_api_calls();
    for hwnd in &foreign {
        assert!(
            mutations_to(&calls, *hwnd).is_empty(),
            "foreign-pid descendant {hwnd} must never be a Win32 target: {calls:?}"
        );
    }
    for snap in h.caches.snapshots.values() {
        assert!(
            h.pids.contains(&snap.identity.pid),
            "snapshots must only cover KakaoTalk-owned windows"
        );
    }
}

#[test]
fn f19_unknown_host_text_blocks_popup_until_known() {
    let mut h = boot("popup_adfit_webview.json", enabled_settings(), true);
    h.api
        .set_window_text_result(200, WindowText::Unknown { error_code: 5 });
    for _ in 0..2 {
        let eval = h.tick();
        assert!(!eval.actions.close.contains(&200));
        assert!(!eval.actions.close.contains(&201));
        assert!(h.api.is_window_visible(200));
    }

    // A truncated-but-known title still carries its text for the guard.
    h.api
        .set_window_text_result(200, WindowText::Truncated("사진 보기".to_string()));
    let eval = h.tick();
    assert!(!eval.actions.close.contains(&200));
    assert!(h.api.is_window_visible(200));

    // Once the empty title is readable, the popup is dismissed.
    h.api.clear_window_text_result(200);
    h.tick();
    assert!(
        !h.api.is_window_visible(200),
        "readable empty host re-arms dismissal"
    );
}

#[test]
fn f21_all_suppression_precedes_close_and_child_snapshot_keeps_visibility() {
    let mut h = boot("popup_adfit_webview.json", enabled_settings(), true);
    h.api.set_ancestor_visibility(true);
    h.api.set_close_behavior(200, CloseBehavior::Timeout);
    h.tick();
    let calls = h.api.take_api_calls();
    let first_close = calls
        .iter()
        .position(|c| c.method == "send_message_timeout")
        .unwrap();
    for target in [200, 201] {
        let hide = calls
            .iter()
            .position(|c| c.target == target && c.method == "show_window")
            .unwrap();
        assert!(
            hide < first_close,
            "all ads must be hidden before any slow close"
        );
        assert!(
            h.caches
                .snapshots
                .values()
                .find(|s| s.identity.hwnd == target)
                .unwrap()
                .was_visible
        );
    }
    h.caches.drain_restore_all(&h.api);
    assert!(h.api.is_window_visible(200));
    assert!(h.api.is_window_visible(201));
}
