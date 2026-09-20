use std::path::PathBuf;

use clap::Parser;

use crate::config::VERSION;

#[derive(Parser, Debug)]
#[command(name = "kakao-adblock-rs", version = VERSION)]
pub struct Args {
    #[arg(long)]
    pub minimized: bool,
    #[arg(long, hide = true)]
    pub startup_launch: bool,
    #[arg(long)]
    pub dump_tree: bool,
    #[arg(long)]
    pub dump_tree_series: bool,
    #[arg(long)]
    pub dump_dir: Option<PathBuf>,
    #[arg(long, default_value_t = 1000)]
    pub dump_series_duration_ms: u64,
    #[arg(long, default_value_t = 100)]
    pub dump_series_interval_ms: u64,
    #[arg(long)]
    pub self_check: bool,
    #[arg(long, hide = true)]
    pub strict_self_check: bool,
    #[arg(long)]
    pub json: bool,
    #[arg(long, hide = true)]
    pub self_check_report: Option<PathBuf>,
    #[arg(long)]
    pub shadow: bool,
    #[arg(long)]
    pub apply: bool,
    #[arg(long, hide = true)]
    pub check_update: bool,
    #[arg(long, hide = true)]
    pub startup_trace: Option<PathBuf>,
    #[arg(long, hide = true)]
    pub exit_after_startup_ms: Option<u64>,
}

/// GUI-subsystem release EXE has no console on Explorer double-click.
/// Attach the parent console only for diagnostic CLI flags, not tray launch.
pub fn should_attach_parent_console<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().any(|arg| {
        let a = arg.as_ref();
        !matches!(a, "--minimized" | "--startup-launch" | "--apply")
            && !a.starts_with("--startup-trace")
            && !a.starts_with("--exit-after-startup-ms")
    })
}

#[cfg(test)]
mod tests {
    use super::should_attach_parent_console;

    #[test]
    fn tray_launch_does_not_attach_console() {
        assert!(!should_attach_parent_console(Vec::<&str>::new()));
        assert!(!should_attach_parent_console(["--minimized"]));
        assert!(!should_attach_parent_console([
            "--startup-launch",
            "--minimized"
        ]));
        assert!(!should_attach_parent_console(["--apply"]));
    }

    #[test]
    fn diagnostic_cli_attaches_parent_console() {
        assert!(should_attach_parent_console(["--self-check"]));
        assert!(should_attach_parent_console(["--dump-tree"]));
        assert!(should_attach_parent_console(["--shadow"]));
        assert!(should_attach_parent_console(["--help"]));
    }
}
