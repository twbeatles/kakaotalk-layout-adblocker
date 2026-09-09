use std::env;
use std::path::{Path, PathBuf};

pub const STARTUP_NAME: &str = "KakaoTalkAdBlockerLayout";
pub const PACKAGED_EXE_NAME: &str = "KakaoTalkLayoutAdBlocker_v11.exe";
const STARTUP_ARGS: &[&str] = &["--startup-launch", "--minimized"];

pub fn normalize_exe_path(path: &Path) -> PathBuf {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

pub fn build_command_from(exe: &Path) -> String {
    format!(
        "\"{}\" --startup-launch --minimized",
        normalize_exe_path(exe).display()
    )
}

pub fn build_command() -> String {
    let exe = env::current_exe().unwrap_or_else(|_| PathBuf::from(PACKAGED_EXE_NAME));
    build_command_from(&exe)
}

pub fn should_repair_registration(health: &str) -> bool {
    matches!(health, "missing" | "missing_target" | "stale")
}

pub fn registration_health(current: Option<&str>, expected: &str) -> &'static str {
    let packaged_runtime = env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name().map(|name| {
                name.to_string_lossy()
                    .eq_ignore_ascii_case(PACKAGED_EXE_NAME)
            })
        })
        .unwrap_or(false);
    registration_health_with(current, expected, packaged_runtime, |path| {
        Path::new(path).exists()
    })
}

pub fn registration_health_with(
    current: Option<&str>,
    expected: &str,
    packaged_runtime: bool,
    exists: impl Fn(&str) -> bool,
) -> &'static str {
    match current {
        None => "missing",
        Some(cmd) if cmd == expected => "healthy",
        Some(cmd) if is_source_compatible(cmd, packaged_runtime, &exists) => "source-compatible",
        Some(cmd) if is_managed_command(cmd, expected) => {
            if command_target_missing(cmd, &exists) {
                "missing_target"
            } else {
                "stale"
            }
        }
        Some(_) => "custom",
    }
}

fn command_exe_path(command: &str) -> Option<&str> {
    let command = command.trim();
    if let Some(rest) = command.strip_prefix('"') {
        rest.split_once('"').map(|(path, _)| path)
    } else {
        command.split_whitespace().next()
    }
}

fn command_args_after_exe(command: &str) -> Vec<&str> {
    let command = command.trim();
    if let Some(rest) = command.strip_prefix('"') {
        return rest
            .split_once('"')
            .map(|(_, rest)| rest.split_whitespace().collect())
            .unwrap_or_default();
    }
    command.split_whitespace().skip(1).collect()
}

fn path_file_name(path: &str) -> Option<&str> {
    Path::new(path).file_name().and_then(|name| name.to_str())
}

fn is_source_compatible(
    command: &str,
    packaged_runtime: bool,
    exists: &impl Fn(&str) -> bool,
) -> bool {
    if packaged_runtime {
        return false;
    }
    let Some(path) = command_exe_path(command) else {
        return false;
    };
    let Some(name) = path_file_name(path) else {
        return false;
    };
    if !name.eq_ignore_ascii_case(PACKAGED_EXE_NAME) {
        return false;
    }
    if !exists(path) {
        return false;
    }
    command_args_after_exe(command) == STARTUP_ARGS
}

fn is_managed_command(command: &str, expected: &str) -> bool {
    let Some(path) = command_exe_path(command) else {
        return false;
    };
    if path_file_name(path).is_some_and(|name| name.eq_ignore_ascii_case(PACKAGED_EXE_NAME)) {
        return true;
    }
    if let Some(expected_path) = command_exe_path(expected) {
        if let (Some(actual_name), Some(expected_name)) =
            (path_file_name(path), path_file_name(expected_path))
        {
            if actual_name.eq_ignore_ascii_case(expected_name) {
                return true;
            }
        }
    }
    false
}

fn command_target_missing(command: &str, exists: &impl Fn(&str) -> bool) -> bool {
    command_exe_path(command).is_some_and(|path| !path.is_empty() && !exists(path))
}

pub fn set_enabled(enabled: bool) -> bool {
    #[cfg(windows)]
    {
        if enabled {
            kakao_win32::startup::set_run_command(&build_command())
        } else {
            kakao_win32::startup::delete_run_command()
        }
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        false
    }
}

pub fn current_command() -> Option<String> {
    #[cfg(windows)]
    {
        kakao_win32::startup::get_run_command()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn missing_registry_value_is_missing() {
        let expected = r#""D:\App\KakaoTalkLayoutAdBlocker_v11.exe" --startup-launch --minimized"#;
        assert_eq!(registration_health(None, expected), "missing");
    }

    #[test]
    fn exact_match_is_healthy() {
        let cmd = r#""D:\App\KakaoTalkLayoutAdBlocker_v11.exe" --startup-launch --minimized"#;
        assert_eq!(registration_health(Some(cmd), cmd), "healthy");
    }

    #[test]
    fn missing_packaged_path_is_missing_target_not_source_compatible() {
        let current =
            r#""C:\Missing\KakaoTalkLayoutAdBlocker_v11.exe" --startup-launch --minimized"#;
        let expected = r#""D:\App\KakaoTalkLayoutAdBlocker_v11.exe" --startup-launch --minimized"#;
        assert_eq!(
            registration_health(Some(current), expected),
            "missing_target"
        );
    }

    #[test]
    fn unrelated_command_is_custom() {
        let current = r#""C:\Other\helper.exe" --foo"#;
        let expected = r#""D:\App\KakaoTalkLayoutAdBlocker_v11.exe" --startup-launch --minimized"#;
        assert_eq!(registration_health(Some(current), expected), "custom");
    }

    #[test]
    fn stale_packaged_path_is_repaired_when_running_as_packaged() {
        let current = r#""C:\Old\KakaoTalkLayoutAdBlocker_v11.exe" --startup-launch --minimized"#;
        let expected = r#""D:\New\KakaoTalkLayoutAdBlocker_v11.exe" --startup-launch --minimized"#;
        assert_eq!(
            registration_health_with(Some(current), expected, true, |_| true),
            "stale"
        );
        assert!(should_repair_registration("stale"));
        assert!(should_repair_registration("missing"));
        assert!(should_repair_registration("missing_target"));
        assert!(!should_repair_registration("healthy"));
        assert!(!should_repair_registration("source-compatible"));
        assert!(!should_repair_registration("custom"));
    }

    #[test]
    fn cargo_runtime_keeps_existing_packaged_exe() {
        let current = r#""C:\Apps\KakaoTalkLayoutAdBlocker_v11.exe" --startup-launch --minimized"#;
        let expected =
            r#""C:\src\target\release\kakao-adblock-rs.exe" --startup-launch --minimized"#;
        assert_eq!(
            registration_health_with(Some(current), expected, false, |_| true),
            "source-compatible"
        );
        assert!(!should_repair_registration("source-compatible"));
    }

    #[test]
    fn build_command_strips_extended_path_prefix() {
        let cmd = build_command_from(Path::new(r"\\?\D:\Apps\KakaoTalkLayoutAdBlocker_v11.exe"));
        assert_eq!(
            cmd,
            r#""D:\Apps\KakaoTalkLayoutAdBlocker_v11.exe" --startup-launch --minimized"#
        );
    }
}
