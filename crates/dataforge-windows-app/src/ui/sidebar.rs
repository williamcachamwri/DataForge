//! Object explorer sidebar (schemas, tables, views, functions, favorites, filter).

use herogpui::gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, Window, div, px,
};
use herogpui::theme::ActiveTheme;

use super::DataForgeWorkspace;
use super::theme::{HAIRLINE_BORDER_PX, hairline_border_color, panel_surface_bg};

/// Persistent UI state for the object explorer sidebar.
#[derive(Debug, Default)]
pub struct SidebarPanelState {
    pub filter_focused: bool,
}

impl SidebarPanelState {
    /// Initializes sidebar state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) -> Self {
        Self::default()
    }

    /// Requests focus on the sidebar filter input (`Ctrl+F`).
    pub fn focus_filter(&mut self, _window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) {
        self.filter_focused = true;
    }
}

/// Renders the database object explorer sidebar.
#[must_use]
pub fn render_sidebar(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .id("workspace-sidebar")
        .w(px(260.0))
        .h_full()
        .flex()
        .flex_col()
        .p(px(12.0))
        .gap(px(8.0))
        .bg(panel_surface_bg(cx))
        .border_r(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .text_xs()
        .text_color(colors.muted)
        .child(format!("Objects ({})", ws.controller.sidebar_rows.len()))
        .into_any_element()
}
