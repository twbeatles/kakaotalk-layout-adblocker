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
