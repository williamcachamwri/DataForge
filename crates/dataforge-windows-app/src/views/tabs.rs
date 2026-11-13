//! Pill Tab Strip view (detachable/re-attachable tabs, `+` button, close button).

use dataforge_gfx::Result;
use dataforge_gfx::context::{
    FontWeight, RenderSurface, TextAlign, TextFormatSpec, VerticalAlign, point_f,
};
use dataforge_gfx::palette::ThemePalette;
use dataforge_state::tabs::{TabId, TabItem};

use crate::shell_layout::{Rect, ShellLayout};

/// Renders the Pill Tab Strip using Direct2D.
pub fn render_tab_strip(
    surface: &mut RenderSurface,
    layout: &ShellLayout,
    palette: &ThemePalette,
    tabs: &[TabItem],
    active_tab: Option<TabId>,
) -> Result<()> {
    let r = layout.tab_strip;
    surface.fill_rect(to_d2d_rect(r), palette.chrome.toolbar_bg)?;
    surface.draw_line(
        point_f(r.x, r.bottom() - 0.5),
        point_f(r.right(), r.bottom() - 0.5),
        palette.chrome.border,
        1.0,
    )?;

    for (idx, (pill, close)) in layout.tab_pills.iter().enumerate() {
        let Some(tab) = tabs.get(idx) else {
            break;
        };
        let is_active = active_tab == Some(tab.id);
        let bg = if is_active {
            palette.chrome.tab_active_bg
        } else {
            palette.chrome.tab_inactive_bg
        };
        surface.fill_rounded_rect(to_d2d_rect(*pill), 8.0, bg)?;
        if is_active {
            surface.draw_rounded_rect(to_d2d_rect(*pill), 8.0, palette.chrome.border, 1.0)?;
        }

        let title = if tab.has_unsaved_edits {
            format!("* {}", tab.title)
        } else {
            tab.title.clone()
        };
        let label_rect = Rect::new(
            pill.x + 10.0,
            pill.y,
            (close.x - pill.x - 12.0).max(0.0),
            pill.height,
        );
        let fmt = TextFormatSpec::new("Segoe UI", 12.0)
            .with_weight(if is_active {
                FontWeight::SemiBold
            } else {
                FontWeight::Normal
            })
            .with_align(TextAlign::Leading)
            .with_valign(VerticalAlign::Center);
        let text_color = if is_active {
            palette.chrome.text
        } else {
            palette.chrome.text_muted
        };
        surface.draw_text(&title, to_d2d_rect(label_rect), &fmt, text_color)?;

        let close_fmt = TextFormatSpec::new("Segoe UI", 11.0)
            .with_align(TextAlign::Center)
            .with_valign(VerticalAlign::Center);
        surface.draw_text(
            "x",
            to_d2d_rect(*close),
            &close_fmt,
            palette.chrome.text_muted,
        )?;
    }

    let plus_fmt = TextFormatSpec::new("Segoe UI", 14.0)
        .with_weight(FontWeight::Medium)
        .with_align(TextAlign::Center)
        .with_valign(VerticalAlign::Center);
    surface.draw_text(
        "+",
        to_d2d_rect(layout.tab_new_btn),
        &plus_fmt,
        palette.chrome.text,
    )?;

    Ok(())
}

use super::to_d2d_rect;
