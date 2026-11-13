//! Right-side Row & Column Inspector drawer displaying selected cell details.

use herogpui::gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, Window, div, px,
};
use herogpui::theme::ActiveTheme;

use super::DataForgeWorkspace;
use super::theme::{HAIRLINE_BORDER_PX, hairline_border_color, panel_surface_bg};

/// Persistent UI state for the right-hand Row Inspector drawer.
#[derive(Debug, Default)]
pub struct InspectorViewState {
    pub raw_pretty_mode: bool,
}

impl InspectorViewState {
    /// Initializes inspector drawer state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) -> Self {
        Self::default()
    }
}

/// Renders the right-hand Row Inspector drawer.
#[must_use]
pub fn render_inspector(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .id("workspace-inspector")
        .w(px(300.0))
        .h_full()
        .flex()
        .flex_col()
        .p(px(12.0))
        .gap(px(8.0))
        .bg(panel_surface_bg(cx))
        .border_l(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .text_xs()
        .text_color(colors.muted)
        .child(format!(
            "Inspector ({} fields)",
            ws.controller.inspector_fields.len()
        ))
        .into_any_element()
}
