//! Startup tracing initialization and panic/crash report persistence
//! backed by `tableminus_services::diagnostics`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Once;

use tableminus_services::diagnostics::{
    CrashReport, init_logging, write_crash_report as write_service_crash_report,
};

use crate::error::Result;

static LOG_INIT: Once = Once::new();

/// Initializes structured tracing and registers a panic crash-report hook.
///
/// Safe to call multiple times; initialization runs at most once per process.
pub fn init_logging_and_crash_reports(log_dir: &Path) -> Result<()> {
    fs::create_dir_all(log_dir)?;
    let crash_dir = log_dir.to_path_buf();

    LOG_INIT.call_once(|| {
        init_logging("tableminus=info,warn");
        install_panic_hook(crash_dir);
    });

    Ok(())
}

fn install_panic_hook(log_dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = write_crash_report(&log_dir, &info.to_string());
        previous(info);
    }));
}

/// Writes an atomic crash report file into `log_dir` and returns its path.
pub fn write_crash_report(log_dir: &Path, message: &str) -> Result<PathBuf> {
    let report = CrashReport {
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        os_version: std::env::consts::OS.to_owned(),
        message: message.to_owned(),
        location: None,
        backtrace: None,
    };
    let path = write_service_crash_report(log_dir, &report)?;
    Ok(path)
}
