#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use kakao_updater::{
    discard_replacement, relaunch_previous, should_relaunch_previous, update_executable_with,
    UpdateOptions,
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
    #[arg(long, default_value_t = 0)]
    pid: u32,

    #[arg(long)]
    current: PathBuf,

    #[arg(long)]
    replacement: PathBuf,

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
        use windows::core::HSTRING;
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
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

    let args = Cli::parse();
    let relaunch = !args.no_relaunch;

    match update_executable_with(
        &args.current,
        &args.replacement,
        args.pid,
        Duration::from_secs(args.timeout_secs),
        &UpdateOptions {
            relaunch,
            relaunch_args: args.relaunch_arg.clone(),
            expected_sha256: args.sha256.clone(),
        },
    ) {
        Ok(()) => {
            info!("update helper finished successfully");
            std::process::exit(0);
        }
        Err(err) => {
            error!(%err, "update helper failed");
            discard_replacement(&args.replacement);
            // Start the previous version first so the user is not left
            // without the tray app while the error dialog is open.
            let relaunched = relaunch
                && should_relaunch_previous(&err)
                && relaunch_previous(&args.current, &args.relaunch_arg);
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
