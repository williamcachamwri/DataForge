//! Connection window phase machine, attemptToken fencing, session liveness,
//! inline unavailable state, title resolution, and SafeMode/transaction guards.

use std::collections::BTreeMap;

/// Unique identifier for a workspace hosted inside a connection window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceId(pub u64);

/// Monotonic fencing token for connection attempts.
///
/// A cancel updates the phase immediately and advances the token so any late
/// driver completion or failure carrying an older token is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AttemptToken(pub u64);

/// Lifecycle phase of a connection workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionWindowPhase {
    Disconnected,
    Connecting { token: AttemptToken },
    Connected,
    Unavailable(ConnectionUnavailableView),
    Reconnecting { token: AttemptToken },
}

impl ConnectionWindowPhase {
    /// Whether this phase retains restore intent for "Reopen Last Session".
    ///
    /// Per `connection-window.md`: connected or failed/unavailable workspaces dial
    /// again on restore; canceled or disconnected ones come back Not Connected.
    #[must_use]
    pub const fn retains_restore_intent(&self) -> bool {
        matches!(
            self,
            Self::Connected | Self::Unavailable(_) | Self::Reconnecting { .. }
        )
    }
}

/// Inline error state shown in `ConnectionUnavailableView` (never a modal alert).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionUnavailableView {
    pub summary: String,
    pub detail: Option<String>,
    pub can_retry: bool,
    pub is_auth_failure: bool,
}

/// Key used to decide whether a workspace's panes need to be rebuilt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePaneRenderKey {
    pub workspace_id: WorkspaceId,
    pub phase_tag: u8,
    pub unavailable_summary: Option<String>,
}

/// Logical pane selected by [`ConnectionWindowPaneResolver`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspacePaneKind {
    NotConnected,
    ConnectingSpinner,
    ConnectedContent,
    UnavailableInline,
}

/// Resolves which pane a workspace should render for its current phase.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConnectionWindowPaneResolver;

impl ConnectionWindowPaneResolver {
    /// Picks the pane kind for `phase`.
    #[must_use]
    pub const fn resolve(phase: &ConnectionWindowPhase) -> WorkspacePaneKind {
        match phase {
            ConnectionWindowPhase::Disconnected => WorkspacePaneKind::NotConnected,
            ConnectionWindowPhase::Connecting { .. }
            | ConnectionWindowPhase::Reconnecting { .. } => WorkspacePaneKind::ConnectingSpinner,
            ConnectionWindowPhase::Connected => WorkspacePaneKind::ConnectedContent,
            ConnectionWindowPhase::Unavailable(_) => WorkspacePaneKind::UnavailableInline,
        }
    }
}

#[derive(Debug, Clone)]
struct WorkspaceEntry {
    phase: ConnectionWindowPhase,
    next_token: u64,
    active_token: Option<AttemptToken>,
    last_render_key: Option<WorkspacePaneRenderKey>,
    sync_count: u64,
}

impl WorkspaceEntry {
    fn new() -> Self {
        Self {
            phase: ConnectionWindowPhase::Disconnected,
            next_token: 1,
            active_token: None,
            last_render_key: None,
            sync_count: 0,
        }
    }

    fn issue_token(&mut self) -> AttemptToken {
        let token = AttemptToken(self.next_token);
        self.next_token = self.next_token.saturating_add(1);
        self.active_token = Some(token);
        token
    }

    fn render_key(&self, id: WorkspaceId) -> WorkspacePaneRenderKey {
        let (phase_tag, unavailable_summary) = match &self.phase {
            ConnectionWindowPhase::Disconnected => (0, None),
            ConnectionWindowPhase::Connecting { .. } => (1, None),
            ConnectionWindowPhase::Connected => (2, None),
            ConnectionWindowPhase::Unavailable(view) => (3, Some(view.summary.clone())),
            ConnectionWindowPhase::Reconnecting { .. } => (4, None),
        };
        WorkspacePaneRenderKey {
            workspace_id: id,
            phase_tag,
            unavailable_summary,
        }
    }
}

/// Owns phase transitions for all workspaces in a connection window.
///
/// `transition_to` is the single phase writer and always ends in `sync_panes`.
#[derive(Debug, Default, Clone)]
pub struct ConnectionWindowPhaseMachine {
    workspaces: BTreeMap<WorkspaceId, WorkspaceEntry>,
    selected: Option<WorkspaceId>,
}

impl ConnectionWindowPhaseMachine {
    /// Creates an empty phase machine.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a workspace in the `Disconnected` phase.
    pub fn add_workspace(&mut self, id: WorkspaceId) {
        self.workspaces
            .entry(id)
            .or_insert_with(WorkspaceEntry::new);
        if self.selected.is_none() {
            self.selected = Some(id);
        }
        self.sync_panes(id);
    }

    /// Selects the active workspace.
    pub fn select_workspace(&mut self, id: WorkspaceId) {
        if self.workspaces.contains_key(&id) {
            self.selected = Some(id);
        }
    }

    /// Returns the phase of `id`, if registered.
    #[must_use]
    pub fn phase(&self, id: WorkspaceId) -> Option<&ConnectionWindowPhase> {
        self.workspaces.get(&id).map(|w| &w.phase)
    }

    /// Returns how many times `sync_panes` actually rebuilt panes for `id`.
    #[must_use]
    pub fn pane_rebuild_count(&self, id: WorkspaceId) -> u64 {
        self.workspaces.get(&id).map_or(0, |w| w.sync_count)
    }

    /// Begins a fresh connection attempt and returns its fencing [`AttemptToken`].
    pub fn begin_connect(&mut self, id: WorkspaceId) -> Option<AttemptToken> {
        let token = self.workspaces.get_mut(&id)?.issue_token();
        self.transition_to(ConnectionWindowPhase::Connecting { token }, id);
        Some(token)
    }

    /// Begins a reconnect attempt from `Connected` or `Unavailable`.
    pub fn begin_reconnect(&mut self, id: WorkspaceId) -> Option<AttemptToken> {
        let token = self.workspaces.get_mut(&id)?.issue_token();
        self.transition_to(ConnectionWindowPhase::Reconnecting { token }, id);
        Some(token)
    }

    /// Cancels any in-flight attempt immediately and moves to `Disconnected`.
    pub fn cancel_connect(&mut self, id: WorkspaceId) {
        if let Some(entry) = self.workspaces.get_mut(&id) {
            entry.active_token = None;
            self.transition_to(ConnectionWindowPhase::Disconnected, id);
        }
    }

    /// Disconnects a workspace cleanly.
    pub fn disconnect(&mut self, id: WorkspaceId) {
        if let Some(entry) = self.workspaces.get_mut(&id) {
            entry.active_token = None;
            self.transition_to(ConnectionWindowPhase::Disconnected, id);
        }
    }

    /// Completes a connection attempt if `token` matches the workspace's active token.
    ///
    /// Returns `true` if accepted, `false` if fenced out as stale.
    pub fn complete_connect(&mut self, id: WorkspaceId, token: AttemptToken) -> bool {
        let Some(entry) = self.workspaces.get_mut(&id) else {
            return false;
        };
        if entry.active_token != Some(token) {
            return false;
        }
        entry.active_token = None;
        self.transition_to(ConnectionWindowPhase::Connected, id);
        true
    }

    /// Fails a connection attempt if `token` matches the workspace's active token.
    ///
    /// Returns `true` if accepted, `false` if fenced out as stale.
    pub fn fail_connect(
        &mut self,
        id: WorkspaceId,
        token: AttemptToken,
        view: ConnectionUnavailableView,
    ) -> bool {
        let Some(entry) = self.workspaces.get_mut(&id) else {
            return false;
        };
        if entry.active_token != Some(token) {
            return false;
        }
        entry.active_token = None;
        self.transition_to(ConnectionWindowPhase::Unavailable(view), id);
        true
    }

    /// Closes the window, cancelling in-flight attempts for every hosted workspace.
    pub fn close_window(&mut self) {
        let ids: Vec<WorkspaceId> = self.workspaces.keys().copied().collect();
        for id in ids {
            self.cancel_connect(id);
        }
    }

    /// Single phase writer; always ends in `sync_panes`.
    fn transition_to(&mut self, next: ConnectionWindowPhase, id: WorkspaceId) {
        if let Some(entry) = self.workspaces.get_mut(&id) {
            entry.phase = next;
        }
        self.sync_panes(id);
    }

    fn sync_panes(&mut self, id: WorkspaceId) {
        if let Some(entry) = self.workspaces.get_mut(&id) {
            let key = entry.render_key(id);
            if entry.last_render_key.as_ref() != Some(&key) {
                entry.last_render_key = Some(key);
                entry.sync_count = entry.sync_count.saturating_add(1);
            }
        }
    }
}

/// Resolves non-blank window titles from connection and database metadata.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowTitleResolver;

impl WindowTitleResolver {
    /// Resolves a guaranteed non-blank window title.
    #[must_use]
    pub fn resolve(
        connection_name: &str,
        database: Option<&str>,
        phase: &ConnectionWindowPhase,
    ) -> String {
        let base = match connection_name.trim() {
            "" => "Untitled Connection",
            name => name,
        };
        let with_db = match database.map(str::trim).filter(|d| !d.is_empty()) {
            Some(db) => format!("{base} — {db}"),
            None => base.to_owned(),
        };
        match phase {
            ConnectionWindowPhase::Disconnected => format!("{with_db} (Not Connected)"),
            ConnectionWindowPhase::Connecting { .. } => format!("{with_db} (Connecting…)"),
            ConnectionWindowPhase::Reconnecting { .. } => format!("{with_db} (Reconnecting…)"),
            ConnectionWindowPhase::Unavailable(_) => format!("{with_db} (Unavailable)"),
            ConnectionWindowPhase::Connected => with_db,
        }
    }
}

/// Resolves non-blank editor tab labels from creation.
#[derive(Debug, Clone, Copy, Default)]
pub struct EditorTabLabelResolver;

impl EditorTabLabelResolver {
    /// Resolves a guaranteed non-blank tab label.
    #[must_use]
    pub fn resolve(explicit_title: Option<&str>, index: usize, is_dirty: bool) -> String {
        let base = explicit_title
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map_or_else(
                || format!("Query {}", index.saturating_add(1)),
                str::to_owned,
            );
        if is_dirty {
            format!("{base} •")
        } else {
            base
        }
    }
}

/// Liveness state of an installed driver session.
///
/// An installed driver is a handle, not a live connection. Health is tracked
/// exclusively through `mark_session_unreachable` and `mark_session_live`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionLiveness {
    Live,
    Unreachable { started_with: u64, info: String },
}

/// UI-facing connection status derived solely from [`SessionLiveness`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportedStatus {
    Connected,
    Unreachable,
    Disconnected,
}

/// Opaque handle representing an installed driver that stays non-null across outages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverHandle {
    pub driver_id: String,
    pub generation: u64,
}

/// Tracks a connection session's driver handle, liveness, SafeMode, and transaction state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionSession {
    driver: Option<DriverHandle>,
    liveness: SessionLiveness,
    pub safe_mode: tablepro_core::SafeModeLevel,
    pub transaction_state: tablepro_core::TransactionState,
}

impl ConnectionSession {
    /// Creates a new session with no driver installed yet.
    #[must_use]
    pub fn new(safe_mode: tablepro_core::SafeModeLevel) -> Self {
        Self {
            driver: None,
            liveness: SessionLiveness::Live,
            safe_mode,
            transaction_state: tablepro_core::TransactionState::Idle,
        }
    }

    /// Installs a driver handle and marks the session live.
    pub fn install_driver(&mut self, driver: DriverHandle) {
        self.driver = Some(driver);
        self.liveness = SessionLiveness::Live;
    }

    /// Clears the driver handle only on explicit disconnect/teardown (never on outage).
    pub fn clear_driver(&mut self) {
        self.driver = None;
        self.liveness = SessionLiveness::Live;
        self.transaction_state = tablepro_core::TransactionState::Idle;
    }

    /// Returns a reference to the driver handle (remains `Some` even during outages).
    #[must_use]
    pub fn driver(&self) -> Option<&DriverHandle> {
        self.driver.as_ref()
    }

    /// Marks the session unreachable without clearing the driver handle.
    pub fn mark_session_unreachable(&mut self, started_with: u64, info: impl Into<String>) {
        self.liveness = SessionLiveness::Unreachable {
            started_with,
            info: info.into(),
        };
    }

    /// Marks the session live again.
    pub fn mark_session_live(&mut self) {
        self.liveness = SessionLiveness::Live;
    }

    /// Returns the raw liveness state.
    #[must_use]
    pub fn liveness(&self) -> &SessionLiveness {
        &self.liveness
    }

    /// Returns the UI-facing status derived from `liveness` (and whether a driver is installed).
    #[must_use]
    pub fn reported_status(&self) -> ReportedStatus {
        if self.driver.is_none() {
            return ReportedStatus::Disconnected;
        }
        match self.liveness {
            SessionLiveness::Live => ReportedStatus::Connected,
            SessionLiveness::Unreachable { .. } => ReportedStatus::Unreachable,
        }
    }

    /// Evaluates SafeMode gating for a statement access classification.
    #[must_use]
    pub fn gate_statement(
        &self,
        access: tablepro_core::sql_syntax::StatementAccess,
    ) -> tablepro_core::WriteGate {
        self.safe_mode.gate(access)
    }

    /// Evaluates whether closing a tab or window requires an open-transaction warning.
    #[must_use]
    pub fn check_close_guard(&self) -> CloseDecision {
        CloseDecision::for_transaction(self.transaction_state)
    }
}

/// Decision produced by the open-transaction close guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    Allow,
    WarnOpenTransaction,
    WarnFailedTransaction,
    WarnUnknownTransaction,
}

impl CloseDecision {
    /// Determines the close decision for a given [`tablepro_core::TransactionState`].
    #[must_use]
    pub const fn for_transaction(state: tablepro_core::TransactionState) -> Self {
        match state {
            tablepro_core::TransactionState::Idle => Self::Allow,
            tablepro_core::TransactionState::InTransaction => Self::WarnOpenTransaction,
            tablepro_core::TransactionState::Failed => Self::WarnFailedTransaction,
            tablepro_core::TransactionState::Unknown => Self::WarnUnknownTransaction,
        }
    }

    /// Whether the close requires user confirmation before proceeding.
    #[must_use]
    pub const fn requires_confirmation(self) -> bool {
        !matches!(self, Self::Allow)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use tablepro_core::sql_syntax::StatementAccess;
    use tablepro_core::{SafeModeLevel, TransactionState, WriteGate};

    use super::*;

    #[test]
    fn cancel_fences_late_driver_completion_and_failure() {
        let mut machine = ConnectionWindowPhaseMachine::new();
        let ws = WorkspaceId(1);
        machine.add_workspace(ws);

        let token1 = machine.begin_connect(ws);
        assert!(token1.is_some());
        if let Some(t1) = token1 {
            machine.cancel_connect(ws);
            assert_eq!(
                machine.phase(ws),
                Some(&ConnectionWindowPhase::Disconnected)
            );

            // Late completion or failure with t1 must be ignored.
            assert!(!machine.complete_connect(ws, t1));
            assert!(!machine.fail_connect(
                ws,
                t1,
                ConnectionUnavailableView {
                    summary: "late".to_owned(),
                    detail: None,
                    can_retry: true,
                    is_auth_failure: false,
                }
            ));
            assert_eq!(
                machine.phase(ws),
                Some(&ConnectionWindowPhase::Disconnected)
            );
        }
    }

    #[test]
    fn close_window_cancels_all_workspaces() {
        let mut machine = ConnectionWindowPhaseMachine::new();
        let w1 = WorkspaceId(1);
        let w2 = WorkspaceId(2);
        machine.add_workspace(w1);
        machine.add_workspace(w2);

        let t1 = machine.begin_connect(w1);
        let t2 = machine.begin_connect(w2);
        machine.close_window();

        assert_eq!(
            machine.phase(w1),
            Some(&ConnectionWindowPhase::Disconnected)
        );
        assert_eq!(
            machine.phase(w2),
            Some(&ConnectionWindowPhase::Disconnected)
        );
        if let (Some(t1), Some(t2)) = (t1, t2) {
            assert!(!machine.complete_connect(w1, t1));
            assert!(!machine.complete_connect(w2, t2));
        }
    }

    #[test]
    fn session_keeps_driver_handle_across_outages_and_gates_safemode() {
        let mut session = ConnectionSession::new(SafeModeLevel::ConfirmWrites);
        assert_eq!(session.reported_status(), ReportedStatus::Disconnected);

        session.install_driver(DriverHandle {
            driver_id: "postgres".to_owned(),
            generation: 1,
        });
        assert_eq!(session.reported_status(), ReportedStatus::Connected);

        session.mark_session_unreachable(1, "network reset");
        assert_eq!(session.reported_status(), ReportedStatus::Unreachable);
        assert!(
            session.driver().is_some(),
            "driver handle must not be cleared on outage"
        );

        session.mark_session_live();
        assert_eq!(session.reported_status(), ReportedStatus::Connected);

        assert_eq!(
            session.gate_statement(StatementAccess::Read),
            WriteGate::Run
        );
        assert_eq!(
            session.gate_statement(StatementAccess::Write),
            WriteGate::Confirm
        );

        session.transaction_state = TransactionState::InTransaction;
        assert!(session.check_close_guard().requires_confirmation());
    }

    #[test]
    fn title_and_tab_label_resolvers_never_return_blank() {
        let title =
            WindowTitleResolver::resolve("   ", Some(""), &ConnectionWindowPhase::Disconnected);
        assert!(!title.trim().is_empty());

        let label = EditorTabLabelResolver::resolve(Some("   "), 0, true);
        assert_eq!(label, "Query 1 •");
    }

    proptest! {
        #[test]
        fn prop_stale_tokens_never_mutate_phase(ops in prop::collection::vec(0u8..4, 1..40)) {
            let mut machine = ConnectionWindowPhaseMachine::new();
            let ws = WorkspaceId(7);
            machine.add_workspace(ws);
            let mut stale_tokens = Vec::new();
            let mut active_token: Option<AttemptToken> = None;

            for op in ops {
                match op {
                    0 => {
                        if let Some(prev) = active_token {
                            stale_tokens.push(prev);
                        }
                        active_token = machine.begin_connect(ws);
                    }
                    1 => {
                        if let Some(prev) = active_token.take() {
                            stale_tokens.push(prev);
                        }
                        machine.cancel_connect(ws);
                    }
                    2 => {
                        if let Some(tok) = active_token.take() {
                            prop_assert!(machine.complete_connect(ws, tok));
                        }
                    }
                    _ => {
                        for &stale in &stale_tokens {
                            let before = machine.phase(ws).cloned();
                            prop_assert!(!machine.complete_connect(ws, stale));
                            prop_assert_eq!(machine.phase(ws).cloned(), before);
                        }
                    }
                }
            }
        }
    }
}
