//! Object Sidebar view (`Tables`, `Views`, `Functions`, filter box, list/favorites toggle).

use dataforge_gfx::Result;
use dataforge_gfx::context::{
    FontWeight, RenderSurface, TextAlign, TextFormatSpec, VerticalAlign, point_f,
};
use dataforge_gfx::palette::ThemePalette;

use crate::shell_layout::{Rect, ShellLayout};

/// Category of database object in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarSectionKind {
    Tables,
    Views,
    Functions,
}

/// A flattened row displayed in the Object Sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarRow {
    SectionHeader {
        kind: SidebarSectionKind,
        title: String,
        expanded: bool,
        count: usize,
    },
    ObjectItem {
        kind: SidebarSectionKind,
        name: String,
        is_favorite: bool,
        selected: bool,
    },
}

/// Renders the Object Sidebar using Direct2D.
pub fn render_sidebar(
    surface: &mut RenderSurface,
    layout: &ShellLayout,
    palette: &ThemePalette,
    filter_text: &str,
    filter_placeholder: &str,
    favorites_only: bool,
    rows: &[SidebarRow],
) -> Result<()> {
    let r = layout.sidebar;
    if r.width <= 0.0 || r.height <= 0.0 {
        return Ok(());
    }

    surface.fill_rect(to_d2d_rect(r), palette.chrome.sidebar_bg)?;
    surface.draw_line(
        point_f(r.right() - 0.5, r.y),
        point_f(r.right() - 0.5, r.bottom()),
        palette.chrome.border,
        1.0,
    )?;

    // Filter box
    let fb = layout.sidebar_filter;
    surface.fill_rounded_rect(to_d2d_rect(fb), 6.0, palette.content.bg)?;
    surface.draw_rounded_rect(to_d2d_rect(fb), 6.0, palette.chrome.border, 1.0)?;
    let (display_filter, filter_color) = if filter_text.is_empty() {
        (filter_placeholder, palette.chrome.text_muted)
    } else {
        (filter_text, palette.chrome.text)
    };
    let filter_rect = Rect::new(fb.x + 8.0, fb.y, (fb.width - 16.0).max(0.0), fb.height);
    let filter_fmt = TextFormatSpec::new("Segoe UI", 12.0)
        .with_align(TextAlign::Leading)
        .with_valign(VerticalAlign::Center);
    surface.draw_text(
        display_filter,
        to_d2d_rect(filter_rect),
        &filter_fmt,
        filter_color,
    )?;

    // List / Favorites segmented toggle
    let tg = layout.sidebar_mode_toggle;
    surface.fill_rounded_rect(to_d2d_rect(tg), 6.0, palette.chrome.tab_inactive_bg)?;
    let half_w = tg.width * 0.5;
    let active_rect = if favorites_only {
        Rect::new(
            tg.x + half_w + 2.0,
            tg.y + 2.0,
            (half_w - 4.0).max(0.0),
            (tg.height - 4.0).max(0.0),
        )
    } else {
        Rect::new(
            tg.x + 2.0,
            tg.y + 2.0,
            (half_w - 4.0).max(0.0),
            (tg.height - 4.0).max(0.0),
        )
    };
    surface.fill_rounded_rect(to_d2d_rect(active_rect), 4.0, palette.chrome.tab_active_bg)?;
    let seg_fmt = TextFormatSpec::new("Segoe UI", 11.0)
        .with_weight(FontWeight::Medium)
        .with_align(TextAlign::Center)
        .with_valign(VerticalAlign::Center);
    let left_half = Rect::new(tg.x, tg.y, half_w, tg.height);
    let right_half = Rect::new(tg.x + half_w, tg.y, half_w, tg.height);
    surface.draw_text("All", to_d2d_rect(left_half), &seg_fmt, palette.chrome.text)?;
    surface.draw_text(
        "Favorites",
        to_d2d_rect(right_half),
        &seg_fmt,
        palette.chrome.text,
    )?;

    // Section headers and object rows
    for (idx, row_rect) in layout.sidebar_rows.iter().enumerate() {
        let Some(row) = rows.get(idx) else {
            break;
        };
        draw_sidebar_row(surface, *row_rect, palette, row)?;
    }

    Ok(())
}

fn draw_sidebar_row(
    surface: &mut RenderSurface,
    rect: Rect,
    palette: &ThemePalette,
    row: &SidebarRow,
) -> Result<()> {
    match row {
        SidebarRow::SectionHeader {
            title,
            expanded,
            count,
            ..
        } => {
            let prefix = if *expanded { "v" } else { ">" };
            let label = format!("{prefix} {title} ({count})");
            let text_rect = Rect::new(
                rect.x + 6.0,
                rect.y,
                (rect.width - 12.0).max(0.0),
                rect.height,
            );
            let fmt = TextFormatSpec::new("Segoe UI", 11.0)
                .with_weight(FontWeight::SemiBold)
                .with_align(TextAlign::Leading)
                .with_valign(VerticalAlign::Center);
            surface.draw_text(
                &label,
                to_d2d_rect(text_rect),
                &fmt,
                palette.chrome.text_muted,
            )?;
        }
        SidebarRow::ObjectItem {
            name,
            is_favorite,
            selected,
            ..
        } => {
            if *selected {
                surface.fill_rounded_rect(to_d2d_rect(rect), 4.0, palette.chrome.hover_bg)?;
            }
            let prefix = if *is_favorite { "* " } else { "  " };
            let label = format!("{prefix}{name}");
            let text_rect = Rect::new(
                rect.x + 12.0,
                rect.y,
                (rect.width - 18.0).max(0.0),
                rect.height,
            );
            let fmt = TextFormatSpec::new("Segoe UI", 12.0)
                .with_weight(if *selected {
                    FontWeight::Medium
                } else {
                    FontWeight::Normal
                })
                .with_align(TextAlign::Leading)
                .with_valign(VerticalAlign::Center);
            surface.draw_text(&label, to_d2d_rect(text_rect), &fmt, palette.chrome.text)?;
        }
    }
    Ok(())
}

use super::to_d2d_rect;
