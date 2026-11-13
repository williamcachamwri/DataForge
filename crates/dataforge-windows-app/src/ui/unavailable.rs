//! Inline connection-unavailable / auth-failed / disconnected empty-state banners.

use dataforge_state::connection_window::ConnectionWindowPhase;
use herogpui::gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, Window, div, px,
};
use herogpui::theme::ActiveTheme;

use super::DataForgeWorkspace;
use super::theme::{HAIRLINE_BORDER_PX, hairline_border_color, panel_surface_bg};

/// Renders an inline banner when the active workspace phase is unavailable,
/// connecting, or reconnecting. Returns `None` when connected or idle.
#[must_use]
pub fn render_unavailable_banner(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> Option<AnyElement> {
    let phase = ws
        .controller
        .connections
        .phase(ws.controller.active_workspace)?;
    let (title, detail) = match phase {
        ConnectionWindowPhase::Unavailable(view) => (
            view.summary.clone(),
            view.detail.clone().unwrap_or_default(),
        ),
        ConnectionWindowPhase::Connecting { .. } => (
            "Connecting...".to_owned(),
            ws.controller.toolbar_status.host.clone(),
        ),
        ConnectionWindowPhase::Reconnecting { .. } => (
            "Reconnecting...".to_owned(),
            ws.controller.toolbar_status.host.clone(),
        ),
        ConnectionWindowPhase::Disconnected | ConnectionWindowPhase::Connected => return None,
    };
    Some(render_unavailable_card(&title, &detail, cx))
}

/// Reusable compact card for disconnected, unsupported, or unavailable surfaces.
#[must_use]
pub fn render_unavailable_card(
    title: &str,
    detail: &str,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .id("workspace-unavailable-card")
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .px(px(12.0))
        .py(px(8.0))
        .bg(panel_surface_bg(cx))
        .border_b(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.foreground)
                        .child(title.to_owned()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted)
                        .child(detail.to_owned()),
                ),
        )
        .into_any_element()
}
