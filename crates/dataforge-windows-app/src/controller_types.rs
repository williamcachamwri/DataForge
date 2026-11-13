//! Auxiliary state types used by [`AppController`](crate::app::AppController)
//! for SQL editor modes, split panes, query history, and saved queries.

/// Vim modal editing state when Vim mode is active in the SQL editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VimMode {
    #[default]
    Normal,
    Insert,
    Visual,
    Command,
}

impl VimMode {
    /// Human-readable status indicator for the status bar.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Visual => "VISUAL",
            Self::Command => "COMMAND",
        }
    }
}

/// Split-pane layout mode for the SQL editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SplitPaneMode {
    #[default]
    Single,
    Horizontal,
    Vertical,
}

/// Entry recorded in the SQL editor query history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryHistoryEntry {
    pub sql: String,
    pub database: String,
    pub elapsed_ms: u64,
    pub rows_affected: Option<u64>,
    pub succeeded: bool,
}

/// User-saved SQL query snippet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedQueryEntry {
    pub name: String,
    pub sql: String,
    pub folder: Option<String>,
}
