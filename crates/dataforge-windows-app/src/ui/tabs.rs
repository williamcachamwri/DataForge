//! Horizontal tab strip backed by `dataforge_state::tabs::TabStripModel`.

use herogpui::gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, ScrollHandle, Styled,
    Window, div, px,
};
use herogpui::theme::ActiveTheme;

use super::DataForgeWorkspace;
use super::theme::{HAIRLINE_BORDER_PX, hairline_border_color, muted_surface_bg};

/// Persistent UI state for the horizontal workspace tab strip.
#[derive(Debug, Default)]
pub struct TabsStripState {
    pub scroll_handle: ScrollHandle,
}

impl TabsStripState {
    /// Creates tab-strip scroll/drag state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) -> Self {
        Self {
            scroll_handle: ScrollHandle::new(),
        }
    }
}

/// Renders the active workspace tab strip.
#[must_use]
pub fn render_tab_strip(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let count = ws.controller.tabs.tabs().len();
    div()
        .id("workspace-tabs")
        .h(px(34.0))
        .w_full()
        .flex()
        .items_center()
        .px(px(8.0))
        .gap(px(4.0))
        .bg(muted_surface_bg(cx))
        .border_b(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .text_xs()
        .text_color(colors.muted)
        .child(format!("Tabs ({count})"))
        .into_any_element()
}
