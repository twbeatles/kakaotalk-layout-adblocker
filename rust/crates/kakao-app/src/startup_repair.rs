use crate::config::AppSettings;

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
