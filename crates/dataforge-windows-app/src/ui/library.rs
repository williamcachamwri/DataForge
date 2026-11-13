//! Full-screen Connections Library, URL parser, foreign import, and connection form.

use herogpui::gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, Window, div, px,
};
use herogpui::theme::ActiveTheme;

use super::DataForgeWorkspace;

/// Persistent UI state for the Connections Library and Form screen.
#[derive(Debug, Default)]
pub struct LibraryScreenState {
    pub url_draft: String,
}

impl LibraryScreenState {
    /// Initializes library screen state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) -> Self {
        Self::default()
    }
}

/// Renders the Connections Library and New Connection form screen.
#[must_use]
pub fn render_library_screen(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let count = ws.controller.library.outline().len();
    div()
        .id("connections-library-screen")
        .flex_1()
        .size_full()
        .flex()
        .flex_col()
        .p(px(20.0))
        .gap(px(8.0))
        .bg(ws.content_palette.window_background)
        .text_color(colors.foreground)
        .child(format!("Connections Library ({count} rows)"))
        .into_any_element()
}
