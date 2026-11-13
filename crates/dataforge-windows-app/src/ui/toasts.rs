//! Non-blocking toast viewport overlay layered on top of the workspace window.

use herogpui::components::ToastViewport;
use herogpui::gpui::{AnyElement, Context, IntoElement, Window};

use super::DataForgeWorkspace;

/// Renders the application-wide HeroGPUI [`ToastViewport`].
#[must_use]
pub fn render_toast_viewport(
    _ws: &DataForgeWorkspace,
    _window: &mut Window,
    _cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    ToastViewport::new()
        .id("dataforge-toast-viewport")
        .into_any_element()
}
