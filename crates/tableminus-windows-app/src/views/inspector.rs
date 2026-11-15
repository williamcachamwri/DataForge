//! Row Inspector right drawer view.

use tableminus_gfx::Result;
use tableminus_gfx::context::{
    FontWeight, RenderSurface, TextAlign, TextFormatSpec, VerticalAlign, point_f,
};
use tableminus_gfx::palette::ThemePalette;

use crate::shell_layout::{Rect, ShellLayout};

/// Key-value field displayed in the Row Inspector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectorField {
    pub column: String,
    pub data_type: String,
    pub value: String,
    pub is_null: bool,
}

/// Renders the Row Inspector drawer using Direct2D.
pub fn render_inspector(
    surface: &mut RenderSurface,
    layout: &ShellLayout,
    palette: &ThemePalette,
    title: &str,
    empty_label: &str,
    fields: &[InspectorField],
) -> Result<()> {
    let r = layout.inspector;
    if r.width <= 0.0 || r.height <= 0.0 {
        return Ok(());
    }

    surface.fill_rect(to_d2d_rect(r), palette.chrome.sidebar_bg)?;
    surface.draw_line(
        point_f(r.x + 0.5, r.y),
        point_f(r.x + 0.5, r.bottom()),
        palette.chrome.border,
        1.0,
    )?;

    let header = Rect::new(r.x + 12.0, r.y + 8.0, (r.width - 24.0).max(0.0), 28.0);
    let header_fmt = TextFormatSpec::new("Segoe UI", 13.0)
        .with_weight(FontWeight::SemiBold)
        .with_align(TextAlign::Leading)
        .with_valign(VerticalAlign::Center);
    surface.draw_text(title, to_d2d_rect(header), &header_fmt, palette.chrome.text)?;

    if fields.is_empty() {
        let empty_rect = Rect::new(r.x + 12.0, header.bottom() + 12.0, header.width, 24.0);
        let fmt = TextFormatSpec::new("Segoe UI", 12.0)
            .with_align(TextAlign::Leading)
            .with_valign(VerticalAlign::Center);
        surface.draw_text(
            empty_label,
            to_d2d_rect(empty_rect),
            &fmt,
            palette.chrome.text_muted,
        )?;
        return Ok(());
    }

    let mut y = header.bottom() + 8.0;
    for field in fields {
        if y + 48.0 > r.bottom() {
            break;
        }
        let key_rect = Rect::new(r.x + 12.0, y, header.width, 18.0);
        let key_label = format!("{} ({})", field.column, field.data_type);
        let key_fmt = TextFormatSpec::new("Segoe UI", 11.0)
            .with_weight(FontWeight::Medium)
            .with_align(TextAlign::Leading)
            .with_valign(VerticalAlign::Center);
        surface.draw_text(
            &key_label,
            to_d2d_rect(key_rect),
            &key_fmt,
            palette.chrome.text_muted,
        )?;

        let val_rect = Rect::new(r.x + 12.0, y + 20.0, header.width, 22.0);
        surface.fill_rounded_rect(to_d2d_rect(val_rect), 4.0, palette.content.bg)?;
        let val_text = if field.is_null { "NULL" } else { &field.value };
        let val_color = if field.is_null {
            palette.grid.null_text
        } else {
            palette.content.text
        };
        let val_fmt = TextFormatSpec::new("Consolas", 11.5)
            .with_align(TextAlign::Leading)
            .with_valign(VerticalAlign::Center);
        let val_text_rect = Rect::new(
            val_rect.x + 6.0,
            val_rect.y,
            (val_rect.width - 12.0).max(0.0),
            val_rect.height,
        );
        surface.draw_text(val_text, to_d2d_rect(val_text_rect), &val_fmt, val_color)?;
        y += 50.0;
    }

    Ok(())
}

use super::to_d2d_rect;
