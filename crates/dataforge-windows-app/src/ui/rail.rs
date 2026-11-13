//! Left vertical connection rail showing live connected/pinned workspaces.

use herogpui::gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, Window, div, px,
};
use herogpui::theme::ActiveTheme;

use super::DataForgeWorkspace;
use super::theme::{HAIRLINE_BORDER_PX, hairline_border_color, panel_surface_bg};

/// Renders the compact left connection rail.
#[must_use]
pub fn render_rail(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .id("workspace-rail")
        .w(px(52.0))
        .h_full()
        .flex()
        .flex_col()
        .items_center()
        .py(px(8.0))
        .gap(px(6.0))
        .bg(panel_surface_bg(cx))
        .border_r(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .text_xs()
        .text_color(colors.muted)
        .child(format!("{}", ws.controller.rail_items.len()))
        .into_any_element()
}
