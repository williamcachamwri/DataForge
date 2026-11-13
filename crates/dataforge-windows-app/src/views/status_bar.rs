//! Bottom Status Bar view (`Data`/`Structure` segmented toggle, selection count,
//! `Columns`, `Filters`, pagination `1 of N`, and settings gear).

use dataforge_gfx::Result;
use dataforge_gfx::context::{
    FontWeight, RenderSurface, TextAlign, TextFormatSpec, VerticalAlign, point_f,
};
use dataforge_gfx::palette::ThemePalette;

use crate::shell_layout::{Rect, ShellLayout};

/// Active sub-mode for a table tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableViewMode {
    #[default]
    Data,
    Structure,
}

/// Snapshot of state rendered in the bottom status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusBarSnapshot {
    pub view_mode: TableViewMode,
    pub selection_count: usize,
    pub current_page: usize,
    pub total_pages: usize,
    pub active_filter_count: usize,
}

impl Default for StatusBarSnapshot {
    fn default() -> Self {
        Self {
            view_mode: TableViewMode::Data,
            selection_count: 0,
            current_page: 1,
            total_pages: 1,
            active_filter_count: 0,
        }
    }
}

/// Renders the Bottom Status Bar using Direct2D.
pub fn render_status_bar(
    surface: &mut RenderSurface,
    layout: &ShellLayout,
    palette: &ThemePalette,
    snapshot: &StatusBarSnapshot,
) -> Result<()> {
    let r = layout.status_bar;
    surface.fill_rect(to_d2d_rect(r), palette.chrome.status_bar_bg)?;
    surface.draw_line(
        point_f(r.x, r.y + 0.5),
        point_f(r.right(), r.y + 0.5),
        palette.chrome.border,
        1.0,
    )?;

    draw_segment_button(
        surface,
        layout.status_data_btn,
        palette,
        "Data",
        snapshot.view_mode == TableViewMode::Data,
    )?;
    draw_segment_button(
        surface,
        layout.status_structure_btn,
        palette,
        "Structure",
        snapshot.view_mode == TableViewMode::Structure,
    )?;
    draw_segment_button(
        surface,
        layout.status_columns_btn,
        palette,
        "Columns",
        false,
    )?;

    let filter_label = if snapshot.active_filter_count > 0 {
        format!("Filters ({})", snapshot.active_filter_count)
    } else {
        "Filters".to_owned()
    };
    draw_segment_button(
        surface,
        layout.status_filters_btn,
        palette,
        &filter_label,
        snapshot.active_filter_count > 0,
    )?;

    // Selection count
    if snapshot.selection_count > 0 {
        let sel_rect = Rect::new(
            layout.status_filters_btn.right() + 12.0,
            r.y,
            140.0,
            r.height,
        );
        let sel_text = format!("{} selected", snapshot.selection_count);
        let fmt = TextFormatSpec::new("Segoe UI", 11.0)
            .with_align(TextAlign::Leading)
            .with_valign(VerticalAlign::Center);
        surface.draw_text(
            &sel_text,
            to_d2d_rect(sel_rect),
            &fmt,
            palette.chrome.text_muted,
        )?;
    }

    // Pagination (`< 1 of N >`)
    draw_segment_button(surface, layout.status_prev_page_btn, palette, "<", false)?;
    let page_rect = Rect::new(
        layout.status_prev_page_btn.right(),
        r.y,
        (layout.status_next_page_btn.x - layout.status_prev_page_btn.right()).max(0.0),
        r.height,
    );
    let page_text = format!(
        "{} of {}",
        snapshot.current_page,
        snapshot.total_pages.max(1)
    );
    let page_fmt = TextFormatSpec::new("Segoe UI", 11.0)
        .with_align(TextAlign::Center)
        .with_valign(VerticalAlign::Center);
    surface.draw_text(
        &page_text,
        to_d2d_rect(page_rect),
        &page_fmt,
        palette.chrome.text,
    )?;
    draw_segment_button(surface, layout.status_next_page_btn, palette, ">", false)?;
    draw_segment_button(surface, layout.status_settings_btn, palette, "Cfg", false)?;

    Ok(())
}

fn draw_segment_button(
    surface: &mut RenderSurface,
    rect: Rect,
    palette: &ThemePalette,
    label: &str,
    active: bool,
) -> Result<()> {
    if active {
        surface.fill_rounded_rect(to_d2d_rect(rect), 4.0, palette.chrome.tab_active_bg)?;
    }
    let fmt = TextFormatSpec::new("Segoe UI", 11.0)
        .with_weight(if active {
            FontWeight::SemiBold
        } else {
            FontWeight::Normal
        })
        .with_align(TextAlign::Center)
        .with_valign(VerticalAlign::Center);
    let color = if active {
        palette.chrome.text
    } else {
        palette.chrome.text_muted
    };
    surface.draw_text(label, to_d2d_rect(rect), &fmt, color)?;
    Ok(())
}

use super::to_d2d_rect;
