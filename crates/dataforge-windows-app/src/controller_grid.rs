//! Live DataGrid paging, sorting, filtering, inline cell edits (`TableEdits`),
//! Row Inspector sync, and atomic CSV/JSON/SQL export for [`AppController`].

use std::path::Path;

use dataforge_gfx::grid_renderer::{GridCell, GridColumn, GridRow, RowState};
use dataforge_services::export::{ExportFormat, ExportOptions, export_rows_atomic};
use dataforge_services::session_bridge::{SessionManager, TableQueryOutcome};
use dataforge_state::data_grid::{DataGridCellEdit, DataGridColumnID, RowID};
use dataforge_state::tabs::TabConnectionId;
use tablepro_core::ForeignKeyMode;
use tablepro_core::column::ResultColumn;
use tablepro_core::filter::{FilterOp, FilterRule, FilterValue};
use tablepro_core::meta::{PagePosition, TableRef};
use tablepro_core::value::Value;
use tokio_util::sync::CancellationToken;

use crate::app::{AppController, ScreenMode};
use crate::dialogs::{DialogKind, DialogOutcome, show_native_dialog};
use crate::error::{AppError, Result};
use crate::views::inspector::InspectorField;

impl AppController {
    /// Opens a table tab and loads its first page, row count, and structure columns.
    pub fn open_table_tab_blocking(&mut self, table: TableRef) -> Result<()> {
        let conn_id = TabConnectionId(format!("ws-{}", self.active_workspace.0));
        self.tabs.open_table_tab(conn_id, &table.name);
        self.active_table = Some(table.clone());
        self.screen_mode = ScreenMode::Workspace;
        self.sync_layout_counts();

        self.query_active_table_page_blocking(&PagePosition::First)?;
        let _ = self.refresh_table_row_count_blocking();
        let _ = self.load_table_structure_blocking(&table);
        Ok(())
    }

    /// Queries a page of `active_table` into `grid_model` and syncs the Direct2D grid view.
    pub fn query_active_table_page_blocking(&mut self, position: &PagePosition) -> Result<()> {
        let services = self.require_services()?.clone();
        let ws = self.active_workspace;
        let table = self
            .active_table
            .clone()
            .ok_or_else(|| AppError::InvalidState("no active table selected".to_owned()))?;
        let cancel = CancellationToken::new();

        let mut grid = std::mem::replace(
            &mut self.grid_model,
            dataforge_state::data_grid::DataGridModel::new(
                dataforge_state::data_grid::DataGridLayoutScope::Ephemeral,
                100,
            ),
        );
        let outcome_res = services.runtime.block_on(async {
            let guard = services.sessions.read().await;
            guard
                .table_query(ws, &table, &mut grid, position, &cancel)
                .await
        });
        self.grid_model = grid;
        let outcome = outcome_res?;
        self.apply_table_query_outcome(outcome);
        Ok(())
    }

    /// Refreshes the total row count for `active_table`.
    pub fn refresh_table_row_count_blocking(&mut self) -> Result<u64> {
        let services = self.require_services()?.clone();
        let ws = self.active_workspace;
        let table = self
            .active_table
            .clone()
            .ok_or_else(|| AppError::InvalidState("no active table selected".to_owned()))?;
        let filter = self.grid_model.filter_bar.filter_set.clone();
        let cancel = CancellationToken::new();

        let count = services.runtime.block_on(async {
            let guard = services.sessions.read().await;
            guard.table_row_count(ws, &table, &filter, &cancel).await
        })?;
        self.apply_table_row_count(count);
        Ok(count)
    }

    /// Applies a [`TableQueryOutcome`] to `grid_model`, `grid_columns`, `grid_rows`, and `status_bar`.
    pub fn apply_table_query_outcome(&mut self, outcome: TableQueryOutcome) {
        self.grid_page_prev = outcome.previous;
        self.grid_page_next = outcome.next;
        self.grid_row_keys = outcome.row_keys;
        self.grid_raw_columns = outcome.page.columns.to_vec();
        self.grid_raw_rows = outcome.page.rows;
        self.sync_grid_view_from_model();
    }

    /// Updates `grid_model.page.table_row_count` and status bar pagination totals.
    pub fn apply_table_row_count(&mut self, count: u64) {
        self.grid_model.page.table_row_count = Some(count);
        let limit = self.grid_model.page.limit.max(1);
        let total_pages = count.div_ceil(limit).max(1) as usize;
        self.status_bar.total_pages = total_pages;
    }

    /// Advances to the next page of `active_table` if available.
    pub fn next_grid_page_blocking(&mut self) -> Result<bool> {
        let Some(pos) = self.grid_page_next.clone() else {
            return Ok(false);
        };
        if self.grid_model.page.next_page() {
            self.status_bar.current_page = self.status_bar.current_page.saturating_add(1);
        }
        self.query_active_table_page_blocking(&pos)?;
        Ok(true)
    }

    /// Returns to the previous page of `active_table` if available.
    pub fn prev_grid_page_blocking(&mut self) -> Result<bool> {
        let Some(pos) = self.grid_page_prev.clone() else {
            return Ok(false);
        };
        if self.grid_model.page.prev_page() && self.status_bar.current_page > 1 {
            self.status_bar.current_page -= 1;
        }
        self.query_active_table_page_blocking(&pos)?;
        Ok(true)
    }

    /// Resizes a grid column in both `grid_model` and `grid_columns`.
    pub fn resize_grid_column(&mut self, col_idx: usize, width_dip: f32) {
        let clamped = width_dip.max(40.0);
        if let Some(col) = self.grid_model.columns.columns().get(col_idx) {
            let id = col.id;
            let _ = self.grid_model.columns.resize_column(id, f64::from(clamped));
        }
        if let Some(col) = self.grid_columns.get_mut(col_idx) {
            col.width = clamped;
        }
    }

    /// Toggles sorting on `col_idx` (server-side when connected to a table, client-side otherwise).
    pub fn toggle_grid_sort_blocking(&mut self, col_idx: usize) -> Result<()> {
        let id = self
            .grid_model
            .columns
            .columns()
            .get(col_idx)
            .map_or(DataGridColumnID(col_idx as u64), |c| c.id);
        self.grid_model.sort.click(id, false);
        if self.active_table.is_some() {
            self.query_active_table_page_blocking(&PagePosition::First)?;
        } else {
            self.grid_model.sort_client_rows();
            self.sync_grid_view_from_model();
        }
        Ok(())
    }

    /// Adds a filter rule on `column` and re-queries the active table.
    pub fn add_grid_filter_blocking(
        &mut self,
        column: &str,
        op: FilterOp,
        value: Option<String>,
    ) -> Result<()> {
        self.grid_model.filter_bar.add_rule(FilterRule {
            column: column.to_owned(),
            op,
            value: value.map(FilterValue::Single),
        });
        if self.active_table.is_some() {
            self.query_active_table_page_blocking(&PagePosition::First)?;
            let _ = self.refresh_table_row_count_blocking();
        }
        Ok(())
    }

    /// Clears all active grid filters and re-queries the active table.
    pub fn clear_grid_filters_blocking(&mut self) -> Result<()> {
        self.grid_model.filter_bar.clear();
        if self.active_table.is_some() {
            self.query_active_table_page_blocking(&PagePosition::First)?;
            let _ = self.refresh_table_row_count_blocking();
        }
        Ok(())
    }

    /// Records an inline cell edit in `grid_model.edits` (`TableEdits`) and marks tab unsaved.
    pub fn edit_grid_cell(&mut self, row_idx: usize, col_idx: usize, new_value: Value) {
        let Some(row_id) = self.grid_model.row_order.row_id(row_idx) else {
            return;
        };
        let Some(col_id) = self.grid_model.columns.columns().get(col_idx).map(|c| c.id) else {
            return;
        };
        let old_value = self
            .grid_raw_rows
            .get(row_idx)
            .and_then(|r| r.get(col_idx))
            .cloned()
            .unwrap_or(Value::Null);
        let edit = DataGridCellEdit {
            row_id,
            column_id: col_id,
            old_value,
            new_value,
        };
        self.grid_model.commit(vec![edit], "Cell Edit");
        self.mark_active_tab_unsaved(true);
        self.sync_grid_view_from_model();
    }

    /// Inserts a staged row into `grid_model.edits` (`TableEdits`).
    pub fn insert_grid_row(&mut self, values: Vec<Value>) -> RowID {
        let id = self.grid_model.edits.insert_row(values);
        self.mark_active_tab_unsaved(true);
        self.sync_grid_view_from_model();
        id
    }

    /// Marks `row_idx` for deletion in `grid_model.edits` (`TableEdits`).
    pub fn delete_grid_row(&mut self, row_idx: usize) {
        if let Some(row_id) = self.grid_model.row_order.row_id(row_idx) {
            self.grid_model.edits.delete_row(row_id);
            self.mark_active_tab_unsaved(true);
            self.sync_grid_view_from_model();
        }
    }

    /// Commits pending `TableEdits` to the active session (enforcing Safe Mode prompt if enabled).
    pub fn commit_grid_edits_blocking(&mut self) -> Result<u64> {
        if !self.grid_model.edits.has_pending_edits() {
            return Ok(0);
        }
        if self.safe_mode_enabled && !self.headless {
            let kind = DialogKind::SafeModeConfirmation {
                summary: "Commit pending DataGrid changes to database?".to_owned(),
            };
            let outcome = show_native_dialog(None, &kind, &self.strings);
            if !matches!(outcome, DialogOutcome::Accepted(_)) {
                return Err(AppError::InvalidState("commit cancelled in Safe Mode".to_owned()));
            }
        }
        let services = self.require_services()?.clone();
        let ws = self.active_workspace;
        let table = self
            .active_table
            .clone()
            .ok_or_else(|| AppError::InvalidState("no active table for commit".to_owned()))?;
        let cancel = CancellationToken::new();

        let mut grid = std::mem::replace(
            &mut self.grid_model,
            dataforge_state::data_grid::DataGridModel::new(
                dataforge_state::data_grid::DataGridLayoutScope::Ephemeral,
                100,
            ),
        );
        let row_keys = self.grid_row_keys.clone();
        let report_res = services.runtime.block_on(async {
            let guard = services.sessions.read().await;
            guard
                .commit_table_edits(
                    ws,
                    &table,
                    &mut grid,
                    &row_keys,
                    ForeignKeyMode::Enforced,
                    &cancel,
                )
                .await
        });
        self.grid_model = grid;
        let report = report_res?;
        self.clear_active_tab_unsaved();
        let _ = self.query_active_table_page_blocking(&PagePosition::First);
        let total_affected: u64 = report.rows_affected.iter().flatten().sum();
        Ok(total_affected)
    }

    /// Rolls back all uncommitted `TableEdits` in `grid_model`.
    pub fn rollback_grid_edits(&mut self) {
        SessionManager::rollback_table_edits(&mut self.grid_model);
        self.clear_active_tab_unsaved();
        self.sync_grid_view_from_model();
    }

    /// Selects `(row_idx, col_idx)` in the grid and syncs the Row Inspector panel.
    pub fn select_grid_cell(&mut self, row_idx: usize, col_idx: usize) {
        self.grid_selection.focused = Some((row_idx, col_idx));
        self.grid_selection.anchor = Some((row_idx, col_idx));
        self.sync_inspector_from_selection();
    }

    /// Populates `inspector_fields` from the currently focused row in `grid_rows`.
    pub fn sync_inspector_from_selection(&mut self) {
        let Some((row_idx, _)) = self.grid_selection.focused else {
            self.inspector_fields.clear();
            return;
        };
        let Some(row) = self.grid_rows.get(row_idx) else {
            self.inspector_fields.clear();
            return;
        };
        self.inspector_fields = self
            .grid_columns
            .iter()
            .zip(row.cells.iter())
            .map(|(col, cell)| {
                let is_null = matches!(cell.kind, dataforge_gfx::grid_renderer::CellKind::Null);
                InspectorField {
                    column: col.title.clone(),
                    data_type: String::new(),
                    value: if is_null {
                        "NULL".to_owned()
                    } else {
                        cell.text.clone()
                    },
                    is_null,
                }
            })
            .collect();
    }

    /// Exports the current grid columns and rows atomically to `path` in `format`.
    pub fn export_active_grid(&mut self, path: &Path, format: ExportFormat) -> Result<u64> {
        let cols: Vec<ResultColumn> = if !self.grid_raw_columns.is_empty() {
            self.grid_raw_columns.clone()
        } else {
            self.grid_columns
                .iter()
                .map(|c| {
                    ResultColumn::new(
                        &c.title,
                        tablepro_core::column::ColumnType::untyped_text("TEXT"),
                    )
                })
                .collect()
        };
        let options = ExportOptions {
            format,
            table: self.active_table.clone(),
            ..ExportOptions::default()
        };
        let count = export_rows_atomic(path, &cols, &self.grid_raw_rows, &options)?;
        self.last_export_status = Some(format!("Exported {count} rows to {}", path.display()));
        Ok(count)
    }

    /// Synchronizes `grid_columns` and `grid_rows` from `grid_model` and its pending `TableEdits`.
    pub fn sync_grid_view_from_model(&mut self) {
        self.grid_columns = self
            .grid_model
            .columns
            .columns()
            .iter()
            .map(|c| GridColumn::new(&c.name, c.width as f32))
            .collect();

        let mut rows = Vec::with_capacity(self.grid_raw_rows.len());
        for (idx, values) in self.grid_raw_rows.iter().enumerate() {
            let row_id = self.grid_model.row_order.row_id(idx);
            let is_deleted = row_id.is_some_and(|id| self.grid_model.edits.deleted_rows.contains(&id));
            let mut has_modified = false;
            let cells: Vec<GridCell> = values
                .iter()
                .enumerate()
                .map(|(col_idx, val)| {
                    let effective = if let (Some(rid), Some(col)) =
                        (row_id, self.grid_model.columns.columns().get(col_idx))
                        && let Some(edited) = self.grid_model.edits.modified_cells.get(&(rid, col.id))
                    {
                        has_modified = true;
                        edited
                    } else {
                        val
                    };
                    format_value_cell(effective)
                })
                .collect();
            let state = if is_deleted {
                RowState::Deleted
            } else if has_modified {
                RowState::Modified
            } else {
                RowState::Clean
            };
            rows.push(GridRow {
                row_number: idx + 1,
                state,
                cells,
            });
        }

        for inserted in self.grid_model.edits.inserted_rows.values() {
            let row_number = rows.len() + 1;
            rows.push(GridRow {
                row_number,
                state: RowState::Inserted,
                cells: inserted.iter().map(format_value_cell).collect(),
            });
        }

        self.grid_rows = rows;
        self.sync_inspector_from_selection();
    }

    pub(crate) fn clear_active_tab_unsaved(&mut self) {
        self.mark_active_tab_unsaved(false);
    }

    fn mark_active_tab_unsaved(&mut self, unsaved: bool) {
        self.toolbar_status.latency_label = if unsaved {
            "Uncommitted edits".to_owned()
        } else {
            String::new()
        };
    }
}

fn format_value_cell(val: &Value) -> GridCell {
    match val {
        Value::Null => GridCell::null(),
        Value::Bool(b) => GridCell::text(if *b { "true" } else { "false" }),
        Value::Int32(i) => GridCell::text(i.to_string()),
        Value::Int(i) => GridCell::text(i.to_string()),
        Value::UInt(u) => GridCell::text(u.to_string()),
        Value::Float32(f) => GridCell::text(f.to_string()),
        Value::Float64(f) => GridCell::text(f.to_string()),
        Value::Text(s) => GridCell::text(s.as_str()),
        Value::Bytes(bytes) => GridCell::text(format!("<{} bytes>", bytes.len())),
        other => GridCell::text(format!("{other:?}")),
    }
}
