//! Workspace toolbar hosting the active connection status pill and primary actions.

use herogpui::gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, Window, div, px,
};
use herogpui::theme::ActiveTheme;

use super::DataForgeWorkspace;
use super::theme::{HAIRLINE_BORDER_PX, hairline_border_color, muted_surface_bg};

/// Renders the top workspace toolbar with live [`ToolbarStatusPill`] summary.
#[must_use]
pub fn render_toolbar(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .id("workspace-toolbar")
        .h(px(40.0))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .px(px(12.0))
        .bg(muted_surface_bg(cx))
        .border_b(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .text_xs()
        .text_color(colors.muted)
        .child(ws.controller.toolbar_status.format_pill_text())
        .into_any_element()
}
