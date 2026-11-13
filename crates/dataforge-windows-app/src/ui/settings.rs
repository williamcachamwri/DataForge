//! Full-screen Application Settings (Appearance, Themes, Fonts, Editor, MCP, Shortcuts).

use herogpui::gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, Window, div, px,
};
use herogpui::theme::ActiveTheme;

use super::DataForgeWorkspace;

/// Persistent UI state for the Settings screen.
#[derive(Debug, Default)]
pub struct SettingsScreenState {
    pub dirty: bool,
}

impl SettingsScreenState {
    /// Initializes Settings screen state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) -> Self {
        Self::default()
    }
}

/// Renders the Settings screen backed by `ws.controller.settings` and `ws.settings_view`.
#[must_use]
pub fn render_settings_screen(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .id("workspace-settings-screen")
        .flex_1()
        .size_full()
        .flex()
        .flex_col()
        .p(px(20.0))
        .gap(px(8.0))
        .bg(ws.content_palette.window_background)
        .text_color(colors.foreground)
        .child(format!(
            "Settings — {:?} (editor: {}, value: {})",
            ws.settings_view.section,
            ws.controller.settings.editor_font.family,
            ws.controller.settings.value_font.family,
        ))
        .into_any_element()
}
