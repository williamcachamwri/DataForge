//! Main Content Area rendering (`DataGridRenderer`, Structure view,
//! `SqlEditorRenderer` with Vim mode & autocomplete, and `ConnectionUnavailableView`).

use dataforge_gfx::Result;
use dataforge_gfx::context::{
    FontWeight, RenderSurface, TextAlign, TextFormatSpec, VerticalAlign, point_f,
};
use dataforge_gfx::editor_renderer::{
    AutocompletePopup, CursorMode, EditorCursor, EditorLayout, EditorLine, EditorRenderState,
    EditorSelection, SqlEditorRenderer,
};
use dataforge_gfx::grid_renderer::{
    DataGridRenderer, GridColumn, GridLayout, GridRow, GridSelection,
};
use dataforge_gfx::palette::ThemePalette;
use dataforge_state::connection_window::ConnectionUnavailableView;
use windows_numerics::Matrix3x2;

use crate::shell_layout::{Rect, ShellLayout};

/// Column schema row displayed in the Structure view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureColumnRow {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default_expr: Option<String>,
    pub is_primary_key: bool,
}

/// Renders the virtualized `DataGridRenderer` inside `layout.content`.
#[expect(
    clippy::too_many_arguments,
    reason = "Direct2D grid pass forwards renderer, columns, rows, selection, and frozen count"
)]
pub fn render_data_grid(
    surface: &mut RenderSurface,
    layout: &ShellLayout,
    palette: &ThemePalette,
    renderer: &DataGridRenderer,
    columns: &[GridColumn],
    rows: &[GridRow],
    selection: &GridSelection,
    frozen_columns: usize,
) -> Result<()> {
    let r = layout.content;
    if r.width <= 0.0 || r.height <= 0.0 {
        return Ok(());
    }
    let grid_layout = GridLayout {
        viewport_width: r.width,
        viewport_height: r.height,
        frozen_columns,
        ..GridLayout::default()
    };
    surface.push_clip(to_d2d_rect(r));
    surface.set_transform(&Matrix3x2::translation(r.x, r.y));
    let res = renderer.draw(surface, palette, &grid_layout, columns, rows, selection);
    surface.reset_transform();
    surface.pop_clip();
    res
}

/// Renders the table Structure schema view inside `layout.content`.
pub fn render_structure_view(
    surface: &mut RenderSurface,
    layout: &ShellLayout,
    palette: &ThemePalette,
    table_name: &str,
    columns: &[StructureColumnRow],
) -> Result<()> {
    let r = layout.content;
    surface.fill_rect(to_d2d_rect(r), palette.content.bg)?;

    let title_rect = Rect::new(r.x + 16.0, r.y + 12.0, (r.width - 32.0).max(0.0), 26.0);
    let title_fmt = TextFormatSpec::new("Segoe UI", 14.0)
        .with_weight(FontWeight::SemiBold)
        .with_align(TextAlign::Leading)
        .with_valign(VerticalAlign::Center);
    let title = format!("Structure: {table_name}");
    surface.draw_text(
        &title,
        to_d2d_rect(title_rect),
        &title_fmt,
        palette.content.text,
    )?;

    let header_y = title_rect.bottom() + 8.0;
    let header_rect = Rect::new(r.x + 16.0, header_y, (r.width - 32.0).max(0.0), 26.0);
    surface.fill_rect(to_d2d_rect(header_rect), palette.grid.header_bg)?;

    let row_fmt = TextFormatSpec::new("Segoe UI", 12.0)
        .with_align(TextAlign::Leading)
        .with_valign(VerticalAlign::Center);
    surface.draw_text(
        "Column | Type | Nullable | Default | Key",
        to_d2d_rect(Rect::new(
            header_rect.x + 8.0,
            header_rect.y,
            header_rect.width,
            26.0,
        )),
        &row_fmt,
        palette.grid.header_text,
    )?;

    let mut y = header_rect.bottom() + 2.0;
    for col in columns {
        if y + 26.0 > r.bottom() {
            break;
        }
        let row_rect = Rect::new(r.x + 16.0, y, header_rect.width, 24.0);
        surface.draw_line(
            point_f(row_rect.x, row_rect.bottom()),
            point_f(row_rect.right(), row_rect.bottom()),
            palette.content.border,
            1.0,
        )?;
        let line = format!(
            "{} | {} | {} | {} | {}",
            col.name,
            col.data_type,
            if col.nullable { "NULL" } else { "NOT NULL" },
            col.default_expr.as_deref().unwrap_or("-"),
            if col.is_primary_key { "PK" } else { "" }
        );
        surface.draw_text(
            &line,
            to_d2d_rect(Rect::new(
                row_rect.x + 8.0,
                row_rect.y,
                row_rect.width,
                24.0,
            )),
            &row_fmt,
            palette.content.text,
        )?;
        y += 26.0;
    }

    Ok(())
}

/// Renders the `SqlEditorRenderer` (with Vim mode and autocomplete) inside `layout.content`.
#[expect(
    clippy::too_many_arguments,
    reason = "Direct2D editor pass forwards viewport, lines, cursors, selections, and optional popup"
)]
pub fn render_sql_editor(
    surface: &mut RenderSurface,
    layout: &ShellLayout,
    palette: &ThemePalette,
    renderer: &mut SqlEditorRenderer,
    lines: &[EditorLine],
    cursors: &[EditorCursor],
    selections: &[EditorSelection],
    cursor_mode: CursorMode,
    autocomplete: Option<&AutocompletePopup>,
) -> Result<()> {
    let r = layout.content;
    if r.width <= 0.0 || r.height <= 0.0 {
        return Ok(());
    }
    let editor_layout = EditorLayout {
        viewport_width: r.width,
        viewport_height: r.height,
        ..EditorLayout::default()
    };
    let state = EditorRenderState {
        palette,
        layout: &editor_layout,
        lines,
        cursors,
        selections,
        cursor_mode,
        popup: autocomplete,
    };

    surface.push_clip(to_d2d_rect(r));
    surface.set_transform(&Matrix3x2::translation(r.x, r.y));
    let res = renderer.draw(surface, &state);
    surface.reset_transform();
    surface.pop_clip();
    res
}

/// Renders the inline `ConnectionUnavailableView` error card (never a modal alert).
pub fn render_unavailable_view(
    surface: &mut RenderSurface,
    layout: &ShellLayout,
    palette: &ThemePalette,
    unavailable: &ConnectionUnavailableView,
    retry_label: &str,
) -> Result<()> {
    let r = layout.content;
    surface.fill_rect(to_d2d_rect(r), palette.content.bg)?;

    let card_w = 440.0_f32.min((r.width - 32.0).max(0.0));
    let card_h = 160.0_f32.min((r.height - 32.0).max(0.0));
    let card = Rect::new(
        r.x + (r.width - card_w) * 0.5,
        r.y + (r.height - card_h) * 0.5,
        card_w,
        card_h,
    );
    surface.fill_rounded_rect(to_d2d_rect(card), 10.0, palette.content.surface_elevated)?;
    surface.draw_rounded_rect(to_d2d_rect(card), 10.0, palette.content.border, 1.0)?;

    let title_rect = Rect::new(
        card.x + 20.0,
        card.y + 18.0,
        (card.width - 40.0).max(0.0),
        26.0,
    );
    let title_fmt = TextFormatSpec::new("Segoe UI", 14.5)
        .with_weight(FontWeight::SemiBold)
        .with_align(TextAlign::Leading)
        .with_valign(VerticalAlign::Center);
    surface.draw_text(
        &unavailable.summary,
        to_d2d_rect(title_rect),
        &title_fmt,
        palette.content.text,
    )?;

    if let Some(detail) = &unavailable.detail {
        let detail_rect = Rect::new(
            card.x + 20.0,
            title_rect.bottom() + 8.0,
            title_rect.width,
            52.0,
        );
        let detail_fmt = TextFormatSpec::new("Segoe UI", 12.0)
            .with_align(TextAlign::Leading)
            .with_valign(VerticalAlign::Top);
        surface.draw_text(
            detail,
            to_d2d_rect(detail_rect),
            &detail_fmt,
            palette.content.text_muted,
        )?;
    }

    if unavailable.can_retry {
        let btn = Rect::new(
            card.x + 20.0,
            (card.bottom() - 46.0).max(card.y),
            96.0,
            28.0,
        );
        surface.fill_rounded_rect(to_d2d_rect(btn), 6.0, palette.content.accent)?;
        let btn_fmt = TextFormatSpec::new("Segoe UI", 12.0)
            .with_weight(FontWeight::SemiBold)
            .with_align(TextAlign::Center)
            .with_valign(VerticalAlign::Center);
        surface.draw_text(
            retry_label,
            to_d2d_rect(btn),
            &btn_fmt,
            palette.content.on_accent,
        )?;
    }

    Ok(())
}

/// Renders an inline connecting state inside `layout.content`.
pub fn render_connecting_view(
    surface: &mut RenderSurface,
    layout: &ShellLayout,
    palette: &ThemePalette,
    connection_name: &str,
) -> Result<()> {
    let r = layout.content;
    surface.fill_rect(to_d2d_rect(r), palette.content.bg)?;
    let msg = format!("Connecting to {connection_name}...");
    let fmt = TextFormatSpec::new("Segoe UI", 14.0)
        .with_weight(FontWeight::Medium)
        .with_align(TextAlign::Center)
        .with_valign(VerticalAlign::Center);
    surface.draw_text(&msg, to_d2d_rect(r), &fmt, palette.content.text_muted)?;
    Ok(())
}

use super::to_d2d_rect;
