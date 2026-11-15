//! Pre-GUI CLI flags (`--version`, `--help`, and `--smoke-startup`) for `tableminus.exe`.

use std::ffi::OsString;
use std::io::Write;
use std::time::Instant;

use tableminus_services::REGISTERED_ENGINE_IDS;
use tableminus_windows_platform::AppSettings;

use crate::app::AppController;

/// Result of handling top-level application flags before GUI or `tablepro-cli` dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppFlagOutcome {
    /// Handled flag; process should exit with the given code.
    Exit(u8),
    /// Not an app-level flag; continue normal CLI/GUI dispatch.
    Continue,
}

/// Inspects `args` for `--version`, `--help`, or `--smoke-startup` and writes output to `out`.
pub fn handle_app_flags(args: &[OsString], out: &mut dyn Write) -> AppFlagOutcome {
    let Some(first) = args.get(1).and_then(|s| s.to_str()) else {
        return AppFlagOutcome::Continue;
    };

    match first {
        "--version" | "-V" => {
            let _ = writeln!(out, "tableminus {}", env!("CARGO_PKG_VERSION"));
            AppFlagOutcome::Exit(0)
        }
        "--help" | "-h" => {
            let _ = writeln!(
                out,
                "TableMinus {} — Native Windows Database Workbench\n\n\
                 USAGE:\n    \
                     tableminus.exe [OPTIONS] [FILE_OR_URL...]\n    \
                     tableminus.exe <SUBCOMMAND>\n\n\
                 OPTIONS:\n    \
                     -h, --help             Print help information\n    \
                     -V, --version          Print version information\n    \
                         --smoke-startup    Run headless startup timing/driver probe and exit",
                env!("CARGO_PKG_VERSION")
            );
            AppFlagOutcome::Exit(0)
        }
        "--smoke-startup" => run_smoke_startup(out),
        _ => AppFlagOutcome::Continue,
    }
}

fn run_smoke_startup(out: &mut dyn Write) -> AppFlagOutcome {
    let started = Instant::now();
    let controller = AppController::with_detected_theme(AppSettings::default());
    let _layout = controller.layout_config;
    let driver_count = controller
        .services
        .as_ref()
        .map_or(0, |s| s.runtime.drivers().len());
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let _ = writeln!(
        out,
        "tableminus smoke-startup ok: version={} drivers={}/{} locale={:?} elapsed_ms={elapsed_ms:.2}",
        env!("CARGO_PKG_VERSION"),
        driver_count,
        REGISTERED_ENGINE_IDS.len(),
        controller.locale,
    );
    if driver_count == REGISTERED_ENGINE_IDS.len() {
        AppFlagOutcome::Exit(0)
    } else {
        AppFlagOutcome::Exit(1)
    }
}
