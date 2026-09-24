use crate::config::AppSettings;

/// Registry → setting direction of the startup sync (Python v11 contract:
/// "시작 시 `run_on_startup` 값을 레지스트리 상태로 1회 동기화").
///
/// When the setting says "off" but a Run value exists (the settings file was
/// reset to defaults, or edited by hand), the app still starts at logon, so
/// the tray check mark must say so. Returns whether `settings` changed. The
/// opposite direction (setting on, registration missing) is handled by
/// `maybe_repair_startup_registration`, which re-registers.
pub fn adopt_registry_startup_state(settings: &mut AppSettings, run_command: Option<&str>) -> bool {
    if settings.run_on_startup || run_command.is_none_or(|cmd| cmd.trim().is_empty()) {
        return false;
    }
    settings.run_on_startup = true;
    true
}

/// Repairs the HKCU Run registration when it is missing/stale, preserving a
/// user-customized command. No-op unless `run_on_startup` is enabled.
pub fn maybe_repair_startup_registration(settings: &AppSettings) {
    if !settings.run_on_startup {
        return;
    }
    let expected = crate::startup::build_command();
    let health = crate::startup::registration_health(
        crate::startup::current_command().as_deref(),
        &expected,
    );
    if crate::startup::should_repair_registration(health) {
        if crate::startup::set_enabled(true) {
            tracing::info!("시작프로그램 등록을 현재 실행 파일로 복구했습니다. status={health}");
        } else {
            tracing::warn!(
                "시작프로그램 등록이 없거나 대상이 사라져 복구를 시도했으나 실패했습니다. status={health}"
            );
        }
    } else if health == "custom" {
        tracing::info!("시작프로그램에 사용자 지정 명령이 있어 자동 복구하지 않습니다.");
    } else {
        #[cfg(windows)]
        if !kakao_win32::startup::ensure_startup_approved_enabled() {
            tracing::warn!(
                "시작프로그램은 등록되어 있으나 Windows가 꺼 둔 시작 앱 상태를 켜지 못했습니다."
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_run_value_turns_the_setting_on() {
        let mut settings = AppSettings {
            run_on_startup: false,
            ..AppSettings::default()
        };
        assert!(adopt_registry_startup_state(
            &mut settings,
            Some(r#""C:\a.exe" --startup-launch"#)
        ));
        assert!(settings.run_on_startup);
    }

    #[test]
    fn missing_or_empty_run_value_changes_nothing() {
        let mut settings = AppSettings {
            run_on_startup: false,
            ..AppSettings::default()
        };
        assert!(!adopt_registry_startup_state(&mut settings, None));
        assert!(!adopt_registry_startup_state(&mut settings, Some("  ")));
        assert!(!settings.run_on_startup);

        let mut on = AppSettings {
            run_on_startup: true,
            ..AppSettings::default()
        };
        assert!(
            !adopt_registry_startup_state(&mut on, None),
            "on + missing is the repair path"
        );
        assert!(on.run_on_startup);
    }
}
