//! Reactive screen, sub-view, result-pane, cell-edit, and toast state helpers
//! for the HeroGPUI workspace shell.

use dataforge_state::editor::RunEventAccumulator;
use dataforge_state::tabs::TabId;
use herogpui::components::Toast;
use herogpui::core::Color as HeroColor;
use herogpui::gpui::{App, SharedString};
use tablepro_core::{ResultEvent, RunSummary};

/// Result sub-tab rendered below a SQL query editor tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResultPaneSubView {
    #[default]
    Results,
    Messages,
    Summary,
    Explain,
    Charts,
}

/// Active inline editor session over a single editable DataGrid cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineCellEditState {
    pub row_index: usize,
    pub col_index: usize,
    pub draft_text: String,
    pub original_text: String,
    pub set_null: bool,
}

/// Per-query-tab streaming result state backed by [`RunEventAccumulator`].
#[derive(Debug, Clone)]
pub struct QueryTabResultState {
    pub tab_id: TabId,
    pub accumulator: RunEventAccumulator,
    pub active_result_set: usize,
    pub sub_view: ResultPaneSubView,
    pub elapsed_ms: Option<u64>,
    pub active_error: Option<String>,
    pub running: bool,
}

impl QueryTabResultState {
    /// Creates an idle result accumulator for `tab_id`.
    #[must_use]
    pub fn new(tab_id: TabId) -> Self {
        Self {
            tab_id,
            accumulator: RunEventAccumulator::new(),
            active_result_set: 0,
            sub_view: ResultPaneSubView::Results,
            elapsed_ms: None,
            active_error: None,
            running: false,
        }
    }

    /// Clears prior results and marks the query tab as running.
    pub fn reset_for_run(&mut self) {
        self.accumulator = RunEventAccumulator::new();
        self.active_result_set = 0;
        self.elapsed_ms = None;
        self.active_error = None;
        self.running = true;
    }

    /// Feeds one live [`ResultEvent`] into the tab's [`RunEventAccumulator`].
    pub fn push_event(&mut self, event: ResultEvent) {
        self.accumulator.push_event(event);
        if !self.accumulator.result_sets.is_empty()
            && self.active_result_set >= self.accumulator.result_sets.len()
        {
            self.active_result_set = self.accumulator.result_sets.len() - 1;
        }
    }

    /// Finalizes the run with its [`RunSummary`] and measured elapsed milliseconds.
    pub fn finish(&mut self, summary: RunSummary, elapsed_ms: u64) {
        self.accumulator.finish(summary);
        self.elapsed_ms = Some(elapsed_ms);
        self.running = false;
    }

    /// Records a terminal execution error on this query tab.
    pub fn fail(&mut self, message: impl Into<String>) {
        self.active_error = Some(message.into());
        self.running = false;
    }
}

/// Focused sub-section inside the Connections Library & Form screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionScreenFocus {
    #[default]
    LibrarySearch,
    ConnectionForm,
    UrlImport,
    ForeignImport,
}

/// UI state for the Connections Library & Form screen.
#[derive(Debug, Clone, Default)]
pub struct ConnectionsViewState {
    pub search_query: String,
    pub selected_id: Option<String>,
    pub focus: ConnectionScreenFocus,
    pub show_ssh_section: bool,
    pub show_tls_section: bool,
    pub testing_connection: bool,
}

/// Active category inside the Settings screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsSection {
    #[default]
    Appearance,
    Editor,
    Fonts,
    McpServer,
    Shortcuts,
}

/// UI state for the Settings screen.
#[derive(Debug, Clone, Default)]
pub struct SettingsViewState {
    pub section: SettingsSection,
    pub theme_preview_id: Option<String>,
    pub status_notice: Option<String>,
}

/// Active main content sub-view when in [`crate::ScreenMode::Workspace`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkspaceSubView {
    #[default]
    DataGrid,
    Structure,
    SqlEditor,
    SplitSqlAndGrid,
}

/// Active full-pane workbench tool rendered in place of the tab sub-view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceToolView {
    SchemaDesigner,
    ImportWizard,
    ErDiagram,
    QueryBuilder,
    Compare,
    Admin,
}

/// UI state for the active Workspace panes (filter bar, inline edit, confirm sheets).
#[derive(Debug, Clone, Default)]
pub struct WorkspacePaneState {
    pub sub_view: WorkspaceSubView,
    pub active_tool: Option<WorkspaceToolView>,
    pub filter_input: String,
    pub inline_edit: Option<InlineCellEditState>,
    pub confirm_commit_open: bool,
    pub close_guard_open: bool,
}

impl WorkspacePaneState {
    /// Opens `tool` in the workspace center pane.
    pub fn open_tool(&mut self, tool: WorkspaceToolView) {
        self.active_tool = Some(tool);
    }

    /// Closes any active workbench tool and returns to the active tab's sub-view.
    pub fn close_tool(&mut self) {
        self.active_tool = None;
    }

    /// Toggles `tool`, closing it when already active.
    pub fn toggle_tool(&mut self, tool: WorkspaceToolView) {
        if self.active_tool == Some(tool) {
            self.active_tool = None;
        } else {
            self.active_tool = Some(tool);
        }
    }
}

/// UI state for the right-hand AI Assistant drawer.
#[derive(Debug, Clone, Default)]
pub struct AiPanelState {
    pub prompt_draft: String,
    pub waiting_reply: bool,
}

/// Semantic toast severity mapped onto HeroGPUI [`Toast`] variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastSeverity {
    Info,
    Success,
    Warning,
    Error,
}

/// Pushes a HeroGPUI toast notification onto the app-wide [`herogpui::components::ToastStore`].
pub fn push_toast(
    severity: ToastSeverity,
    title: impl Into<SharedString>,
    description: Option<SharedString>,
    cx: &mut App,
) -> u64 {
    let variant = match severity {
        ToastSeverity::Info => HeroColor::Accent,
        ToastSeverity::Success => HeroColor::Success,
        ToastSeverity::Warning => HeroColor::Warning,
        ToastSeverity::Error => HeroColor::Danger,
    };
    let mut toast = Toast::new(title).variant(variant);
    if let Some(desc) = description {
        toast = toast.description(desc);
    }
    toast.push(None, cx)
}

/// Convenience helper for a success toast.
pub fn push_success_toast(
    title: impl Into<SharedString>,
    description: Option<SharedString>,
    cx: &mut App,
) -> u64 {
    push_toast(ToastSeverity::Success, title, description, cx)
}

/// Convenience helper for an error toast.
pub fn push_error_toast(
    title: impl Into<SharedString>,
    description: Option<SharedString>,
    cx: &mut App,
) -> u64 {
    push_toast(ToastSeverity::Error, title, description, cx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tablepro_core::{RunEnd, RunSummary, TransactionState};

    #[test]
    fn query_tab_result_state_tracks_streaming_events_and_finish() {
        let mut state = QueryTabResultState::new(TabId(42));
        assert!(!state.running);

        state.reset_for_run();
        assert!(state.running);
        state.push_event(ResultEvent::CommandCompleted {
            rows_affected: Some(3),
        });
        assert_eq!(state.accumulator.commands_completed, vec![Some(3)]);

        let summary = RunSummary {
            elapsed: std::time::Duration::from_millis(12),
            end: RunEnd::Completed,
            transaction_state: TransactionState::Idle,
        };
        state.finish(summary, 12);
        assert!(!state.running);
        assert_eq!(state.elapsed_ms, Some(12));
        assert!(state.accumulator.summary.is_some());
    }
}
