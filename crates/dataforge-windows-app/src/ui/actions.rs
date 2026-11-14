//! GPUI workspace action definitions and Windows keyboard shortcut bindings.

use herogpui::actions;
use herogpui::gpui::{self, Action, App, KeyBinding};

/// Focus key context attached to the root [`crate::ui::DataForgeWorkspace`] element.
pub const WORKSPACE_KEY_CONTEXT: &str = "DataForgeWorkspace";

actions!(
    dataforge,
    [
        NewQueryTab,
        CloseActiveTab,
        ReopenClosedTab,
        NextTab,
        PrevTab,
        RunQuery,
        RunSelectedQuery,
        CancelQuery,
        SaveQuery,
        CommitEdits,
        RollbackEdits,
        AddRow,
        DeleteSelectedRows,
        ExportResults,
        OpenCommandPalette,
        FocusSidebarFilter,
        ToggleSidebar,
        ToggleInspector,
        ToggleAiDrawer,
        OpenConnections,
        OpenSettings,
        ToggleTheme,
        EscapeDismiss,
    ]
);

fn binding<A: Action>(keys: &'static str, action: A) -> KeyBinding {
    KeyBinding::new(keys, action, Some(WORKSPACE_KEY_CONTEXT))
}

/// Builds the canonical DataForge Windows keybindings scoped to [`WORKSPACE_KEY_CONTEXT`].
#[must_use]
pub fn workspace_key_bindings() -> Vec<KeyBinding> {
    vec![
        binding("ctrl-enter", RunQuery),
        binding("f5", RunQuery),
        binding("ctrl-shift-enter", RunSelectedQuery),
        binding("shift-escape", CancelQuery),
        binding("ctrl-k", OpenCommandPalette),
        binding("ctrl-shift-k", OpenCommandPalette),
        binding("ctrl-p", OpenCommandPalette),
        binding("ctrl-f", FocusSidebarFilter),
        binding("ctrl-t", NewQueryTab),
        binding("ctrl-w", CloseActiveTab),
        binding("ctrl-shift-t", ReopenClosedTab),
        binding("ctrl-tab", NextTab),
        binding("ctrl-shift-tab", PrevTab),
        binding("ctrl-b", ToggleSidebar),
        binding("ctrl-i", ToggleInspector),
        binding("ctrl-shift-a", ToggleAiDrawer),
        binding("ctrl-shift-o", OpenConnections),
        binding("ctrl-,", OpenSettings),
        binding("ctrl-s", SaveQuery),
        binding("ctrl-shift-s", CommitEdits),
        binding("ctrl-shift-r", RollbackEdits),
        binding("alt-insert", AddRow),
        binding("ctrl-shift-backspace", DeleteSelectedRows),
        binding("ctrl-shift-e", ExportResults),
        binding("ctrl-shift-m", ToggleTheme),
        binding("escape", EscapeDismiss),
    ]
}

/// Registers all DataForge keyboard shortcuts in GPUI's global keymap.
pub fn bind_workspace_keys(cx: &mut App) {
    cx.bind_keys(workspace_key_bindings());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_key_bindings_parse_without_errors() {
        let bindings = workspace_key_bindings();
        assert_eq!(bindings.len(), 26);
    }
}
