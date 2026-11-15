//! Live SQL Editor execution (`run_query` streaming `ResultEvent`s),
//! saved queries, query history, schema-aware autocomplete, Vim mode, and split panes.

use std::time::Instant;

use tableminus_gfx::editor_renderer::{
    AutocompleteItem, AutocompletePopup, CompletionKind, CursorMode, EditorLine,
};
use tableminus_services::{
    DatabaseEngine, ExplainNode, FindReplaceOptions, find_matches, format_sql,
    parse_explain_output, replace_all_matches,
};
use tablepro_core::column::ResultColumn;
use tablepro_core::value::Value;
use tablepro_core::{EditorSessionOptions, ResultEvent, RunOptions, RunSummary};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::app::AppController;
use crate::controller_grid::format_value_cell;
use crate::controller_types::{QueryHistoryEntry, SavedQueryEntry, SplitPaneMode, VimMode};
use crate::error::{AppError, Result};

impl AppController {
    /// Sets the SQL editor text from `sql`.
    pub fn set_editor_sql(&mut self, sql: &str) {
        self.editor_lines = sql
            .lines()
            .enumerate()
            .map(|(i, line)| EditorLine::new(line, (i + 1) as u64))
            .collect();
        if self.editor_lines.is_empty() {
            self.editor_lines.push(EditorLine::new("", 1));
        }
        if let Some(cursor) = self.editor_cursors.first_mut() {
            let last_line = self.editor_lines.len().saturating_sub(1);
            cursor.line = last_line;
            cursor.col = self.editor_lines[last_line].text.len();
        }
    }

    /// Returns the full SQL text currently in the editor.
    #[must_use]
    pub fn editor_sql_text(&self) -> String {
        self.editor_lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Executes the active SQL editor buffer synchronously via `SessionManager::run_query`,
    /// streaming [`ResultEvent`]s into `grid_model` and recording query history.
    pub fn execute_active_sql_blocking(&mut self) -> Result<RunSummary> {
        let sql = self.editor_sql_text();
        self.execute_sql_blocking(&sql)
    }

    /// Executes `sql` via `SessionManager::run_query` and streams all [`ResultEvent`]s.
    pub fn execute_sql_blocking(&mut self, sql: &str) -> Result<RunSummary> {
        let services = self.require_services()?.clone();
        let ws = self.active_workspace;
        let cancel = CancellationToken::new();
        let options = RunOptions::new(cancel.clone());
        let editor_options = EditorSessionOptions {
            statement_timeout: None,
            database: None,
        };
        let (tx, mut rx) = mpsc::channel::<ResultEvent>(128);
        let started = Instant::now();

        self.query_notices.clear();
        let mut streamed_cols: Vec<ResultColumn> = Vec::new();
        let mut streamed_rows: Vec<Vec<Value>> = Vec::new();

        let summary_res = services.runtime.block_on(async {
            let guard = services.sessions.read().await;
            let res = guard
                .run_query(ws, sql, options, editor_options, tx, &cancel)
                .await;
            drop(guard);
            while let Some(ev) = rx.recv().await {
                match &ev {
                    ResultEvent::ResultSetStarted { columns }
                    | ResultEvent::ColumnsResolved { columns, .. } => {
                        streamed_cols = columns.to_vec();
                    }
                    ResultEvent::Rows(batch) => {
                        streamed_rows.extend(batch.iter().cloned());
                    }
                    _ => {}
                }
                self.apply_query_stream_event(ev);
            }
            res
        });

        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        if !streamed_cols.is_empty() || !streamed_rows.is_empty() {
            let total = Some(streamed_rows.len() as u64);
            self.grid_raw_columns = streamed_cols.clone();
            self.grid_raw_rows = streamed_rows.clone();
            self.grid_model
                .reload_rows(streamed_cols, streamed_rows, total);
            self.sync_grid_view_from_model();
        }

        match summary_res {
            Ok(summary) => {
                self.apply_query_finished(Ok(summary.clone()), elapsed_ms);
                self.record_query_history(sql, elapsed_ms, None, true);
                Ok(summary)
            }
            Err(err) => {
                self.apply_query_finished(Err(err.to_string()), elapsed_ms);
                self.record_query_history(sql, elapsed_ms, None, false);
                Err(err.into())
            }
        }
    }

    /// Processes a single streamed [`ResultEvent`] from an active query run.
    pub fn apply_query_stream_event(&mut self, event: ResultEvent) {
        match event {
            ResultEvent::ResultSetStarted { columns }
            | ResultEvent::ColumnsResolved { columns, .. } => {
                self.grid_raw_columns = columns.to_vec();
                self.grid_raw_rows.clear();
                self.grid_model
                    .reload_rows(columns.to_vec(), Vec::new(), None);
                self.sync_grid_view_from_model();
            }
            ResultEvent::Rows(batch) => {
                self.grid_raw_rows.extend(batch);
                self.sync_grid_view_from_model();
            }
            ResultEvent::ResultSetFinished { .. } => {}
            ResultEvent::Notice(notice) => {
                self.query_notices.push(notice.message);
            }
            ResultEvent::CommandCompleted {
                rows_affected: Some(n),
            } => {
                self.query_notices.push(format!("{n} row(s) affected"));
            }
            ResultEvent::RowLimitReached { retained } => {
                self.query_notices
                    .push(format!("Row limit reached ({retained} rows retained)"));
            }
            ResultEvent::ByteLimitReached { retained } => {
                self.query_notices
                    .push(format!("Byte limit reached ({retained} rows retained)"));
            }
            _ => {}
        }
    }

    /// Updates toolbar latency and notices when a query finishes.
    pub fn apply_query_finished(
        &mut self,
        summary: std::result::Result<RunSummary, String>,
        elapsed_ms: u64,
    ) {
        self.toolbar_status.latency_label = format!("{elapsed_ms}ms");
        if let Err(err) = summary {
            self.query_notices.push(format!("Query error: {err}"));
        }
    }

    /// Records an entry in `query_history`.
    pub fn record_query_history(
        &mut self,
        sql: &str,
        elapsed_ms: u64,
        rows_affected: Option<u64>,
        succeeded: bool,
    ) {
        self.query_history.push(QueryHistoryEntry {
            sql: sql.to_owned(),
            database: self.toolbar_status.database.clone(),
            elapsed_ms,
            rows_affected,
            succeeded,
        });
    }

    /// Saves the current SQL buffer as a named saved query.
    pub fn save_current_query(&mut self, name: &str, folder: Option<String>) {
        let sql = self.editor_sql_text();
        if let Some(existing) = self.saved_queries.iter_mut().find(|q| q.name == name) {
            existing.sql = sql;
            existing.folder = folder;
        } else {
            self.saved_queries.push(SavedQueryEntry {
                name: name.to_owned(),
                sql,
                folder,
            });
        }
    }

    /// Loads a saved query by name into the SQL editor.
    pub fn load_saved_query(&mut self, name: &str) -> bool {
        let Some(sql) = self
            .saved_queries
            .iter()
            .find(|q| q.name == name)
            .map(|q| q.sql.clone())
        else {
            return false;
        };
        self.set_editor_sql(&sql);
        true
    }

    /// Refreshes schema-aware autocomplete candidates for `prefix` from cached schemas,
    /// tables, and columns.
    pub fn refresh_editor_autocomplete(&mut self, prefix: &str) {
        let lower = prefix.trim().to_ascii_lowercase();
        let mut items = Vec::new();

        for obj in &self.cached_objects {
            if lower.is_empty() || obj.name.to_ascii_lowercase().starts_with(&lower) {
                items.push(AutocompleteItem {
                    label: obj.name.clone(),
                    detail: format!("{:?}", obj.kind),
                    kind: CompletionKind::Table,
                });
            }
        }
        for col in &self.structure_columns {
            if lower.is_empty() || col.name.to_ascii_lowercase().starts_with(&lower) {
                items.push(AutocompleteItem {
                    label: col.name.clone(),
                    detail: col.data_type.clone(),
                    kind: CompletionKind::Column,
                });
            }
        }

        if items.is_empty() {
            self.editor_autocomplete = None;
        } else {
            let anchor = self
                .editor_cursors
                .first()
                .map_or((0, 0), |c| (c.line, c.col));
            self.editor_autocomplete = Some(AutocompletePopup {
                visible: true,
                anchor,
                items,
                selected: 0,
            });
        }
    }

    /// Toggles Vim modal editing in the SQL editor.
    pub fn toggle_vim_mode(&mut self) {
        if self.vim_mode.is_some() {
            self.vim_mode = None;
            self.editor_cursor_mode = CursorMode::Bar;
        } else {
            self.vim_mode = Some(VimMode::Normal);
            self.editor_cursor_mode = CursorMode::VimBlock;
        }
    }

    /// Updates the active Vim mode (`Normal`, `Insert`, `Visual`, `Command`).
    pub fn set_vim_mode(&mut self, mode: VimMode) {
        self.vim_mode = Some(mode);
        self.editor_cursor_mode = match mode {
            VimMode::Insert => CursorMode::Bar,
            VimMode::Normal | VimMode::Visual | VimMode::Command => CursorMode::VimBlock,
        };
    }

    /// Cycles SQL editor split pane mode (`Single` -> `Horizontal` -> `Vertical`).
    pub fn toggle_split_pane(&mut self) {
        self.split_pane = match self.split_pane {
            SplitPaneMode::Single => {
                if self.split_editor_lines.is_empty() {
                    self.split_editor_lines = self.editor_lines.clone();
                }
                SplitPaneMode::Horizontal
            }
            SplitPaneMode::Horizontal => SplitPaneMode::Vertical,
            SplitPaneMode::Vertical => SplitPaneMode::Single,
        };
    }

    /// Formats the active SQL editor buffer using TableMinus's dialect-neutral SQL formatter.
    pub fn format_editor_sql(&mut self) {
        let formatted = format_sql(&self.editor_sql_text());
        self.set_editor_sql(&formatted);
    }

    /// Finds and replaces text in the active SQL editor buffer, returning the replacement count.
    pub fn find_replace_in_editor(
        &mut self,
        find: &str,
        replace: &str,
        case_sensitive: bool,
        whole_word: bool,
        replace_all: bool,
    ) -> usize {
        if find.is_empty() {
            return 0;
        }
        let text = self.editor_sql_text();
        let options = FindReplaceOptions {
            case_sensitive,
            whole_word,
        };
        if replace_all {
            let (updated, count) = replace_all_matches(&text, find, replace, options);
            if count > 0 {
                self.set_editor_sql(&updated);
            }
            return count;
        }
        let matches = find_matches(&text, find, options);
        let Some(first) = matches.first() else {
            return 0;
        };
        let mut updated = String::with_capacity(text.len() + replace.len());
        updated.push_str(&text[..first.start]);
        updated.push_str(replace);
        updated.push_str(&text[first.end..]);
        self.set_editor_sql(&updated);
        1
    }

    /// Executes an EXPLAIN / EXPLAIN QUERY PLAN query without mutating editor or grid state,
    /// parses the resulting plan tree, and stores it in `self.last_explain_nodes`.
    pub fn run_visual_explain_blocking(
        &mut self,
        sql: &str,
        analyze: bool,
    ) -> Result<Vec<ExplainNode>> {
        let trimmed = sql.trim();
        if trimmed.is_empty() {
            return Err(AppError::InvalidState(
                "cannot run EXPLAIN on an empty query".to_owned(),
            ));
        }
        let engine = self.active_database_engine();
        let explain_sql = build_explain_sql(engine, trimmed, analyze);
        let (cols, raw_rows) = self.execute_query_capture_blocking(&explain_sql)?;
        let col_names: Vec<String> = cols.iter().map(|c| c.name.clone()).collect();
        let str_rows: Vec<Vec<String>> = raw_rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| format_value_cell(cell).text)
                    .collect()
            })
            .collect();
        let nodes = parse_explain_output(engine, &col_names, &str_rows);
        self.last_explain_nodes.clone_from(&nodes);
        Ok(nodes)
    }
}

fn build_explain_sql(engine: DatabaseEngine, trimmed_sql: &str, analyze: bool) -> String {
    let already_explain = trimmed_sql
        .split_whitespace()
        .next()
        .is_some_and(|w| w.eq_ignore_ascii_case("EXPLAIN"));
    if already_explain {
        return trimmed_sql.to_owned();
    }
    let prefix = match engine {
        DatabaseEngine::Sqlite => "EXPLAIN QUERY PLAN ",
        DatabaseEngine::Postgres => {
            if analyze {
                "EXPLAIN (ANALYZE, FORMAT JSON) "
            } else {
                "EXPLAIN (FORMAT JSON) "
            }
        }
        DatabaseEngine::Mysql
        | DatabaseEngine::Duckdb
        | DatabaseEngine::Clickhouse
        | DatabaseEngine::Mssql
        | DatabaseEngine::Generic => {
            if analyze {
                "EXPLAIN ANALYZE "
            } else {
                "EXPLAIN "
            }
        }
    };
    format!("{prefix}{trimmed_sql}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tableminus_windows_platform::AppSettings;

    #[test]
    fn format_and_find_replace_editor_sql() {
        let mut controller = AppController::with_detected_theme(AppSettings::default());
        controller.set_editor_sql("select id, name from users where id = 1");
        controller.format_editor_sql();
        let formatted = controller.editor_sql_text();
        assert!(formatted.contains("SELECT"));
        assert!(formatted.contains("FROM"));

        let replaced_one =
            controller.find_replace_in_editor("users", "accounts", false, true, false);
        assert_eq!(replaced_one, 1);
        assert!(controller.editor_sql_text().contains("accounts"));

        controller.set_editor_sql("SELECT a, a_col FROM a");
        let replaced_all = controller.find_replace_in_editor("a", "b", true, true, true);
        assert_eq!(replaced_all, 2);
        assert_eq!(controller.editor_sql_text(), "SELECT b, a_col FROM b");
    }

    #[test]
    fn sqlite_visual_explain_populates_nodes_without_clobbering_grid()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let db_path = dir.path().join("explain_test.sqlite");
        std::fs::File::create(&db_path)?;

        let mut controller = AppController::with_detected_theme(AppSettings::default());
        controller.connect_local_file_blocking(&db_path)?;
        let _ = controller.execute_sql_script_blocking(
            "CREATE TABLE items (id INTEGER PRIMARY KEY, label TEXT);",
        )?;

        let nodes = controller.run_visual_explain_blocking("SELECT id, label FROM items", false)?;
        assert!(!nodes.is_empty());
        assert_eq!(controller.last_explain_nodes, nodes);
        assert!(controller.grid_raw_rows.is_empty());
        Ok(())
    }
}
