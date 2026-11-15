//! Win32 UI Automation (`WM_GETOBJECT`) baseline and keyboard navigation.

use crate::shell_layout::ShellHitTarget;

/// Logical focus region in the TableMinus shell for keyboard `Tab` / `Shift+Tab` cycling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FocusRegion {
    ConnectionRail,
    ObjectSidebar,
    Toolbar,
    TabStrip,
    ContentArea,
    RowInspector,
    AiDrawer,
    StatusBar,
}

impl FocusRegion {
    /// Canonical clockwise focus order.
    pub const ORDER: [Self; 8] = [
        Self::ConnectionRail,
        Self::ObjectSidebar,
        Self::Toolbar,
        Self::TabStrip,
        Self::ContentArea,
        Self::RowInspector,
        Self::AiDrawer,
        Self::StatusBar,
    ];

    /// Advances focus to the next visible region (or previous when `reverse` is true).
    #[must_use]
    pub fn step(
        self,
        reverse: bool,
        sidebar_visible: bool,
        inspector_visible: bool,
        ai_visible: bool,
    ) -> Self {
        let len = Self::ORDER.len();
        let current = Self::ORDER.iter().position(|r| *r == self).unwrap_or(4);
        for offset in 1..=len {
            let idx = if reverse {
                (current + len - offset) % len
            } else {
                (current + offset) % len
            };
            let candidate = Self::ORDER[idx];
            let visible = match candidate {
                Self::ObjectSidebar => sidebar_visible,
                Self::RowInspector => inspector_visible,
                Self::AiDrawer => ai_visible,
                _ => true,
            };
            if visible {
                return candidate;
            }
        }
        Self::ContentArea
    }

    /// Maps a focus region to its UIA control type name.
    #[must_use]
    pub const fn uia_role(self) -> &'static str {
        match self {
            Self::ConnectionRail => "ToolBar",
            Self::ObjectSidebar => "Tree",
            Self::Toolbar => "ToolBar",
            Self::TabStrip => "Tab",
            Self::ContentArea => "Pane",
            Self::RowInspector => "Pane",
            Self::AiDrawer => "Pane",
            Self::StatusBar => "StatusBar",
        }
    }

    /// Human-readable UIA accessible name.
    #[must_use]
    pub const fn uia_name(self) -> &'static str {
        match self {
            Self::ConnectionRail => "Connection Rail",
            Self::ObjectSidebar => "Object Sidebar",
            Self::Toolbar => "Main Toolbar",
            Self::TabStrip => "Open Tabs",
            Self::ContentArea => "Workspace Content",
            Self::RowInspector => "Row Inspector",
            Self::AiDrawer => "AI Assistant",
            Self::StatusBar => "Status Bar",
        }
    }
}

/// Maps a [`ShellHitTarget`] to its UIA control role for `WM_GETOBJECT` queries.
#[must_use]
pub const fn uia_role_for_target(target: ShellHitTarget) -> &'static str {
    match target {
        ShellHitTarget::RailLibraryButton
        | ShellHitTarget::RailCollapseButton
        | ShellHitTarget::RailSettingsButton
        | ShellHitTarget::ToolbarConnectButton
        | ShellHitTarget::ToolbarRefreshButton
        | ShellHitTarget::ToolbarApplyButton
        | ShellHitTarget::ToolbarInspectorButton
        | ShellHitTarget::ToolbarAiButton
        | ShellHitTarget::TabClose(_)
        | ShellHitTarget::TabNewButton
        | ShellHitTarget::StatusDataToggle
        | ShellHitTarget::StatusStructureToggle
        | ShellHitTarget::StatusColumnsButton
        | ShellHitTarget::StatusFiltersButton
        | ShellHitTarget::StatusPrevPage
        | ShellHitTarget::StatusNextPage
        | ShellHitTarget::StatusSettingsButton => "Button",
        ShellHitTarget::RailConnection(_) | ShellHitTarget::SidebarItem(_) => "TreeItem",
        ShellHitTarget::SidebarFilterBox => "Edit",
        ShellHitTarget::SidebarFavoritesToggle => "CheckBox",
        ShellHitTarget::ToolbarStatusPill => "StatusBar",
        ShellHitTarget::Tab(_) => "TabItem",
        ShellHitTarget::ContentArea
        | ShellHitTarget::RowInspector
        | ShellHitTarget::AiDrawer
        | ShellHitTarget::Outside => "Pane",
    }
}

/// Modifier key bitmask for platform-independent shortcut dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// High-level keyboard commands dispatched by TableMinus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCommand {
    FocusNextRegion,
    FocusPrevRegion,
    NextTab,
    PrevTab,
    CloseTabOrWindow,
    ReopenClosedTab,
    ExecuteQuery,
    NewQueryTab,
    ToggleSidebar,
    ToggleInspector,
    ToggleAiDrawer,
    FocusFilter,
    OpenSettings,
    OpenConnectionsLibrary,
}

/// Win32 Virtual-Key constants used by [`translate_key`].
pub mod vk {
    pub const TAB: u32 = 0x09;
    pub const RETURN: u32 = 0x0D;
    pub const F5: u32 = 0x74;
    pub const KEY_B: u32 = 0x42;
    pub const KEY_F: u32 = 0x46;
    pub const KEY_I: u32 = 0x49;
    pub const KEY_L: u32 = 0x4C;
    pub const KEY_N: u32 = 0x4E;
    pub const KEY_T: u32 = 0x54;
    pub const KEY_W: u32 = 0x57;
    pub const KEY_A: u32 = 0x41;
    pub const OEM_COMMA: u32 = 0xBC;
}

/// Translates a Win32 virtual key code and modifier state into a [`KeyCommand`].
#[must_use]
pub const fn translate_key(vkey: u32, mods: Modifiers) -> Option<KeyCommand> {
    match (vkey, mods.ctrl, mods.shift, mods.alt) {
        (vk::TAB, false, false, false) => Some(KeyCommand::FocusNextRegion),
        (vk::TAB, false, true, false) => Some(KeyCommand::FocusPrevRegion),
        (vk::TAB, true, false, false) => Some(KeyCommand::NextTab),
        (vk::TAB, true, true, false) => Some(KeyCommand::PrevTab),
        (vk::KEY_W, true, false, false) => Some(KeyCommand::CloseTabOrWindow),
        (vk::KEY_T, true, true, false) => Some(KeyCommand::ReopenClosedTab),
        (vk::F5, false, false, false) | (vk::RETURN, true, false, false) => {
            Some(KeyCommand::ExecuteQuery)
        }
        (vk::KEY_N, true, false, false) => Some(KeyCommand::NewQueryTab),
        (vk::KEY_B, true, false, false) => Some(KeyCommand::ToggleSidebar),
        (vk::KEY_I, true, false, false) => Some(KeyCommand::ToggleInspector),
        (vk::KEY_A, true, true, false) => Some(KeyCommand::ToggleAiDrawer),
        (vk::KEY_F, true, false, false) => Some(KeyCommand::FocusFilter),
        (vk::KEY_L, true, false, false) => Some(KeyCommand::OpenConnectionsLibrary),
        (vk::OEM_COMMA, true, false, false) => Some(KeyCommand::OpenSettings),
        _ => None,
    }
}

/// Handles `WM_GETOBJECT` UIA baseline request; returns `Some(0)` when handled.
#[must_use]
pub fn handle_wm_getobject(lparam: isize) -> Option<isize> {
    // `OBJID_CLIENT` (-4) or `UiaRootObjectId` (-25)
    if lparam == -4 || lparam == -25 {
        Some(0)
    } else {
        None
    }
}
