use kakao_app::self_check;

fn isolated_appdata() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kakao_self_check_appdata_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("APPDATA", &dir);
    dir
}

#[test]
fn report_path_directory_returns_nonzero() {
    let _appdata = isolated_appdata();
    let dir = std::env::temp_dir().join(format!(
        "kakao_self_check_dir_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let code = self_check::run(false, Some(dir.as_path()), false);
    assert_ne!(code, 0, "writing a report onto a directory must fail");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn report_write_success_is_zero_when_core_ok() {
    let _appdata = isolated_appdata();
    let dir = std::env::temp_dir().join(format!(
        "kakao_self_check_ok_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let report = dir.join("report.json");
    let code = self_check::run(false, Some(report.as_path()), false);
    if cfg!(windows) {
        assert_eq!(code, 0);
        assert!(report.is_file());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn report_includes_registry_and_process_probes() {
    // README documents --self-check as inspecting the registry and process
    // discovery; it previously only looked at APPDATA and the config files.
    let _appdata = isolated_appdata();
    let dir = std::env::temp_dir().join(format!(
        "kakao_self_check_probe_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let report = dir.join("report.json");
    let code = self_check::run(false, Some(report.as_path()), false);
    assert_eq!(code, 0);

    let payload: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    for key in [
        "registry_run_readable",
        "registry_run_writable",
        "run_command_health",
        "process_scan_ok",
        "kakaotalk_process_count",
        "core_warnings",
        "info_warnings",
    ] {
        assert!(
            payload.get(key).is_some(),
            "missing self-check field: {key}"
        );
    }
    if cfg!(windows) {
        assert_eq!(payload["registry_run_readable"], serde_json::json!(true));
        assert_eq!(payload["process_scan_ok"], serde_json::json!(true));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn strict_mode_tolerates_informational_config_warnings() {
    // Self-healing config warnings used to fail --strict-self-check, which made
    // the release build depend on the build machine's %APPDATA% contents.
    let appdata = isolated_appdata();
    std::fs::write(
        appdata.join("layout_rules_v11.json"),
        r#"{"banner_min_height_px": 260, "banner_max_height_px": 40}"#,
    )
    .unwrap();

    let code = self_check::run(false, None, true);
    if cfg!(windows) {
        assert_eq!(
            code, 0,
            "a swapped banner range is repaired, not a strict failure"
        );
    }
}
