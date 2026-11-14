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
        Self::from_pairs(
            "en-US",
            [
                ("app.name", "DataForge"),
                ("connection.connect", "Connect"),
                ("connection.disconnect", "Disconnect"),
                ("connection.test", "Test Connection"),
                ("connection.unavailable", "Connection Unavailable"),
                ("editor.run", "Run Query"),
                ("editor.stop", "Stop"),
                ("grid.commit", "Commit Changes"),
                ("grid.rollback", "Discard Changes"),
                ("export.title", "Export Data"),
                ("import.title", "Import Connections"),
                ("settings.title", "Settings"),
            ],
        )
    }

    /// Creates a localized `LocaleTable` for a supported BCP-47 language tag
    /// (`en-US`, `vi-VN`, `zh-CN`, `ja-JP`, `es-ES`, `fr-FR`), falling back to `en-US`.
    #[must_use]
    pub fn for_locale_tag(tag: &str) -> Self {
        let normalized = tag.trim().to_ascii_lowercase().replace('_', "-");
        if normalized.starts_with("vi") {
            Self::from_pairs(
                "vi-VN",
                [
                    ("app.name", "DataForge"),
                    ("connection.connect", "Kết nối"),
                    ("connection.disconnect", "Ngắt kết nối"),
                    ("connection.test", "Kiểm tra kết nối"),
                    ("connection.unavailable", "Kết nối không khả dụng"),
                    ("editor.run", "Chạy truy vấn"),
                    ("editor.stop", "Dừng"),
                    ("grid.commit", "Lưu thay đổi (Commit)"),
                    ("grid.rollback", "Hủy thay đổi (Rollback)"),
                    ("export.title", "Xuất dữ liệu"),
                    ("import.title", "Nhập kết nối"),
                    ("settings.title", "Cài đặt"),
                ],
            )
        } else if normalized.starts_with("zh") {
            Self::from_pairs(
                "zh-CN",
                [
                    ("app.name", "DataForge"),
                    ("connection.connect", "连接"),
                    ("connection.disconnect", "断开连接"),
                    ("connection.test", "测试连接"),
                    ("connection.unavailable", "连接不可用"),
                    ("editor.run", "运行查询"),
                    ("editor.stop", "停止"),
                    ("grid.commit", "提交更改"),
                    ("grid.rollback", "放弃更改"),
                    ("export.title", "导出数据"),
                    ("import.title", "导入连接"),
                    ("settings.title", "设置"),
                ],
            )
        } else if normalized.starts_with("ja") {
            Self::from_pairs(
                "ja-JP",
                [
                    ("app.name", "DataForge"),
                    ("connection.connect", "接続"),
                    ("connection.disconnect", "切断"),
                    ("connection.test", "接続テスト"),
                    ("connection.unavailable", "接続を利用できません"),
                    ("editor.run", "クエリを実行"),
                    ("editor.stop", "停止"),
                    ("grid.commit", "変更をコミット"),
                    ("grid.rollback", "変更を破棄"),
                    ("export.title", "データをエクスポート"),
                    ("import.title", "接続をインポート"),
                    ("settings.title", "設定"),
                ],
            )
        } else if normalized.starts_with("es") {
            Self::from_pairs(
                "es-ES",
                [
                    ("app.name", "DataForge"),
                    ("connection.connect", "Conectar"),
                    ("connection.disconnect", "Desconectar"),
                    ("connection.test", "Probar conexión"),
                    ("connection.unavailable", "Conexión no disponible"),
                    ("editor.run", "Ejecutar consulta"),
                    ("editor.stop", "Detener"),
                    ("grid.commit", "Confirmar cambios"),
                    ("grid.rollback", "Descartar cambios"),
                    ("export.title", "Exportar datos"),
                    ("import.title", "Importar conexiones"),
                    ("settings.title", "Configuración"),
                ],
            )
        } else if normalized.starts_with("fr") {
            Self::from_pairs(
                "fr-FR",
                [
                    ("app.name", "DataForge"),
                    ("connection.connect", "Connecter"),
                    ("connection.disconnect", "Déconnecter"),
                    ("connection.test", "Tester la connexion"),
                    ("connection.unavailable", "Connexion indisponible"),
                    ("editor.run", "Exécuter la requête"),
                    ("editor.stop", "Arrêter"),
                    ("grid.commit", "Valider les modifications"),
                    ("grid.rollback", "Annuler les modifications"),
                    ("export.title", "Exporter les données"),
                    ("import.title", "Importer des connexions"),
                    ("settings.title", "Paramètres"),
                ],
            )
        } else {
            Self::en_us()
        }
    }

    fn from_pairs<const N: usize>(locale: &str, pairs: [(&'static str, &'static str); N]) -> Self {
        let mut strings = BTreeMap::new();
        for (k, v) in pairs {
            strings.insert(k, v);
        }
        Self {
            locale: locale.to_owned(),
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

    #[test]
    fn locale_table_supports_all_six_locales() {
        let en = LocaleTable::en_us();
        for tag in ["en-US", "vi-VN", "zh-CN", "ja-JP", "es-ES", "fr-FR"] {
            let table = LocaleTable::for_locale_tag(tag);
            assert_eq!(table.locale(), tag);
            assert_eq!(table.len(), en.len());
            assert_eq!(table.get("app.name"), "DataForge");
            assert!(!table.get("grid.commit").is_empty());
            assert!(!table.get("settings.title").is_empty());
        }
    }
}
