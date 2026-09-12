//! ISSUE-001: rotation used to happen once at startup, so a tray app left
//! running for weeks never checked its log size again.

use std::io::Write;
use std::path::PathBuf;

use kakao_app::config::{atomic_write, rotate_log_if_needed, rotated_log_path, RotatingLog};
use tracing_subscriber::fmt::MakeWriter;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kakao_log_rotation_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn rotate_at_startup_respects_the_threshold() {
    let dir = temp_dir("startup");
    let log = dir.join("layout_adblock.log");

    std::fs::write(&log, vec![b'x'; 1024]).unwrap();
    rotate_log_if_needed(&log);
    assert!(log.is_file(), "a small log must not be rotated");
    assert!(!rotated_log_path(&log).exists());

    std::fs::write(&log, vec![b'x'; 6 * 1024 * 1024]).unwrap();
    rotate_log_if_needed(&log);
    assert!(
        rotated_log_path(&log).is_file(),
        "an oversized log must move to .log.1"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn writer_rotates_while_the_process_keeps_running() {
    let dir = temp_dir("runtime");
    let log = dir.join("layout_adblock.log");
    let max_bytes = 64 * 1024;

    let rotating = RotatingLog::open(&log, max_bytes).expect("open log");
    let record = vec![b'y'; 4096];
    // Well past the cap: the old code would have grown one file forever.
    for _ in 0..64 {
        let mut writer = rotating.make_writer();
        writer.write_all(&record).unwrap();
        writer.flush().unwrap();
    }

    let live = std::fs::metadata(&log).map(|m| m.len()).unwrap_or(0);
    assert!(
        live <= max_bytes,
        "active log must stay under the cap, was {live} bytes"
    );
    assert!(
        rotated_log_path(&log).is_file(),
        "rotation must produce a .log.1"
    );

    drop(rotating);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn atomic_write_uses_a_process_unique_temp_name() {
    // `--self-check` runs outside the single-instance mutex, so two processes
    // can write these files at the same time. A shared `foo.tmp` let them
    // truncate each other.
    let dir = temp_dir("atomic");
    let target = dir.join("layout_settings_v11.json");

    atomic_write(&target, "{\"enabled\":true}\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "{\"enabled\":true}\n"
    );

    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "temp files must not survive a successful write: {leftovers:?}"
    );

    let shared = dir.join("layout_settings_v11.tmp");
    assert!(
        !shared.exists(),
        "the old shared temp name must no longer be used"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
