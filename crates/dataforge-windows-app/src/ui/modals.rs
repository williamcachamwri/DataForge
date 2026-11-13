//! Modal & confirmation alert layer (close-transaction guard, commit edits, export).

use herogpui::gpui::{AnyElement, Context, InteractiveElement, IntoElement, Styled, Window, div};

use super::DataForgeWorkspace;

/// Persistent UI state for modal dialogs and confirmation sheets.
#[derive(Debug, Default)]
pub struct ModalLayerState {
    pub export_modal_open: bool,
}

impl ModalLayerState {
    /// Initializes modal overlay state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) -> Self {
        Self::default()
    }

    /// Opens the result/grid export modal.
    pub fn open_export(&mut self, _window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) {
        self.export_modal_open = true;
    }

    /// Closes all modal-owned dialogs.
    pub fn close_all(&mut self) {
        self.export_modal_open = false;
    }

    /// Returns `true` if any modal or workspace confirmation sheet is open.
    #[must_use]
    pub const fn has_open_modal(&self, ws: &DataForgeWorkspace) -> bool {
        self.export_modal_open
            || ws.workspace_pane.close_guard_open
            || ws.workspace_pane.confirm_commit_open
    }
}

/// Renders active confirmation alerts or modal dialogs above the workspace.
#[must_use]
pub fn render_modals(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    _cx: &mut Context<DataForgeWorkspace>,
) -> Option<AnyElement> {
    if !ws.modals_ui.has_open_modal(ws) {
        return None;
    }
    Some(
        div()
            .id("dataforge-modal-layer")
            .absolute()
            .inset_0()
            .into_any_element(),
    )
}
