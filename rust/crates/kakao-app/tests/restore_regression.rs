use std::fs;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

use kakao_app::config::AppSettings;
use kakao_app::engine::{tick, EngineCaches, SharedFlags};
use kakao_core::LayoutRules;
use kakao_win32::{FakeWin32, Win32Api};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repo root")
}

fn load_owned_popup_fake() -> (FakeWin32, Vec<i64>) {
    let dump = fs::read_to_string(
        repo_root().join("tests/fixtures/window_dumps/owned_popup_legacy_ad.json"),
    )
    .unwrap();
    let api = FakeWin32::from_dump_json(&dump).unwrap();
    let pids = api.pids();
    (api, pids)
}

fn load_normal_main_fake() -> (FakeWin32, Vec<i64>) {
    let dump =
        fs::read_to_string(repo_root().join("tests/fixtures/window_dumps/normal_main_window.json"))
            .unwrap();
    let api = FakeWin32::from_dump_json(&dump).unwrap();
    let pids = api.pids();
    (api, pids)
}

#[test]
fn normal_main_view_resize_not_saved_in_restore_snapshots() {
    let (api, pids) = load_normal_main_fake();
    let settings = AppSettings {
        enabled: true,
        aggressive_mode: false,
        ..AppSettings::default()
    };
    let flags = SharedFlags::from_settings(&settings, true);
    let rules = LayoutRules::default();
    let mut caches = EngineCaches::new();

    let eval = tick(&api, &pids, &settings, &rules, &mut caches, &flags);

    // Main view (OnlineMainView: 65860) resized
    assert!(!eval.actions.set_pos.is_empty());
    let resized_hwnd = eval.actions.set_pos[0][0];

    // OnlineMainView resize must NOT be recorded in snapshots (prevents black screen regression)
    assert!(
        !caches
            .snapshots
            .values()
            .any(|s| s.identity.hwnd == resized_hwnd),
        "Normal main view resize must not be captured in restore snapshots"
    );
}

#[test]
fn hidden_ad_restored_on_disable() {
    let (api, pids) = load_owned_popup_fake();
    let settings = AppSettings {
        enabled: true,
        aggressive_mode: true,
        ..AppSettings::default()
    };
    let flags = SharedFlags::from_settings(&settings, true);
    let rules = LayoutRules::default();
    let mut caches = EngineCaches::new();

    let eval = tick(&api, &pids, &settings, &rules, &mut caches, &flags);

    let ad_hwnd = 527936;
    assert!(eval.actions.hide.contains(&ad_hwnd));
    assert!(!api.is_window_visible(ad_hwnd));
    assert!(caches
        .snapshots
        .values()
        .any(|s| s.identity.hwnd == ad_hwnd));

    // Disable blocker -> restore_all
    flags.enabled.store(false, Ordering::SeqCst);
    let (failures, err) = caches.drain_restore_all(&api);
    assert_eq!(failures, 0, "restore_all failed: {err}");
    assert!(
        api.is_window_visible(ad_hwnd),
        "Hidden ad must be restored visible when disabled"
    );
}

#[test]
fn stale_hide_restored_after_two_miss_ticks() {
    let (api, pids) = load_owned_popup_fake();
    let settings = AppSettings {
        enabled: true,
        aggressive_mode: true,
        ..AppSettings::default()
    };
    let flags = SharedFlags::from_settings(&settings, true);
    let rules = LayoutRules::default();
    let mut caches = EngineCaches::new();

    let ad_hwnd = 527936;

    // Tick 1: ad matches and gets hidden
    tick(&api, &pids, &settings, &rules, &mut caches, &flags);
    assert!(!api.is_window_visible(ad_hwnd));
    assert!(caches
        .snapshots
        .values()
        .any(|s| s.identity.hwnd == ad_hwnd));

    // Change ad window text to non-ad (e.g. Chat room title)
    api.set_text(ad_hwnd, "프로그래밍 토크방");

    // Tick 2: ad no longer matches (miss 1, still within threshold 2)
    tick(&api, &pids, &settings, &rules, &mut caches, &flags);
    assert!(
        !api.is_window_visible(ad_hwnd),
        "Should remain hidden on first miss tick (grace period)"
    );

    // Tick 3: miss 2 reached threshold -> automatically restored!
    tick(&api, &pids, &settings, &rules, &mut caches, &flags);
    assert!(
        api.is_window_visible(ad_hwnd),
        "Window must be automatically restored after 2 miss ticks"
    );
    assert!(
        !caches
            .snapshots
            .values()
            .any(|s| s.identity.hwnd == ad_hwnd),
        "Restored window must be removed from snapshots"
    );
}

#[test]
fn hwnd_reuse_different_pid_or_class_skips_restore() {
    let (api, pids) = load_owned_popup_fake();
    let settings = AppSettings {
        enabled: true,
        aggressive_mode: true,
        ..AppSettings::default()
    };
    let flags = SharedFlags::from_settings(&settings, true);
    let rules = LayoutRules::default();
    let mut caches = EngineCaches::new();

    let ad_hwnd = 527936;
    tick(&api, &pids, &settings, &rules, &mut caches, &flags);
    assert!(!api.is_window_visible(ad_hwnd));

    // HWND was reused by another process with different PID
    api.set_pid(ad_hwnd, 99999);

    let (failures, _) = caches.drain_restore_all(&api);
    // Identity mismatch skipped restore safely
    assert_eq!(failures, 0);
    // The window on different PID was NOT touched (remains invisible/untouched by blocker)
    assert!(!api.is_window_visible(ad_hwnd));
}

#[test]
fn kakaotalk_restart_ignores_stale_snapshots() {
    let (api, pids) = load_owned_popup_fake();
    let settings = AppSettings {
        enabled: true,
        aggressive_mode: true,
        ..AppSettings::default()
    };
    let flags = SharedFlags::from_settings(&settings, true);
    let rules = LayoutRules::default();
    let mut caches = EngineCaches::new();

    let ad_hwnd = 527936;
    tick(&api, &pids, &settings, &rules, &mut caches, &flags);

    // Simulate KakaoTalk restart: old window is gone / re-created under new PID
    let _new_pids = [88888];
    api.set_pid(ad_hwnd, 88888);
    api.set_class_name(ad_hwnd, "DifferentClass");

    let (failures, _) = caches.drain_restore_all(&api);
    assert_eq!(failures, 0);
}

#[test]
fn popup_hide_fallback_stays_hidden_across_ticks() {
    let dump = fs::read_to_string(
        repo_root().join("tests/fixtures/window_dumps/popup_adfit_webview.json"),
    )
    .unwrap();
    let api = FakeWin32::from_dump_json(&dump).unwrap();
    let pids = api.pids();
    let settings = AppSettings {
        enabled: true,
        aggressive_mode: true,
        ..AppSettings::default()
    };
    let flags = SharedFlags::from_settings(&settings, true);
    let rules = LayoutRules::default();
    let mut caches = EngineCaches::new();
    let host = 200;

    for _ in 0..20 {
        tick(&api, &pids, &settings, &rules, &mut caches, &flags);
        assert!(
            !api.is_window_visible(host),
            "close-refused popup host must stay hidden while the AdFit signal remains"
        );
    }

    api.set_class_name(201, "NotAnAd");
    tick(&api, &pids, &settings, &rules, &mut caches, &flags);
    tick(&api, &pids, &settings, &rules, &mut caches, &flags);
    assert!(
        api.is_window_visible(host),
        "popup must restore after the ad class disappears and grace ticks elapse"
    );
}

#[test]
fn failed_restore_keeps_snapshot_and_retries() {
    let (api, pids) = load_owned_popup_fake();
    let settings = AppSettings {
        enabled: true,
        aggressive_mode: true,
        ..AppSettings::default()
    };
    let flags = SharedFlags::from_settings(&settings, true);
    let rules = LayoutRules::default();
    let mut caches = EngineCaches::new();
    let ad_hwnd = 527936;

    tick(&api, &pids, &settings, &rules, &mut caches, &flags);
    assert!(!api.is_window_visible(ad_hwnd));
    assert!(caches
        .snapshots
        .values()
        .any(|s| s.identity.hwnd == ad_hwnd));

    api.set_fail_show_window(ad_hwnd, true);
    api.set_fail_set_window_pos(ad_hwnd, true);
    let (failures, _) = caches.drain_restore_all(&api);
    assert_eq!(failures, 1);
    assert!(
        caches
            .snapshots
            .values()
            .any(|s| s.identity.hwnd == ad_hwnd),
        "failed restore must keep the original snapshot"
    );
    assert!(!api.is_window_visible(ad_hwnd));

    api.set_fail_show_window(ad_hwnd, false);
    api.set_fail_set_window_pos(ad_hwnd, false);
    let (failures, err) = caches.drain_restore_all(&api);
    assert_eq!(failures, 0, "retry restore failed: {err}");
    assert!(api.is_window_visible(ad_hwnd));
    assert!(!caches
        .snapshots
        .values()
        .any(|s| s.identity.hwnd == ad_hwnd));
}

#[test]
fn disable_flag_blocks_mutations() {
    let (api, pids) = load_owned_popup_fake();
    let settings = AppSettings {
        enabled: false, // Disabled
        aggressive_mode: true,
        ..AppSettings::default()
    };
    let flags = SharedFlags::from_settings(&settings, true);
    let rules = LayoutRules::default();
    let mut caches = EngineCaches::new();

    let ad_hwnd = 527936;
    let initial_visible = api.is_window_visible(ad_hwnd);

    let _eval = tick(&api, &pids, &settings, &rules, &mut caches, &flags);

    // When disabled, no hide or set_pos mutations applied to Win32
    assert_eq!(api.is_window_visible(ad_hwnd), initial_visible);
    assert!(caches.snapshots.is_empty());
}

#[test]
fn persistent_restore_failure_backs_off_instead_of_retrying_every_tick() {
    // ISSUE-002: a window whose ancestors are hidden refuses to become visible
    // again. Retrying that every tick emitted a warning ~5x/second forever and
    // inflated the failure counter without bound.
    let (api, pids) = load_owned_popup_fake();
    let settings = AppSettings {
        enabled: true,
        aggressive_mode: true,
        ..AppSettings::default()
    };
    let flags = SharedFlags::from_settings(&settings, true);
    let rules = LayoutRules::default();
    let mut caches = EngineCaches::new();
    let ad_hwnd = 527936;

    tick(&api, &pids, &settings, &rules, &mut caches, &flags);
    assert!(!api.is_window_visible(ad_hwnd));

    // Ad signal disappears, so the engine wants to restore it...
    api.set_text(ad_hwnd, "프로그래밍 토크방");
    // ...but the window refuses to come back.
    api.set_fail_show_window(ad_hwnd, true);
    api.set_fail_set_window_pos(ad_hwnd, true);
    api.reset_restore_attempts();

    for _ in 0..40 {
        tick(&api, &pids, &settings, &rules, &mut caches, &flags);
    }

    let attempts = api.restore_attempts(ad_hwnd);
    assert!(
        attempts <= 6,
        "restore must back off, not retry every tick (attempts={attempts})"
    );
    assert!(attempts >= 1, "restore must be attempted at least once");
    assert_eq!(
        flags.restore_failures.load(Ordering::SeqCst),
        1,
        "restore_failures is a gauge of stuck windows, not a running total"
    );
    assert!(
        caches
            .snapshots
            .values()
            .any(|s| s.identity.hwnd == ad_hwnd),
        "a failing restore must keep its snapshot for a later retry"
    );
}

#[test]
fn recovered_window_clears_the_failure_gauge_and_restores() {
    let (api, pids) = load_owned_popup_fake();
    let settings = AppSettings {
        enabled: true,
        aggressive_mode: true,
        ..AppSettings::default()
    };
    let flags = SharedFlags::from_settings(&settings, true);
    let rules = LayoutRules::default();
    let mut caches = EngineCaches::new();
    let ad_hwnd = 527936;

    tick(&api, &pids, &settings, &rules, &mut caches, &flags);
    api.set_text(ad_hwnd, "프로그래밍 토크방");
    api.set_fail_show_window(ad_hwnd, true);
    for _ in 0..6 {
        tick(&api, &pids, &settings, &rules, &mut caches, &flags);
    }
    assert_eq!(flags.restore_failures.load(Ordering::SeqCst), 1);

    // The tray "복원 실패 초기화" path clears the backoff so the next tick retries.
    api.set_fail_show_window(ad_hwnd, false);
    caches.clear_restore_failures();
    for _ in 0..3 {
        tick(&api, &pids, &settings, &rules, &mut caches, &flags);
    }

    assert!(
        api.is_window_visible(ad_hwnd),
        "window must be restored once it can be shown again"
    );
    assert_eq!(flags.restore_failures.load(Ordering::SeqCst), 0);
}

#[test]
fn engine_counters_track_hides_and_resizes() {
    let (api, pids) = load_normal_main_fake();
    let settings = AppSettings {
        enabled: true,
        aggressive_mode: false,
        ..AppSettings::default()
    };
    let flags = SharedFlags::from_settings(&settings, true);
    let rules = LayoutRules::default();
    let mut caches = EngineCaches::new();

    tick(&api, &pids, &settings, &rules, &mut caches, &flags);
    assert!(flags.main_windows.load(Ordering::SeqCst) >= 1);
    assert!(
        flags.resized_windows.load(Ordering::SeqCst) >= 1,
        "main view resize must be counted for the tray status"
    );
}

fn default_flags(settings: &AppSettings) -> std::sync::Arc<SharedFlags> {
    SharedFlags::from_settings(settings, true)
}

const OWNED_AD_HOST: i64 = 527936;
const OWNED_AD_CHILD: i64 = 2032986;

#[test]
fn hung_window_is_neither_hidden_nor_restored() {
    // PROJECT_AUDIT 2026-09-24 ISSUE-003: synchronous ShowWindow/SetWindowPos
    // on a not-responding KakaoTalk could block the worker (and shutdown).
    let (api, pids) = load_owned_popup_fake();
    let settings = AppSettings::default();
    let flags = default_flags(&settings);
    let rules = LayoutRules::default();
    let mut caches = EngineCaches::new();

    api.set_hung(OWNED_AD_HOST, true);
    tick(&api, &pids, &settings, &rules, &mut caches, &flags);
    assert!(
        api.is_window_visible(OWNED_AD_HOST),
        "hung window must not be touched"
    );
    assert!(caches.snapshots.is_empty());

    api.set_hung(OWNED_AD_HOST, false);
    tick(&api, &pids, &settings, &rules, &mut caches, &flags);
    assert!(!api.is_window_visible(OWNED_AD_HOST));

    // KakaoTalk stops responding, then the user disables blocking.
    api.set_hung(OWNED_AD_HOST, true);
    api.reset_restore_attempts();
    let (failures, err) = caches.drain_restore_all(&api);
    assert_eq!(failures, 1);
    assert!(err.contains("not responding"), "{err}");
    assert_eq!(
        api.restore_attempts(OWNED_AD_HOST),
        0,
        "no blocking call while hung"
    );
    assert!(
        caches
            .snapshots
            .values()
            .any(|s| s.identity.hwnd == OWNED_AD_HOST),
        "snapshot must be kept for a later retry"
    );

    api.set_hung(OWNED_AD_HOST, false);
    let (failures, _) = caches.drain_restore_all(&api);
    assert_eq!(failures, 0);
    assert!(api.is_window_visible(OWNED_AD_HOST));
}

#[test]
fn child_restored_under_a_hidden_parent_is_not_a_failure() {
    // PROJECT_AUDIT 2026-09-24 ISSUE-004: IsWindowVisible is false for any
    // child of a hidden window, so correct restores were counted as failures.
    let (api, _pids) = load_owned_popup_fake();
    api.set_ancestor_visibility(true);
    let parent_rect = api.get_window_rect(OWNED_AD_CHILD);
    api.set_visible(OWNED_AD_HOST, false);
    api.set_visible(OWNED_AD_CHILD, false);

    let identity = kakao_core::WindowIdentity {
        hwnd: OWNED_AD_CHILD,
        pid: api.get_window_thread_process_id(OWNED_AD_CHILD),
        class_name: api.get_class_name(OWNED_AD_CHILD),
    };
    let mut snapshots = std::collections::HashMap::new();
    snapshots.insert(
        identity.clone(),
        kakao_app::engine::RestoreSnapshot {
            identity,
            was_visible: true,
            rect: parent_rect,
            top_level: false,
        },
    );
    let (failures, err) = kakao_app::engine::restore_all(&api, &mut snapshots);
    assert_eq!(failures, 0, "unexpected failure: {err}");
    assert!(snapshots.is_empty());
    assert!(api.has_visible_style(OWNED_AD_CHILD));
    assert!(
        !api.is_window_visible(OWNED_AD_CHILD),
        "still not effectively visible while the parent is hidden"
    );
}

#[test]
fn restore_error_clears_itself_after_natural_recovery() {
    // PROJECT_AUDIT 2026-09-24 ISSUE-005: last_error stayed forever after the
    // failing window recovered on its own.
    let (api, pids) = load_owned_popup_fake();
    let settings = AppSettings::default();
    let flags = default_flags(&settings);
    let rules = LayoutRules::default();
    let mut caches = EngineCaches::new();

    tick(&api, &pids, &settings, &rules, &mut caches, &flags);
    api.set_text(OWNED_AD_HOST, "프로그래밍 토크방");
    api.set_fail_show_window(OWNED_AD_HOST, true);
    for _ in 0..4 {
        tick(&api, &pids, &settings, &rules, &mut caches, &flags);
    }
    assert_eq!(flags.restore_failures.load(Ordering::SeqCst), 1);
    assert!(!flags.last_error_text().is_empty());

    api.set_fail_show_window(OWNED_AD_HOST, false);
    for _ in 0..20 {
        tick(&api, &pids, &settings, &rules, &mut caches, &flags);
    }
    assert!(api.is_window_visible(OWNED_AD_HOST));
    assert_eq!(flags.restore_failures.load(Ordering::SeqCst), 0);
    assert_eq!(
        flags.last_error_text(),
        "",
        "recovered restore must clear its error"
    );
}

#[test]
fn hidden_window_snapshot_is_captured_once() {
    // PROJECT_AUDIT 2026-09-24 P5: each tick re-captured the snapshot of an
    // already hidden window (GetWindowRect + IsWindowVisible) and discarded it.
    let (api, pids) = load_owned_popup_fake();
    let settings = AppSettings::default();
    let flags = default_flags(&settings);
    let rules = LayoutRules::default();
    let mut caches = EngineCaches::new();

    let ticks = 6;
    for _ in 0..ticks {
        tick(&api, &pids, &settings, &rules, &mut caches, &flags);
    }
    assert!(!api.is_window_visible(OWNED_AD_HOST));
    // build_graph reads the rect once per tick; capture adds exactly one more.
    assert_eq!(api.rect_queries(OWNED_AD_HOST), ticks + 1);
}
