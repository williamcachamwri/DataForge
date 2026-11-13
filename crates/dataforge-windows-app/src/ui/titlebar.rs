//! Custom HeroGPUI client-side title bar with Win32 Snap Layout hit-testing.

use herogpui::components::TitleBar;
use herogpui::gpui::{AnyElement, Context, IntoElement, Window};

use super::DataForgeWorkspace;

/// Renders the custom DataForge title bar wired with workspace branding and mode title.
#[must_use]
pub fn render_titlebar(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    _cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    TitleBar::new("dataforge-titlebar")
        .title(ws.controller.strings.app_title)
        .into_any_element()
}
