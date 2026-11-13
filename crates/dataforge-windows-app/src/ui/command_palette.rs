//! Keyboard-driven command palette overlay (`Ctrl+K`).

use herogpui::gpui::{AnyElement, Context, InteractiveElement, IntoElement, Styled, Window, div};

use super::DataForgeWorkspace;

/// Persistent state for the workspace command palette (`Ctrl+K`).
#[derive(Debug, Default)]
pub struct CommandPaletteState {
    pub open: bool,
    pub query: String,
    pub selected_index: usize,
}

impl CommandPaletteState {
    /// Initializes command palette state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) -> Self {
        Self::default()
    }

    /// Opens the command palette and resets selection.
    pub fn open_palette(&mut self, _window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) {
        self.open = true;
        self.selected_index = 0;
    }

    /// Toggles command palette visibility.
    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<DataForgeWorkspace>) {
        if self.open {
            self.close();
        } else {
            self.open_palette(window, cx);
        }
    }

    /// Closes the command palette.
    pub fn close(&mut self) {
        self.open = false;
    }

    /// Returns whether the command palette overlay is currently open.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }
}

/// Renders the command palette overlay when open.
#[must_use]
pub fn render_command_palette(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    _cx: &mut Context<DataForgeWorkspace>,
) -> Option<AnyElement> {
    if !ws.command_palette_ui.is_open() {
        return None;
    }
    Some(
        div()
            .id("dataforge-command-palette")
            .absolute()
            .inset_0()
            .into_any_element(),
    )
}
