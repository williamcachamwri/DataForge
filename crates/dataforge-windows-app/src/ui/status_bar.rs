//! Bottom workspace status bar displaying real connection/query/safe-mode metrics.

use herogpui::gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, Window, div, px,
};
use herogpui::theme::ActiveTheme;

use super::DataForgeWorkspace;
use super::theme::{HAIRLINE_BORDER_PX, hairline_border_color, muted_surface_bg};

/// Renders the bottom status bar driven by `AppController::status_bar`.
#[must_use]
pub fn render_status_bar(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let status = &ws.controller.status_bar;
    div()
        .id("workspace-status-bar")
        .h(px(26.0))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .px(px(12.0))
        .bg(muted_surface_bg(cx))
        .border_t(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .text_xs()
        .text_color(colors.muted)
        .child(format!(
            "{:?} | page {}/{}",
            status.view_mode, status.current_page, status.total_pages
        ))
        .into_any_element()
}
