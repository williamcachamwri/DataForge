//! Connections Library and Connection Form (SSH/TLS/SSPI, URL parser, foreign import).

use dataforge_gfx::Result;
use dataforge_gfx::context::{FontWeight, RenderSurface, TextAlign, TextFormatSpec, VerticalAlign};
use dataforge_gfx::glyphs::EngineKind;
use dataforge_gfx::palette::ThemePalette;
use dataforge_state::library::{
    ConnectionsLibraryModel, ForeignImportReport, LibraryConnection, connection_from_url,
    parse_connection_url,
};
use tablepro_foreign_import::CollectedImport;

use crate::shell_layout::{Rect, ShellLayout};

/// TLS verification mode for a connection form draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TlsMode {
    #[default]
    Disable,
    Prefer,
    Require,
    VerifyCa,
    VerifyFull,
}

/// Windows authentication / SSPI mode for SQL Server and PostgreSQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuthMode {
    #[default]
    Password,
    SspiIntegrated,
    Certificate,
}

/// Editable draft for the Connection Form (supporting SSH, TLS, SSPI, and URL import).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionFormDraft {
    pub name: String,
    pub engine: EngineKind,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub auth_mode: AuthMode,
    pub tls_mode: TlsMode,
    pub ssh_enabled: bool,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_user: String,
    pub ssh_key_path: String,
    pub url_input: String,
    pub last_error: Option<String>,
}

impl Default for ConnectionFormDraft {
    fn default() -> Self {
        Self {
            name: String::new(),
            engine: EngineKind::Postgresql,
            host: "localhost".to_owned(),
            port: 5432,
            database: "postgres".to_owned(),
            username: "postgres".to_owned(),
            auth_mode: AuthMode::Password,
            tls_mode: TlsMode::Prefer,
            ssh_enabled: false,
            ssh_host: String::new(),
            ssh_port: 22,
            ssh_user: String::new(),
            ssh_key_path: String::new(),
            url_input: String::new(),
            last_error: None,
        }
    }
}

impl ConnectionFormDraft {
    /// Parses a connection URL (`postgres://`, `mysql://`, `sqlite://`, etc.) into this draft.
    pub fn apply_url(&mut self, raw_url: &str) -> std::result::Result<LibraryConnection, String> {
        self.url_input = raw_url.to_owned();
        let parsed = parse_connection_url(raw_url).map_err(|err| err.to_string())?;
        let conn = connection_from_url(&parsed, None);
        self.name = conn.name.clone();
        self.host = conn.host.clone();
        self.port = conn.port.unwrap_or(5432);
        self.database = conn.database.clone();
        self.username = conn.username.clone();
        self.engine = engine_from_driver_id(&conn.driver_id);
        self.last_error = None;
        Ok(conn)
    }
}

/// Maps a `tablepro-core` driver id string to a [`dataforge_gfx::glyphs::EngineKind`].
#[must_use]
pub fn engine_from_driver_id(driver_id: &str) -> EngineKind {
    let lower = driver_id.to_ascii_lowercase();
    if lower.contains("postgres") {
        EngineKind::Postgresql
    } else if lower.contains("mariadb") {
        EngineKind::Mariadb
    } else if lower.contains("mysql") {
        EngineKind::Mysql
    } else if lower.contains("sqlite") {
        EngineKind::Sqlite
    } else if lower.contains("duckdb") {
        EngineKind::Duckdb
    } else if lower.contains("mssql") || lower.contains("sqlserver") {
        EngineKind::Mssql
    } else if lower.contains("clickhouse") {
        EngineKind::Clickhouse
    } else if lower.contains("scylla") {
        EngineKind::Scylladb
    } else if lower.contains("cassandra") {
        EngineKind::Cassandra
    } else if lower.contains("mongo") {
        EngineKind::Mongodb
    } else if lower.contains("redis") {
        EngineKind::Redis
    } else {
        EngineKind::Postgresql
    }
}

/// Imports connections discovered from foreign apps (DBeaver, TablePlus, HeidiSQL, etc.).
pub fn import_foreign_connections(
    library: &mut ConnectionsLibraryModel,
    collected: &CollectedImport,
) -> ForeignImportReport {
    library.import_foreign(collected)
}

/// Renders the Connections Library and Connection Form view in the content area.
pub fn render_library_view(
    surface: &mut RenderSurface,
    layout: &ShellLayout,
    palette: &ThemePalette,
    library: &ConnectionsLibraryModel,
    form: &ConnectionFormDraft,
) -> Result<()> {
    let r = layout.content;
    surface.fill_rect(to_d2d_rect(r), palette.content.bg)?;

    let left_w = (r.width * 0.45).max(220.0).min(r.width);
    let list_rect = Rect::new(
        r.x + 16.0,
        r.y + 16.0,
        (left_w - 24.0).max(0.0),
        (r.height - 32.0).max(0.0),
    );
    let form_rect = Rect::new(
        r.x + left_w + 8.0,
        r.y + 16.0,
        (r.width - left_w - 24.0).max(0.0),
        (r.height - 32.0).max(0.0),
    );

    // Left: Connections Library outline
    let title_fmt = TextFormatSpec::new("Segoe UI", 15.0)
        .with_weight(FontWeight::SemiBold)
        .with_align(TextAlign::Leading)
        .with_valign(VerticalAlign::Center);
    let header_rect = Rect::new(list_rect.x, list_rect.y, list_rect.width, 28.0);
    surface.draw_text(
        "Connections Library",
        to_d2d_rect(header_rect),
        &title_fmt,
        palette.content.text,
    )?;

    let row_fmt = TextFormatSpec::new("Segoe UI", 12.0)
        .with_align(TextAlign::Leading)
        .with_valign(VerticalAlign::Center);
    let mut y = header_rect.bottom() + 8.0;
    for row in library.outline() {
        if y + 28.0 > list_rect.bottom() {
            break;
        }
        let item_rect = Rect::new(
            list_rect.x + (row.depth as f32 * 14.0),
            y,
            list_rect.width,
            26.0,
        );
        let color = if row.is_group {
            palette.content.text_muted
        } else {
            palette.content.text
        };
        surface.draw_text(&row.label, to_d2d_rect(item_rect), &row_fmt, color)?;
        y += 28.0;
    }

    // Right: Connection Form card
    if form_rect.width > 120.0 {
        surface.fill_rounded_rect(
            to_d2d_rect(form_rect),
            8.0,
            palette.content.surface_elevated,
        )?;
        surface.draw_rounded_rect(to_d2d_rect(form_rect), 8.0, palette.content.border, 1.0)?;
        let fh = Rect::new(
            form_rect.x + 16.0,
            form_rect.y + 12.0,
            (form_rect.width - 32.0).max(0.0),
            28.0,
        );
        let form_title = format!("{} Connection", form.engine.display_name());
        surface.draw_text(
            &form_title,
            to_d2d_rect(fh),
            &title_fmt,
            palette.content.text,
        )?;

        let summary_lines = [
            format!("Host: {}:{}", form.host, form.port),
            format!("Database: {}", form.database),
            format!("User: {} (Auth: {:?})", form.username, form.auth_mode),
            format!(
                "TLS: {:?} | SSH Tunnel: {}",
                form.tls_mode,
                if form.ssh_enabled { "On" } else { "Off" }
            ),
        ];
        let mut fy = fh.bottom() + 12.0;
        for line in summary_lines {
            if fy + 24.0 > form_rect.bottom() {
                break;
            }
            let lr = Rect::new(fh.x, fy, fh.width, 22.0);
            surface.draw_text(&line, to_d2d_rect(lr), &row_fmt, palette.content.text)?;
            fy += 26.0;
        }
    }

    Ok(())
}

use super::to_d2d_rect;
