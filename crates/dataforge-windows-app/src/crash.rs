//! Startup tracing initialization and panic/crash report persistence.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Once;

use tracing_subscriber::EnvFilter;

use crate::error::Result;

static LOG_INIT: Once = Once::new();

/// Initializes structured tracing and registers a panic crash-report hook.
///
/// Safe to call multiple times; initialization runs at most once per process.
pub fn init_logging_and_crash_reports(log_dir: &Path) -> Result<()> {
    fs::create_dir_all(log_dir)?;
    let crash_dir = log_dir.to_path_buf();

    LOG_INIT.call_once(|| {
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("dataforge=info,warn"));
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .with_ansi(false)
            .try_init();

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

/// Writes a crash report file into `log_dir` and returns its path.
pub fn write_crash_report(log_dir: &Path, message: &str) -> Result<PathBuf> {
    fs::create_dir_all(log_dir)?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let path = log_dir.join(format!("crash-{timestamp}.log"));
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(file, "DataForge crash report ({timestamp})")?;
    writeln!(file, "{message}")?;
    Ok(path)
}
