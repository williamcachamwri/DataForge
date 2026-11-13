//! Database table structure/schema metadata viewer.

use herogpui::gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, Window, div, px,
};
use herogpui::theme::ActiveTheme;

use super::DataForgeWorkspace;
use super::theme::value_gpui_font;

/// Persistent UI state for the table Structure view.
#[derive(Debug, Default)]
pub struct StructureViewState {
    pub selected_column: Option<usize>,
}

impl StructureViewState {
    /// Initializes Structure view state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) -> Self {
        Self::default()
    }
}

/// Renders the active table's structure columns, types, and constraints.
#[must_use]
pub fn render_structure_view(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let font = value_gpui_font(&ws.controller.settings);
    let col_count = ws.controller.structure_columns.len();
    div()
        .id("workspace-structure-view")
        .flex_1()
        .size_full()
        .flex()
        .flex_col()
        .p(px(12.0))
        .font(font)
        .bg(ws.content_palette.window_background)
        .text_color(colors.foreground)
        .child(format!("Structure ({col_count} columns)"))
        .into_any_element()
}
