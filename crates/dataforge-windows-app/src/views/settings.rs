//! Settings view and VS Code Theme JSON importer.

use dataforge_gfx::Result;
use dataforge_gfx::context::{FontWeight, RenderSurface, TextAlign, TextFormatSpec, VerticalAlign};
use dataforge_gfx::palette::ThemePalette;
use dataforge_state::theme::{ThemeDefinition, VsCodeThemeImporter};
use dataforge_windows_platform::{AppSettings, ThemePreference};

use crate::shell_layout::{Rect, ShellLayout};

/// Result of importing a VS Code or DataForge theme JSON file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeImportOutcome {
    pub theme_name: String,
    pub slot_count: usize,
}

/// Imports a VS Code or DataForge theme JSON payload using `dataforge-state`'s strict validator.
pub fn import_vscode_theme_bytes(
    bytes: &[u8],
) -> std::result::Result<(ThemeDefinition, ThemeImportOutcome), String> {
    let def = VsCodeThemeImporter::import_bytes(bytes).map_err(|err| err.to_string())?;
    let outcome = ThemeImportOutcome {
        theme_name: def.name.clone(),
        slot_count: def.colors.len(),
    };
    Ok((def, outcome))
}

/// Renders the Settings view inside the content area.
pub fn render_settings_view(
    surface: &mut RenderSurface,
    layout: &ShellLayout,
    palette: &ThemePalette,
    settings: &AppSettings,
    active_theme_name: &str,
    last_import_status: Option<&str>,
) -> Result<()> {
    let r = layout.content;
    surface.fill_rect(to_d2d_rect(r), palette.content.bg)?;

    let title_rect = Rect::new(r.x + 24.0, r.y + 20.0, (r.width - 48.0).max(0.0), 30.0);
    let title_fmt = TextFormatSpec::new("Segoe UI", 16.0)
        .with_weight(FontWeight::SemiBold)
        .with_align(TextAlign::Leading)
        .with_valign(VerticalAlign::Center);
    surface.draw_text(
        "Settings",
        to_d2d_rect(title_rect),
        &title_fmt,
        palette.content.text,
    )?;

    let pref_label = match settings.theme {
        ThemePreference::System => "System",
        ThemePreference::Light => "Light",
        ThemePreference::Dark => "Dark",
        ThemePreference::HighContrast => "High Contrast",
    };

    let rows = [
        format!("Appearance: {pref_label}"),
        format!("Content Theme: {active_theme_name}"),
        format!(
            "Editor Font: {} {:.1}px",
            settings.editor_font.family, settings.editor_font.size
        ),
        format!(
            "Restore Session: {}",
            if settings.restore_session {
                "Enabled"
            } else {
                "Disabled"
            }
        ),
        format!(
            "MCP Server: {}",
            if settings.mcp.enabled {
                "Enabled"
            } else {
                "Disabled"
            }
        ),
        last_import_status
            .map(|s| format!("Theme Import: {s}"))
            .unwrap_or_else(|| "Theme Import: Drop or select a VS Code theme JSON".to_owned()),
    ];

    let row_fmt = TextFormatSpec::new("Segoe UI", 12.5)
        .with_align(TextAlign::Leading)
        .with_valign(VerticalAlign::Center);
    let mut y = title_rect.bottom() + 16.0;
    for row in rows {
        if y + 32.0 > r.bottom() {
            break;
        }
        let card = Rect::new(r.x + 24.0, y, (r.width - 48.0).clamp(0.0, 540.0), 30.0);
        surface.fill_rounded_rect(to_d2d_rect(card), 6.0, palette.content.surface_elevated)?;
        let text_r = Rect::new(
            card.x + 12.0,
            card.y,
            (card.width - 24.0).max(0.0),
            card.height,
        );
        surface.draw_text(&row, to_d2d_rect(text_r), &row_fmt, palette.content.text)?;
        y += 38.0;
    }

    Ok(())
}

use super::to_d2d_rect;
