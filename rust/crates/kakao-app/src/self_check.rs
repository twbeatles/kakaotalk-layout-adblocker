use std::path::Path;

use serde_json::{json, Value};

use crate::config::{ensure_runtime_files, load_rules, load_settings, runtime_paths, VERSION};

pub fn run(as_json: bool, report_path: Option<&Path>, strict: bool) -> i32 {
    let paths = runtime_paths();
    let _ = std::fs::create_dir_all(&paths.appdata_dir);
    let bootstrap_warnings = ensure_runtime_files(&paths);
    let (settings, settings_warnings) = load_settings(&paths.settings_file);
    let (_rules, rules_warnings) = load_rules(&paths.rules_file);
    let appdata_ok = paths.appdata_dir.is_dir();
    let appdata_writable = probe_writable(&paths.appdata_dir);
    let win32_ok = cfg!(windows);

    // Config-load warnings describe successful self-healing (a bad field kept
    // at its default, a swapped banner range, an old *.broken backup). They are
    // informational: failing --strict-self-check on them made the release build
    // depend on whatever happened to be in the build machine's %APPDATA%.
    let mut info_warnings = bootstrap_warnings;
    info_warnings.extend(settings_warnings);
    info_warnings.extend(rules_warnings);
    let mut core_warnings: Vec<String> = Vec::new();
    if !appdata_writable {
        core_warnings.push("APPDATA 디렉터리에 쓸 수 없습니다.".into());
    }

    let registry = probe_registry();
    if !registry.readable {
        core_warnings.push("HKCU Run 레지스트리를 읽을 수 없습니다.".into());
    }
    if !registry.writable {
        core_warnings
            .push("HKCU Run 레지스트리에 쓸 수 없어 시작프로그램 등록이 실패합니다.".into());
    }
    let process_scan = probe_process_scan();
    if !process_scan.enumerated {
        core_warnings.push("프로세스 목록을 열거하지 못했습니다.".into());
    }
    let run_command = crate::startup::current_command();
    let run_health = crate::startup::registration_health(
        run_command.as_deref(),
        &crate::startup::build_command(),
    );

    let mut core_ok = win32_ok && appdata_ok;
    if strict && !core_warnings.is_empty() {
        core_ok = false;
    }
    let mut warnings: Vec<String> = core_warnings.clone();
    warnings.extend(info_warnings.clone());
    let mut exit_code = if core_ok { 0 } else { 1 };
    let core_label = if core_ok { "ok" } else { "fail" };
    let mut payload = json!({
        "version": VERSION,
        "windows": win32_ok,
        "appdata_ok": appdata_ok,
        "appdata_writable": appdata_writable,
        "settings_enabled": settings.enabled,
        "registry_run_readable": registry.readable,
        "registry_run_writable": registry.writable,
        "run_command": run_command,
        "run_command_health": run_health,
        "process_scan_ok": process_scan.enumerated,
        "kakaotalk_process_count": process_scan.kakaotalk_count,
        "warnings": warnings,
        "core_warnings": core_warnings,
        "info_warnings": info_warnings,
        "strict": strict,
        "core": core_label,
        "summary": {
            "exit_code": exit_code,
            "core": core_label
        },
    });
    if let Some(path) = report_path {
        if path.is_dir() {
            eprintln!("self-check report path is a directory: {}", path.display());
            exit_code = 1;
            payload["core"] = json!("fail");
            payload["summary"]["exit_code"] = json!(exit_code);
            payload["summary"]["core"] = json!("fail");
            if let Some(warnings) = payload["warnings"].as_array_mut() {
                warnings.push(json!("self-check 보고서 경로가 디렉터리입니다."));
            }
        } else {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    if let Err(err) = std::fs::create_dir_all(parent) {
                        eprintln!("self-check report directory create failed: {err}");
                        exit_code = 1;
                    }
                }
            }
            if exit_code == 0 || path.is_file() || !path.exists() {
                match serde_json::to_vec_pretty(&payload) {
                    Ok(body) => {
                        if let Err(err) = std::fs::write(path, body) {
                            eprintln!("self-check report write failed: {err}");
                            exit_code = 1;
                        }
                    }
                    Err(err) => {
                        eprintln!("self-check report serialize failed: {err}");
                        exit_code = 1;
                    }
                }
            }
        }
        if exit_code != 0 {
            payload["core"] = json!("fail");
            payload["summary"]["exit_code"] = json!(exit_code);
            payload["summary"]["core"] = json!("fail");
        }
    }
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into())
        );
    } else {
        println!(
            "self-check version={VERSION} windows={win32_ok} appdata_ok={appdata_ok} registry_run={}/{} process_scan={} run_command={run_health}",
            registry.readable, registry.writable, process_scan.enumerated
        );
        for warning in &warnings {
            println!("warning: {warning}");
        }
    }
    exit_code
}

struct RegistryProbe {
    readable: bool,
    writable: bool,
}

/// README promises `--self-check` inspects the registry. Read and write access
/// are probed separately because the startup toggle needs both.
fn probe_registry() -> RegistryProbe {
    #[cfg(windows)]
    {
        RegistryProbe {
            readable: kakao_win32::startup::probe_run_key_readable(),
            writable: kakao_win32::startup::probe_run_key_writable(),
        }
    }
    #[cfg(not(windows))]
    {
        RegistryProbe {
            readable: false,
            writable: false,
        }
    }
}

struct ProcessProbe {
    enumerated: bool,
    kakaotalk_count: usize,
}

/// README promises `--self-check` inspects process discovery. An empty result
/// is normal (KakaoTalk may be closed); a failed snapshot is not.
fn probe_process_scan() -> ProcessProbe {
    #[cfg(windows)]
    {
        match kakao_win32::process::try_process_ids("kakaotalk.exe") {
            Some(pids) => ProcessProbe {
                enumerated: true,
                kakaotalk_count: pids.len(),
            },
            None => ProcessProbe {
                enumerated: false,
                kakaotalk_count: 0,
            },
        }
    }
    #[cfg(not(windows))]
    {
        ProcessProbe {
            enumerated: false,
            kakaotalk_count: 0,
        }
    }
}

fn probe_writable(dir: &Path) -> bool {
    let probe = dir.join(".self-check-write");
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

pub fn as_value() -> Value {
    json!({"version": VERSION})
}
