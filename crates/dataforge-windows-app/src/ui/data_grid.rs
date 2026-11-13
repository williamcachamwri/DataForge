//! Virtualized 2D DataGrid view backed by `dataforge-state::data_grid` and
//! [`ContentThemePalette`] colors + `value_font`.

use herogpui::gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, Window, div, px,
};

use super::DataForgeWorkspace;
use super::theme::value_gpui_font;

/// Persistent UI state for the DataGrid (scroll handles, inline editor, column drag).
#[derive(Debug, Default)]
pub struct DataGridViewState {
    pub scroll_row: usize,
}

impl DataGridViewState {
    /// Initializes DataGrid state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) -> Self {
        Self::default()
    }
}

/// Renders the DataGrid table surface (zero fake rows when disconnected/empty).
#[must_use]
pub fn render_data_grid(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    _cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let font = value_gpui_font(&ws.controller.settings);
    let cols = ws.controller.grid_columns.len();
    let rows = ws.controller.grid_rows.len();
    div()
        .id("workspace-data-grid")
        .flex_1()
        .size_full()
        .flex()
        .flex_col()
        .p(px(12.0))
        .font(font)
        .bg(ws.content_palette.window_background)
        .text_color(ws.content_palette.editor_text)
        .child(format!("DataGrid ({cols} columns, {rows} rows)"))
        .into_any_element()
}
