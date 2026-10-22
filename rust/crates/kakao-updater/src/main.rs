#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use kakao_updater::{
    UpdateOptions, discard_replacement, relaunch_previous, should_relaunch_previous,
    update_executable_with,
};
use std::path::PathBuf;
use std::time::Duration;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(
    name = "kakao-updater",
    about = "KakaoTalk Layout AdBlocker updater helper"
)]
struct Cli {
    /// Print compatibility token without touching files or launching the app.
    #[arg(long)]
    protocol_version: bool,
    /// Restore the backup; defaults to the packaged app beside this helper.
    #[arg(long, conflicts_with = "replacement")]
    recover: bool,
    #[arg(long, default_value_t = 0)]
    pid: u32,

    #[arg(long, required_unless_present_any = ["recover", "protocol_version"])]
    current: Option<PathBuf>,

    #[arg(long, required_unless_present_any = ["recover", "protocol_version"])]
    replacement: Option<PathBuf>,

    #[arg(long, default_value_t = 30)]
    timeout_secs: u64,

    #[arg(long, default_value_t = false)]
    no_relaunch: bool,

    /// Argument to pass to the relaunched app. Repeat for multiple arguments.
    #[arg(long = "relaunch-arg")]
    relaunch_arg: Vec<String>,

    /// Expected SHA-256 of the replacement, re-verified immediately before the
    /// swap. The app already verified the download; this covers the gap while
    /// the staged file sits in %TEMP% waiting for the helper to start.
    #[arg(long)]
    sha256: Option<String>,
}

fn show_error_dialog(message: &str) {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        use windows::core::HSTRING;
        let text = HSTRING::from(message);
        let title = HSTRING::from("업데이트 실패");
        unsafe {
            let _ = MessageBoxW(None, &text, &title, MB_OK | MB_ICONERROR);
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!("Update Error: {message}");
    }
}

fn main() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .try_init();

    let mut raw: Vec<_> = std::env::args_os().collect();
    if raw.len() == 1 {
        raw.push("--recover".into());
    }
    let args = Cli::parse_from(raw);
    if args.protocol_version {
        println!("{}", kakao_updater::HELPER_PROTOCOL);
        return;
    }
    let current = args.current.clone().unwrap_or_else(|| {
        std::env::current_exe()
            .expect("helper path")
            .with_file_name("KakaoTalkLayoutAdBlocker_v11.exe")
    });
    if args.recover {
        let result =
            kakao_updater::wait_for_process_exit(args.pid, Duration::from_secs(args.timeout_secs))
                .and_then(|()| kakao_updater::restore_previous_version(&current));
        if let Err(err) = result {
            show_error_dialog(&format!(
                "{err}\n차단기를 종료한 뒤 다시 시도하세요. 백업은 보존됩니다."
            ));
            std::process::exit(1);
        }
        if !args.no_relaunch && !relaunch_previous(&current, &args.relaunch_arg) {
            show_error_dialog("이전 버전을 복원했지만 실행하지 못했습니다. 백업은 보존됩니다.");
            std::process::exit(1);
        }
        return;
    }
    let replacement = args
        .replacement
        .as_ref()
        .expect("clap requires replacement");
    let relaunch = !args.no_relaunch;

    match update_executable_with(
        &current,
        replacement,
        args.pid,
        Duration::from_secs(args.timeout_secs),
        &UpdateOptions {
            relaunch,
            relaunch_args: args.relaunch_arg.clone(),
            expected_sha256: args.sha256.clone(),
            readiness_timeout: args.sha256.as_ref().map(|_| Duration::from_secs(60)),
        },
    ) {
        Ok(()) => {
            info!("update helper finished successfully");
            std::process::exit(0);
        }
        Err(err) => {
            error!(%err, "update helper failed");
            // A malformed manual invocation must never delete the installed
            // image by treating it as a failed download.
            if replacement.canonicalize().ok() != current.canonicalize().ok()
                && replacement.canonicalize().ok()
                    != kakao_updater::backup_path_for(&current).canonicalize().ok()
            {
                discard_replacement(replacement);
            }
            // Start the previous version first so the user is not left
            // without the tray app while the error dialog is open.
            let relaunched = relaunch
                && should_relaunch_previous(&err)
                && relaunch_previous(&current, &args.relaunch_arg);
            let message = if relaunched {
                format!(
                    "{err}

이전 버전으로 다시 실행했습니다."
                )
            } else {
                err.to_string()
            };
            show_error_dialog(&message);
            std::process::exit(1);
        }
    }
}
