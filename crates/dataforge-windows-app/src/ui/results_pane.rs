//! Streaming SQL query results, messages, and execution summary pane.

use herogpui::gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, Window, div, px,
};
use herogpui::theme::ActiveTheme;

use super::DataForgeWorkspace;
use super::theme::{HAIRLINE_BORDER_PX, hairline_border_color, panel_surface_bg};

/// Persistent UI state for the bottom SQL query results pane.
#[derive(Debug, Default)]
pub struct ResultsPaneState {
    pub height_px: Option<f32>,
}

impl ResultsPaneState {
    /// Initializes Results pane state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) -> Self {
        Self::default()
    }
}

/// Renders the per-tab streaming query results / messages / summary pane.
#[must_use]
pub fn render_results_pane(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let result_sets = ws
        .active_tab_result()
        .map_or(0, |state| state.accumulator.result_sets.len());
    div()
        .id("workspace-results-pane")
        .h(px(220.0))
        .w_full()
        .flex()
        .flex_col()
        .p(px(12.0))
        .bg(panel_surface_bg(cx))
        .border_t(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .text_xs()
        .text_color(colors.muted)
        .child(format!("Query Results ({result_sets} sets)"))
        .into_any_element()
}
