//! Virtualized syntax-highlighted SQL editor view backed by `dataforge-state::editor`
//! and [`crate::ui::syntax_bridge`].

use dataforge_state::editor::IncrementalLineHighlighter;
use herogpui::gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, Window, div, px,
};
use tablepro_core::sql_syntax::SqlGrammar;

use super::DataForgeWorkspace;
use super::theme::editor_gpui_font;

/// Persistent UI state for the SQL editor surface.
#[derive(Debug)]
pub struct SqlEditorViewState {
    pub highlighter: IncrementalLineHighlighter,
}

impl SqlEditorViewState {
    /// Initializes SQL editor rendering state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) -> Self {
        Self {
            highlighter: IncrementalLineHighlighter::new(SqlGrammar::PostgreSql),
        }
    }
}

/// Renders the SQL editor surface using [`ContentThemePalette`] colors and `editor_font`.
#[must_use]
pub fn render_sql_editor(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    _cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let font = editor_gpui_font(&ws.controller.settings);
    let line_count = ws.controller.editor_lines.len();
    div()
        .id("workspace-sql-editor")
        .flex_1()
        .size_full()
        .flex()
        .flex_col()
        .p(px(12.0))
        .font(font)
        .bg(ws.content_palette.editor_background)
        .text_color(ws.content_palette.editor_text)
        .child(format!("SQL Editor ({line_count} lines)"))
        .into_any_element()
}
