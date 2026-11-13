//! Right-side AI SQL Assistant drawer with syntax-highlighted SQL block previews.

use herogpui::gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, Window, div, px,
};
use herogpui::theme::ActiveTheme;

use super::DataForgeWorkspace;
use super::theme::{HAIRLINE_BORDER_PX, hairline_border_color, panel_surface_bg};

/// Persistent UI state for the right-hand AI Assistant drawer.
#[derive(Debug, Default)]
pub struct AiDrawerViewState {
    pub scroll_to_bottom: bool,
}

impl AiDrawerViewState {
    /// Initializes AI drawer state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) -> Self {
        Self::default()
    }
}

/// Renders the right-hand AI SQL Assistant drawer.
#[must_use]
pub fn render_ai_drawer(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .id("workspace-ai-drawer")
        .w(px(340.0))
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
            "AI Assistant ({} messages)",
            ws.controller.ai_drawer.messages.len()
        ))
        .into_any_element()
}
