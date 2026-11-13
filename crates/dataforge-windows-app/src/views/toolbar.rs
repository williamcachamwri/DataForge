//! Top Toolbar view (connection/DB/refresh/apply icons, central
//! `HOST | Engine Version | db | <1ms` pill, and right action icons).

use dataforge_gfx::Result;
use dataforge_gfx::context::{
    FontWeight, RenderSurface, TextAlign, TextFormatSpec, VerticalAlign, point_f, rect_f,
};
use dataforge_gfx::palette::ThemePalette;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use crate::shell_layout::{Rect, ShellLayout};

/// Snapshot of connection metadata shown in the central toolbar status pill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolbarStatusPill {
    pub host: String,
    pub engine_version: String,
    pub database: String,
    pub latency_label: String,
    pub safe_mode: bool,
    pub inspector_open: bool,
    pub ai_open: bool,
}

impl Default for ToolbarStatusPill {
    fn default() -> Self {
        Self {
            host: "localhost".to_owned(),
            engine_version: "Disconnected".to_owned(),
            database: "-".to_owned(),
            latency_label: "<1ms".to_owned(),
            safe_mode: false,
            inspector_open: false,
            ai_open: false,
        }
    }
}

impl ToolbarStatusPill {
    /// Formats the canonical `HOST | Engine Version | db | <1ms` status pill text.
    #[must_use]
    pub fn format_pill_text(&self) -> String {
        format!(
            "{} | {} | {} | {}",
            self.host, self.engine_version, self.database, self.latency_label
        )
    }
}

/// Renders the Top Toolbar using Direct2D.
pub fn render_toolbar(
    surface: &mut RenderSurface,
    layout: &ShellLayout,
    palette: &ThemePalette,
    status: &ToolbarStatusPill,
    has_pending_edits: bool,
) -> Result<()> {
    let r = layout.toolbar;
    surface.fill_rect(to_d2d_rect(r), palette.chrome.toolbar_bg)?;
    surface.draw_line(
        point_f(r.x, r.bottom() - 0.5),
        point_f(r.right(), r.bottom() - 0.5),
        palette.chrome.border,
        1.0,
    )?;

    draw_icon_button(surface, layout.toolbar_connect_btn, palette, "DB", false)?;
    draw_icon_button(surface, layout.toolbar_refresh_btn, palette, "Ref", false)?;
    draw_icon_button(
        surface,
        layout.toolbar_apply_btn,
        palette,
        "App",
        has_pending_edits,
    )?;

    // Central status pill
    let pill = layout.toolbar_status_pill;
    if pill.width > 40.0 {
        surface.fill_rounded_rect(to_d2d_rect(pill), 8.0, palette.content.bg)?;
        surface.draw_rounded_rect(to_d2d_rect(pill), 8.0, palette.chrome.border, 1.0)?;
        let text = status.format_pill_text();
        let fmt = TextFormatSpec::new("Segoe UI", 11.5)
            .with_weight(FontWeight::Medium)
            .with_align(TextAlign::Center)
            .with_valign(VerticalAlign::Center);
        surface.draw_text(&text, to_d2d_rect(pill), &fmt, palette.chrome.text)?;
    }

    draw_icon_button(
        surface,
        layout.toolbar_inspector_btn,
        palette,
        "Row",
        status.inspector_open,
    )?;
    draw_icon_button(
        surface,
        layout.toolbar_ai_btn,
        palette,
        "AI",
        status.ai_open,
    )?;

    Ok(())
}

fn draw_icon_button(
    surface: &mut RenderSurface,
    rect: Rect,
    palette: &ThemePalette,
    label: &str,
    active: bool,
) -> Result<()> {
    if active {
        surface.fill_rounded_rect(to_d2d_rect(rect), 6.0, palette.chrome.hover_bg)?;
        surface.draw_rounded_rect(to_d2d_rect(rect), 6.0, palette.chrome.focus_border, 1.0)?;
    }
    let fmt = TextFormatSpec::new("Segoe UI", 11.0)
        .with_weight(FontWeight::Medium)
        .with_align(TextAlign::Center)
        .with_valign(VerticalAlign::Center);
    surface.draw_text(label, to_d2d_rect(rect), &fmt, palette.chrome.text)?;
    Ok(())
}

const fn to_d2d_rect(r: Rect) -> D2D_RECT_F {
    rect_f(r.x, r.y, r.right(), r.bottom())
}
