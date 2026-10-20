use std::collections::HashMap;
use std::time::{Duration, Instant};

use kakao_core::{LayoutRules, LayoutSettings};
use kakao_win32::Win32Api;

use crate::args::Args;
use crate::config::RuntimePaths;
use crate::dump::{dump_payload, dump_payload_with_states, write_json};
use crate::observability::file_stamp;

/// Runs `--dump-tree` / `--dump-tree-series` and returns the process exit code.
/// The caller only invokes this when one of the dump flags is set.
pub fn run_dump_commands(
    api: &dyn Win32Api,
    pids: &[i64],
    core: &LayoutSettings,
    rules: &LayoutRules,
    paths: &RuntimePaths,
    args: &Args,
) -> i32 {
    let interval = args.dump_series_interval_ms.max(10);
    let dump_dir = args.dump_dir.clone().unwrap_or(paths.appdata_dir.clone());
    if args.dump_tree {
        let payload = dump_payload(api, pids, core, rules);
        let empty_windows = payload
            .get("windows")
            .and_then(|v| v.as_array())
            .is_none_or(|a| a.is_empty());
        let empty_owned = payload
            .get("owned_popups")
            .and_then(|v| v.as_array())
            .is_none_or(|a| a.is_empty());
        if empty_windows && empty_owned {
            eprintln!("no KakaoTalk windows");
            return 1;
        }
        let path = dump_dir.join(format!("window_dump_{}.json", file_stamp()));
        if let Err(err) = write_json(&path, &payload) {
            eprintln!("{err}");
            return 1;
        }
        println!("{}", path.display());
        return 0;
    }
    let mut frames = Vec::new();
    let mut states = HashMap::new();
    let started = Instant::now();
    let deadline = started + Duration::from_millis(args.dump_series_duration_ms);
    loop {
        #[cfg(windows)]
        let live_pids: Vec<i64> = kakao_win32::process::kakaotalk_pids().into_iter().collect();
        #[cfg(windows)]
        let pids: &[i64] = &live_pids;
        let frame_started = Instant::now();
        let mut payload = dump_payload_with_states(api, pids, core, rules, &mut states);
        payload["elapsed_ms"] =
            serde_json::json!(frame_started.duration_since(started).as_secs_f64() * 1000.0);
        payload["capture_us"] = serde_json::json!(frame_started.elapsed().as_micros() as u64);
        frames.push(payload);
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(interval));
    }
    let series = serde_json::json!({
        "timestamp": file_stamp(),
        "duration_ms": args.dump_series_duration_ms,
        "interval_ms": interval,
        "frames": frames,
    });
    let path = dump_dir.join(format!("window_dump_series_{}.json", file_stamp()));
    if let Err(err) = write_json(&path, &series) {
        eprintln!("{err}");
        return 1;
    }
    println!("{}", path.display());
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    #[test]
    fn series_records_actual_monotonic_frame_times() {
        let dir = std::env::temp_dir().join(format!("kakao_series_timing_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let args = Args::parse_from([
            "test",
            "--dump-tree-series",
            "--dump-series-duration-ms",
            "20",
            "--dump-series-interval-ms",
            "10",
            "--dump-dir",
            dir.to_str().unwrap(),
        ]);
        let paths = RuntimePaths {
            appdata_dir: dir.clone(),
            settings_file: dir.join("settings"),
            rules_file: dir.join("rules"),
            log_file: dir.join("log"),
        };
        let api = kakao_win32::FakeWin32::from_dump_json(r#"{"pids":[],"windows":[]}"#).unwrap();
        assert_eq!(
            run_dump_commands(
                &api,
                &[],
                &LayoutSettings::default(),
                &LayoutRules::default(),
                &paths,
                &args
            ),
            0
        );
        let file = std::fs::read_dir(&dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        let frames = value["frames"].as_array().unwrap();
        assert!(!frames.is_empty()); // A slow PID scan may consume the requested duration in one frame.
        assert!(frames.iter().all(|f| f["capture_us"].is_u64()));
        assert!(
            frames
                .windows(2)
                .all(|w| w[0]["elapsed_ms"].as_f64().unwrap()
                    <= w[1]["elapsed_ms"].as_f64().unwrap())
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
