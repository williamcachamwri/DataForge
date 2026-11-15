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

/// Summary aggregate statistics computed over selected grid cells.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GridAggregateStats {
    pub count: usize,
    pub non_null_count: usize,
    pub numeric_count: usize,
    pub sum: f64,
    pub avg: f64,
    pub min: f64,
    pub max: f64,
}

const MAX_TASK_HISTORY_ENTRIES: usize = 50;

fn current_unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn split_export_task_spec(spec: &str) -> Option<(&str, &str)> {
    let trimmed = spec.trim();
    for sep in ["=>", "|"] {
        if let Some((sql_part, path_part)) = trimmed.split_once(sep) {
            let sql = sql_part.trim();
            let path = path_part.trim();
            if !sql.is_empty() && !path.is_empty() {
                return Some((sql, path));
            }
        }
    }
    None
}

impl crate::app::AppController {
    /// Loads persisted scheduled tasks from the default platform path (skipped when headless).
    pub fn load_scheduled_tasks(&mut self) {
        if self.headless {
            return;
        }
        if let Ok(tasks) = dataforge_windows_platform::load_scheduled_tasks(None) {
            self.scheduled_tasks = tasks;
        }
    }

    /// Loads persisted scheduled tasks from an explicit `path`.
    pub fn load_scheduled_tasks_from_path(&mut self, path: &std::path::Path) {
        if let Ok(tasks) = dataforge_windows_platform::load_scheduled_tasks(Some(path)) {
            self.scheduled_tasks = tasks;
        }
    }

    /// Saves `self.scheduled_tasks` to the default platform path (no-op when headless).
    pub fn save_scheduled_tasks_now(&self) -> crate::error::Result<()> {
        if self.headless {
            return Ok(());
        }
        dataforge_windows_platform::save_scheduled_tasks(None, &self.scheduled_tasks)?;
        Ok(())
    }

    /// Saves `self.scheduled_tasks` to an explicit `path`.
    pub fn save_scheduled_tasks_to_path(&self, path: &std::path::Path) -> crate::error::Result<()> {
        dataforge_windows_platform::save_scheduled_tasks(Some(path), &self.scheduled_tasks)?;
        Ok(())
    }

    /// Inserts or updates a scheduled task by ID and persists the task list.
    pub fn upsert_scheduled_task(&mut self, task: dataforge_windows_platform::ScheduledTaskRecord) {
        if let Some(existing) = self.scheduled_tasks.iter_mut().find(|t| t.id == task.id) {
            *existing = task;
        } else {
            self.scheduled_tasks.push(task);
        }
        let _ = self.save_scheduled_tasks_now();
    }

    /// Enables or disables the scheduled task with `id`, returning `true` if found.
    pub fn toggle_scheduled_task(&mut self, id: &str, enabled: bool) -> bool {
        let Some(task) = self.scheduled_tasks.iter_mut().find(|t| t.id == id) else {
            return false;
        };
        task.enabled = enabled;
        let _ = self.save_scheduled_tasks_now();
        true
    }

    /// Deletes the scheduled task with `id`, returning `true` if removed.
    pub fn delete_scheduled_task(&mut self, id: &str) -> bool {
        let before = self.scheduled_tasks.len();
        self.scheduled_tasks.retain(|t| t.id != id);
        if self.scheduled_tasks.len() < before {
            let _ = self.save_scheduled_tasks_now();
            true
        } else {
            false
        }
    }

    /// Executes the scheduled task identified by `id` immediately and records its run history.
    pub fn run_scheduled_task_now(
        &mut self,
        id: &str,
    ) -> crate::error::Result<dataforge_windows_platform::ScheduledTaskRunEntry> {
        let task = self
            .scheduled_tasks
            .iter()
            .find(|t| t.id == id)
            .cloned()
            .ok_or_else(|| {
                crate::error::AppError::InvalidState(format!("scheduled task '{id}' not found"))
            })?;

        let started = std::time::Instant::now();
        let started_unix = current_unix_timestamp();
        let outcome = self.execute_scheduled_task_body(&task);
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

        let entry = match &outcome {
            Ok(summary) => dataforge_windows_platform::ScheduledTaskRunEntry {
                ran_at_unix: started_unix,
                success: true,
                summary: summary.clone(),
                duration_ms,
            },
            Err(err) => dataforge_windows_platform::ScheduledTaskRunEntry {
                ran_at_unix: started_unix,
                success: false,
                summary: err.to_string(),
                duration_ms,
            },
        };

        self.record_scheduled_task_run(id, &entry);
        let _ = self.save_scheduled_tasks_now();

        outcome.map(|_| entry)
    }

    /// Evaluates due scheduled tasks against the current time and executes each one.
    pub fn tick_due_scheduled_tasks(
        &mut self,
    ) -> Vec<dataforge_windows_platform::ScheduledTaskRunEntry> {
        let now_unix = current_unix_timestamp();
        let due_indices =
            dataforge_services::evaluate_due_task_indices(&self.scheduled_tasks, now_unix, |t| {
                (t.enabled, t.last_run_unix, t.interval_minutes)
            });
        let due_ids: Vec<String> = due_indices
            .into_iter()
            .filter_map(|idx| self.scheduled_tasks.get(idx).map(|t| t.id.clone()))
            .collect();

        let mut results = Vec::with_capacity(due_ids.len());
        for task_id in due_ids {
            match self.run_scheduled_task_now(&task_id) {
                Ok(entry) => results.push(entry),
                Err(err) => results.push(dataforge_windows_platform::ScheduledTaskRunEntry {
                    ran_at_unix: now_unix,
                    success: false,
                    summary: err.to_string(),
                    duration_ms: 0,
                }),
            }
        }
        results
    }

    fn execute_scheduled_task_body(
        &mut self,
        task: &dataforge_windows_platform::ScheduledTaskRecord,
    ) -> crate::error::Result<String> {
        if !task.connection_id.is_empty()
            && self.active_connection_id() != Some(task.connection_id.as_str())
            && self.library.storage().connections.contains_key(
                &dataforge_state::library::LibraryConnectionId(task.connection_id.clone()),
            )
        {
            self.connect_library_item_blocking(&task.connection_id)?;
        }

        match task.task_kind.as_str() {
            "sql" => {
                let (stmts, rows) = self.execute_sql_script_blocking(&task.sql_or_target)?;
                Ok(format!(
                    "Executed {stmts} statement(s), {rows} row(s) affected"
                ))
            }
            "export" => {
                let (sql, target_str) =
                    split_export_task_spec(&task.sql_or_target).ok_or_else(|| {
                        crate::error::AppError::InvalidState(
                            "export task requires '<sql> => <output_path>'".to_owned(),
                        )
                    })?;
                let (cols, rows) = self.execute_query_capture_blocking(sql)?;
                let target_path = std::path::Path::new(target_str);
                let opts = dataforge_services::export::ExportOptions::default();
                let count = dataforge_services::export::export_rows_atomic(
                    target_path,
                    &cols,
                    &rows,
                    &opts,
                )?;
                Ok(format!(
                    "Exported {count} row(s) to {}",
                    target_path.display()
                ))
            }
            "schema_compare" => {
                let defs = self.fetch_all_table_definitions_blocking()?;
                let total_cols: usize = defs.iter().map(|d| d.columns.len()).sum();
                Ok(format!(
                    "Captured {} table definition(s) ({total_cols} columns)",
                    defs.len()
                ))
            }
            other => Err(crate::error::AppError::InvalidState(format!(
                "unsupported scheduled task kind '{other}'"
            ))),
        }
    }

    fn record_scheduled_task_run(
        &mut self,
        id: &str,
        entry: &dataforge_windows_platform::ScheduledTaskRunEntry,
    ) {
        if let Some(task) = self.scheduled_tasks.iter_mut().find(|t| t.id == id) {
            task.last_run_unix = Some(entry.ran_at_unix);
            task.last_status = Some(entry.summary.clone());
            task.last_duration_ms = Some(entry.duration_ms);
            task.run_history.push(entry.clone());
            if task.run_history.len() > MAX_TASK_HISTORY_ENTRIES {
                let drain = task.run_history.len() - MAX_TASK_HISTORY_ENTRIES;
                task.run_history.drain(..drain);
            }
        }
    }
}
