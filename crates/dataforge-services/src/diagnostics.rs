//! Structured `tracing` logging setup, panic-free crash report writer, and `en-US` locale table.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;
use tracing_subscriber::EnvFilter;

use crate::error::ServiceError;

/// Initializes `tracing` subscriber with `RUST_LOG` or `default_filter` (e.g. `"info"`).
///
/// Safe to call multiple times: subsequent calls are ignored if a global subscriber is already set.
pub fn init_logging(default_filter: &str) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .try_init();
}

/// Structured crash report payload written without panicking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashReport {
    pub app_version: String,
    pub os_version: String,
    pub message: String,
    pub location: Option<String>,
    pub backtrace: Option<String>,
}

/// Writes a crash report atomically into `crash_dir` and returns the written file path.
///
/// # Errors
///
/// Returns [`ServiceError::Io`] if creating `crash_dir` or writing the report fails.
pub fn write_crash_report(crash_dir: &Path, report: &CrashReport) -> Result<PathBuf, ServiceError> {
    fs::create_dir_all(crash_dir)?;
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let file_name = format!("dataforge-crash-{timestamp}.txt");
    let target = crash_dir.join(&file_name);
    let part = crash_dir.join(format!("{file_name}.part"));

    let body = format_crash_report(report);
    let mut file = File::create(&part)?;
    file.write_all(body.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&part, &target)?;
    Ok(target)
}

fn format_crash_report(report: &CrashReport) -> String {
    let location = report.location.as_deref().unwrap_or("unknown");
    let backtrace = report.backtrace.as_deref().unwrap_or("(unavailable)");
    format!(
        "DataForge Crash Report\n\
         Timestamp: {}\n\
         Version: {}\n\
         OS: {}\n\
         Location: {}\n\
         Message: {}\n\n\
         Backtrace:\n{}\n",
        Utc::now().to_rfc3339(),
        report.app_version,
        report.os_version,
        location,
        report.message,
        backtrace
    )
}

/// Simple key-value i18n locale table with `en-US` defaults.
#[derive(Debug, Clone)]
pub struct LocaleTable {
    locale: String,
    strings: BTreeMap<&'static str, &'static str>,
}

impl LocaleTable {
    /// Creates the default `en-US` locale table.
    #[must_use]
    pub fn en_us() -> Self {
        let mut strings = BTreeMap::new();
        strings.insert("app.name", "DataForge");
        strings.insert("connection.connect", "Connect");
        strings.insert("connection.disconnect", "Disconnect");
        strings.insert("connection.test", "Test Connection");
        strings.insert("connection.unavailable", "Connection Unavailable");
        strings.insert("editor.run", "Run Query");
        strings.insert("editor.stop", "Stop");
        strings.insert("grid.commit", "Commit Changes");
        strings.insert("grid.rollback", "Discard Changes");
        strings.insert("export.title", "Export Data");
        strings.insert("import.title", "Import Connections");
        strings.insert("settings.title", "Settings");
        Self {
            locale: "en-US".to_owned(),
            strings,
        }
    }

    /// Returns the BCP-47 locale tag (e.g. `"en-US"`).
    #[must_use]
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Looks up a localized string by `key`, falling back to `key` if missing.
    #[must_use]
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        self.strings.get(key).copied().unwrap_or(key)
    }

    /// Returns the number of registered translation keys.
    #[must_use]
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Returns whether the locale table is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }
}

impl Default for LocaleTable {
    fn default() -> Self {
        Self::en_us()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_logging_is_idempotent() {
        init_logging("info");
        init_logging("debug");
    }

    #[test]
    fn write_crash_report_creates_file_atomically() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let report = CrashReport {
            app_version: "0.1.0".to_owned(),
            os_version: "Windows 11".to_owned(),
            message: "test panic".to_owned(),
            location: Some("src/main.rs:10".to_owned()),
            backtrace: Some("frame 0".to_owned()),
        };
        let path = write_crash_report(dir.path(), &report)?;
        assert!(path.exists());
        let contents = fs::read_to_string(&path)?;
        assert!(contents.contains("DataForge Crash Report"));
        assert!(contents.contains("test panic"));
        assert!(contents.contains("src/main.rs:10"));
        Ok(())
    }

    #[test]
    fn locale_table_en_us_provides_defaults_and_fallback() {
        let table = LocaleTable::en_us();
        assert_eq!(table.locale(), "en-US");
        assert!(!table.is_empty());
        assert_eq!(table.get("app.name"), "DataForge");
        assert_eq!(table.get("missing.key"), "missing.key");
    }
}
