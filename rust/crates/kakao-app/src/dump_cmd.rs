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
    let deadline = Instant::now() + Duration::from_millis(args.dump_series_duration_ms);
    loop {
        #[cfg(windows)]
        let live_pids: Vec<i64> = kakao_win32::process::kakaotalk_pids().into_iter().collect();
        #[cfg(windows)]
        let pids: &[i64] = &live_pids;
        let payload = dump_payload_with_states(api, pids, core, rules, &mut states);
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
