//! Live DataGrid paging, sorting, filtering, inline cell edits (`TableEdits`),
//! Row Inspector sync, and atomic CSV/JSON/SQL export for [`AppController`].

use std::path::Path;

use tableminus_gfx::grid_renderer::{CellKind, GridCell, GridColumn, GridRow, RowState};
use tableminus_services::export::{ExportFormat, ExportOptions, export_rows_atomic};
use tableminus_services::session_bridge::{SessionManager, TableQueryOutcome};
use tableminus_services::{ColumnMapping, generate_import_sql_batches};
use tableminus_state::data_grid::{DataGridCellEdit, DataGridColumnID, RowID};
use tableminus_state::tabs::TabConnectionId;
use tablepro_core::ForeignKeyMode;
use tablepro_core::column::ResultColumn;
use tablepro_core::export::{TSV_NULL, render_tsv};
use tablepro_core::filter::{FilterOp, FilterRule, FilterValue};
use tablepro_core::meta::{PagePosition, TableRef};
use tablepro_core::value::Value;
use tokio_util::sync::CancellationToken;

use crate::app::{AppController, ScreenMode};
use crate::controller_types::GridAggregateStats;
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
            tableminus_state::data_grid::DataGridModel::new(
                tableminus_state::data_grid::DataGridLayoutScope::Ephemeral,
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
            let _ = self
                .grid_model
                .columns
                .resize_column(id, f64::from(clamped));
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
                return Err(AppError::InvalidState(
                    "commit cancelled in Safe Mode".to_owned(),
                ));
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
            tableminus_state::data_grid::DataGridModel::new(
                tableminus_state::data_grid::DataGridLayoutScope::Ephemeral,
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
                let is_null = matches!(cell.kind, tableminus_gfx::grid_renderer::CellKind::Null);
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
            let is_deleted =
                row_id.is_some_and(|id| self.grid_model.edits.deleted_rows.contains(&id));
            let mut has_modified = false;
            let cells: Vec<GridCell> = values
                .iter()
                .enumerate()
                .map(|(col_idx, val)| {
                    let effective = if let (Some(rid), Some(col)) =
                        (row_id, self.grid_model.columns.columns().get(col_idx))
                        && let Some(edited) =
                            self.grid_model.edits.modified_cells.get(&(rid, col.id))
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

    /// Toggles pinned state for `column_name` in the DataGrid header.
    pub fn toggle_pin_column(&mut self, column_name: &str) {
        if !self.grid_pinned_columns.remove(column_name) {
            self.grid_pinned_columns.insert(column_name.to_owned());
        }
    }

    /// Returns `true` if `column_name` is currently pinned in the DataGrid.
    #[must_use]
    pub fn is_column_pinned(&self, column_name: &str) -> bool {
        self.grid_pinned_columns.contains(column_name)
    }

    /// Computes count, non-null count, numeric count, sum, average, min, and max
    /// across `selected_rows` (for `selected_col`, or all columns when `None`).
    #[must_use]
    pub fn compute_grid_aggregates(
        &self,
        selected_rows: &[usize],
        selected_col: Option<usize>,
    ) -> GridAggregateStats {
        let mut stats = GridAggregateStats::default();
        if selected_rows.is_empty() {
            return stats;
        }
        let col_count = self
            .grid_raw_columns
            .len()
            .max(self.grid_columns.len())
            .max(self.grid_rows.first().map_or(0, |r| r.cells.len()));
        if col_count == 0 {
            return stats;
        }
        let col_indices: Vec<usize> = match selected_col {
            Some(c) if c < col_count => vec![c],
            Some(_) => return stats,
            None => (0..col_count).collect(),
        };

        let mut min_val = f64::INFINITY;
        let mut max_val = f64::NEG_INFINITY;
        for &row_idx in selected_rows {
            for &col_idx in &col_indices {
                self.accumulate_cell_aggregate(
                    row_idx,
                    col_idx,
                    &mut stats,
                    &mut min_val,
                    &mut max_val,
                );
            }
        }
        if stats.numeric_count > 0 {
            stats.avg = stats.sum / (stats.numeric_count as f64);
            stats.min = min_val;
            stats.max = max_val;
        }
        stats
    }

    fn accumulate_cell_aggregate(
        &self,
        row_idx: usize,
        col_idx: usize,
        stats: &mut GridAggregateStats,
        min_val: &mut f64,
        max_val: &mut f64,
    ) {
        if let Some(raw_row) = self.grid_raw_rows.get(row_idx) {
            let row_id = self.grid_model.row_order.row_id(row_idx);
            let col_id = self.grid_model.columns.columns().get(col_idx).map(|c| c.id);
            let edited = row_id
                .zip(col_id)
                .and_then(|key| self.grid_model.edits.modified_cells.get(&key));
            let Some(val) = edited.or_else(|| raw_row.get(col_idx)) else {
                return;
            };
            stats.count += 1;
            if !matches!(val, Value::Null) {
                stats.non_null_count += 1;
                if let Some(num) = value_as_finite_f64(val) {
                    stats.numeric_count += 1;
                    stats.sum += num;
                    *min_val = min_val.min(num);
                    *max_val = max_val.max(num);
                }
            }
            return;
        }

        if let Some(cell) = self
            .grid_rows
            .get(row_idx)
            .and_then(|r| r.cells.get(col_idx))
        {
            stats.count += 1;
            if cell.kind != CellKind::Null {
                stats.non_null_count += 1;
                if let Ok(num) = cell.text.trim().parse::<f64>()
                    && num.is_finite()
                {
                    stats.numeric_count += 1;
                    stats.sum += num;
                    *min_val = min_val.min(num);
                    *max_val = max_val.max(num);
                }
            }
        }
    }

    /// Formats `row_indices` as tab-separated values (`TSV`) for clipboard copy.
    #[must_use]
    pub fn format_rows_as_tsv(&self, row_indices: &[usize]) -> String {
        if row_indices.is_empty() {
            return String::new();
        }
        if !self.grid_raw_columns.is_empty() && !self.grid_raw_rows.is_empty() {
            let rows: Vec<Vec<Value>> = row_indices
                .iter()
                .filter_map(|&idx| {
                    let raw = self.grid_raw_rows.get(idx)?;
                    let row_id = self.grid_model.row_order.row_id(idx);
                    let mut row = raw.clone();
                    for (col_idx, cell) in row.iter_mut().enumerate() {
                        let col_id = self.grid_model.columns.columns().get(col_idx).map(|c| c.id);
                        if let Some(key) = row_id.zip(col_id)
                            && let Some(edit) = self.grid_model.edits.modified_cells.get(&key)
                        {
                            *cell = edit.clone();
                        }
                    }
                    Some(row)
                })
                .collect();
            if let Ok(rendered) = render_tsv(&self.grid_raw_columns, &rows, false) {
                return rendered;
            }
        }

        let mut out = String::new();
        for &idx in row_indices {
            let Some(row) = self.grid_rows.get(idx) else {
                continue;
            };
            for (c_idx, cell) in row.cells.iter().enumerate() {
                if c_idx > 0 {
                    out.push('\t');
                }
                if cell.kind == CellKind::Null {
                    out.push_str(TSV_NULL);
                } else {
                    push_tsv_field(&mut out, &cell.text);
                }
            }
            out.push('\n');
        }
        out
    }

    /// Parses clipboard `tsv` rows and inserts them into `self.active_table`.
    pub fn paste_tsv_into_active_table(&mut self, tsv: &str) -> Result<usize> {
        let table = self
            .active_table
            .clone()
            .ok_or_else(|| AppError::InvalidState("no active table selected".to_owned()))?;
        let parsed = parse_tsv_records(tsv);
        if parsed.is_empty() {
            return Ok(0);
        }

        let table_cols = self.active_table_column_names();
        if table_cols.is_empty() {
            return Err(AppError::InvalidState(
                "active table has no resolved columns for TSV paste".to_owned(),
            ));
        }

        let (mappings, data_rows) = build_tsv_column_mappings(&parsed, &table_cols);
        if data_rows.is_empty() || mappings.is_empty() {
            return Ok(0);
        }

        let engine = self.active_database_engine();
        let batches = generate_import_sql_batches(
            engine,
            table.schema.as_deref(),
            &table.name,
            false,
            &mappings,
            &data_rows,
            100,
        );
        for sql in &batches {
            let _ = self.run_sql_capture_raw(sql)?;
        }
        let _ = self.refresh_table_row_count_blocking();
        let _ = self.query_active_table_page_blocking(&PagePosition::First);
        Ok(data_rows.len())
    }

    fn active_table_column_names(&self) -> Vec<(String, String)> {
        if !self.grid_raw_columns.is_empty() {
            return self
                .grid_raw_columns
                .iter()
                .map(|c| (c.name.clone(), c.column_type.name.clone()))
                .collect();
        }
        if !self.structure_columns.is_empty() {
            return self
                .structure_columns
                .iter()
                .map(|c| (c.name.clone(), c.data_type.clone()))
                .collect();
        }
        self.grid_columns
            .iter()
            .map(|c| (c.title.clone(), "TEXT".to_owned()))
            .collect()
    }
}

fn value_as_finite_f64(val: &Value) -> Option<f64> {
    let parsed = match val {
        Value::Int32(i) => f64::from(*i),
        Value::Int(i) => *i as f64,
        Value::UInt(u) => *u as f64,
        Value::Float32(f) => f64::from(*f),
        Value::Float64(f) => *f,
        Value::Text(s) => s.trim().parse::<f64>().ok()?,
        Value::Null | Value::Bool(_) | Value::Bytes(_) => return None,
        other => format_value_cell(other).text.trim().parse::<f64>().ok()?,
    };
    parsed.is_finite().then_some(parsed)
}

fn push_tsv_field(out: &mut String, field: &str) {
    if field.contains(['\t', '\n', '\r', '"']) {
        out.push('"');
        for ch in field.chars() {
            if ch == '"' {
                out.push_str("\"\"");
            } else {
                out.push(ch);
            }
        }
        out.push('"');
    } else {
        out.push_str(field);
    }
}

fn parse_tsv_records(tsv: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut current_row = Vec::new();
    let mut field = String::new();
    let mut chars = tsv.chars().peekable();
    let mut in_quotes = false;
    let mut was_quoted = false;

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    let _ = chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
            continue;
        }

        match ch {
            '"' if field.is_empty() => {
                in_quotes = true;
                was_quoted = true;
            }
            '\t' => {
                current_row.push(normalize_tsv_cell(&field, was_quoted));
                field.clear();
                was_quoted = false;
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    let _ = chars.next();
                }
                current_row.push(normalize_tsv_cell(&field, was_quoted));
                rows.push(std::mem::take(&mut current_row));
                field.clear();
                was_quoted = false;
            }
            '\n' => {
                current_row.push(normalize_tsv_cell(&field, was_quoted));
                rows.push(std::mem::take(&mut current_row));
                field.clear();
                was_quoted = false;
            }
            other => field.push(other),
        }
    }

    if !field.is_empty() || !current_row.is_empty() {
        current_row.push(normalize_tsv_cell(&field, was_quoted));
        rows.push(current_row);
    }
    rows.retain(|r| !(r.len() == 1 && r[0].is_empty()));
    rows
}

fn normalize_tsv_cell(raw: &str, was_quoted: bool) -> String {
    if !was_quoted && raw == TSV_NULL {
        String::new()
    } else {
        raw.to_owned()
    }
}

fn build_tsv_column_mappings(
    parsed: &[Vec<String>],
    table_cols: &[(String, String)],
) -> (Vec<ColumnMapping>, Vec<Vec<String>>) {
    let first = &parsed[0];
    let has_header = !first.is_empty()
        && first.iter().all(|h| {
            table_cols
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(h.trim()))
        });

    if has_header {
        let mappings = first
            .iter()
            .enumerate()
            .filter_map(|(idx, header)| {
                let (col_name, col_type) = table_cols
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(header.trim()))?;
                Some(ColumnMapping {
                    source_index: idx,
                    target_column: col_name.clone(),
                    target_type: col_type.clone(),
                    include: true,
                })
            })
            .collect();
        return (mappings, parsed[1..].to_vec());
    }

    let col_limit = first.len().min(table_cols.len());
    let mappings = (0..col_limit)
        .map(|idx| ColumnMapping {
            source_index: idx,
            target_column: table_cols[idx].0.clone(),
            target_type: table_cols[idx].1.clone(),
            include: true,
        })
        .collect();
    (mappings, parsed.to_vec())
}

pub(crate) fn format_value_cell(val: &Value) -> GridCell {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tableminus_windows_platform::{AppSettings, ScheduledTaskRecord};

    #[test]
    fn grid_aggregates_pin_and_tsv_formatting()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let db_path = dir.path().join("grid_test.sqlite");
        std::fs::File::create(&db_path)?;

        let mut controller = AppController::with_detected_theme(AppSettings::default());
        controller.connect_local_file_blocking(&db_path)?;
        let _ = controller.execute_sql_script_blocking(
            "CREATE TABLE metrics (id INTEGER PRIMARY KEY, score REAL, note TEXT);\
             INSERT INTO metrics VALUES (1, 10.0, 'ok');\
             INSERT INTO metrics VALUES (2, NULL, 'missing');\
             INSERT INTO metrics VALUES (3, 30.0, 'done');",
        )?;
        controller.open_table_tab_blocking(TableRef::new(None, "metrics"))?;

        assert!(!controller.is_column_pinned("id"));
        controller.toggle_pin_column("id");
        assert!(controller.is_column_pinned("id"));
        controller.toggle_pin_column("id");
        assert!(!controller.is_column_pinned("id"));

        let stats = controller.compute_grid_aggregates(&[0, 1, 2], Some(1));
        assert_eq!(stats.count, 3);
        assert_eq!(stats.non_null_count, 2);
        assert_eq!(stats.numeric_count, 2);
        assert!((stats.sum - 40.0).abs() < 1e-9);
        assert!((stats.avg - 20.0).abs() < 1e-9);
        assert!((stats.min - 10.0).abs() < 1e-9);
        assert!((stats.max - 30.0).abs() < 1e-9);

        let tsv = controller.format_rows_as_tsv(&[0, 1]);
        assert_eq!(tsv, "1\t10\tok\n2\tNULL\tmissing\n");

        let pasted =
            controller.paste_tsv_into_active_table("4\t40.0\tpasted\n5\tNULL\tsecond\n")?;
        assert_eq!(pasted, 2);
        assert_eq!(controller.grid_raw_rows.len(), 5);
        Ok(())
    }

    #[test]
    fn scheduled_tasks_crud_and_tick_execution()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let db_path = dir.path().join("tasks_test.sqlite");
        let tasks_path = dir.path().join("scheduled_tasks.json");
        std::fs::File::create(&db_path)?;

        let mut controller = AppController::with_detected_theme(AppSettings::default());
        controller.connect_local_file_blocking(&db_path)?;
        let _ = controller.execute_sql_script_blocking(
            "CREATE TABLE log_entries (id INTEGER PRIMARY KEY, msg TEXT);",
        )?;

        controller.upsert_scheduled_task(ScheduledTaskRecord {
            id: "task-1".to_owned(),
            name: "Insert Log".to_owned(),
            connection_id: String::new(),
            task_kind: "sql".to_owned(),
            sql_or_target: "INSERT INTO log_entries (msg) VALUES ('tick');".to_owned(),
            interval_minutes: 5,
            enabled: true,
            last_run_unix: None,
            last_status: None,
            last_duration_ms: None,
            run_history: Vec::new(),
        });
        assert!(controller.toggle_scheduled_task("task-1", true));
        controller.save_scheduled_tasks_to_path(&tasks_path)?;

        let mut reloaded = AppController::with_detected_theme(AppSettings::default());
        reloaded.load_scheduled_tasks_from_path(&tasks_path);
        assert_eq!(reloaded.scheduled_tasks.len(), 1);

        let runs = controller.tick_due_scheduled_tasks();
        assert_eq!(runs.len(), 1);
        assert!(runs[0].success);

        let (_, rows) = controller.execute_query_capture_blocking("SELECT msg FROM log_entries")?;
        assert_eq!(rows.len(), 1);
        assert!(controller.delete_scheduled_task("task-1"));
        assert!(controller.scheduled_tasks.is_empty());
        Ok(())
    }
}
