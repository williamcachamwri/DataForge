//! 3-layer DataGridModel, GridSelectionReducer, frozen leading columns,
//! paged table_query, sorting, TableEdits batching, filter bar, and exports.

/// Maximum fraction of the grid viewport width that frozen leading columns may occupy.
pub const MAXIMUM_FROZEN_SHARE: f64 = 2.0 / 3.0;

/// Minimum column width in logical pixels.
pub const MIN_COLUMN_WIDTH: f64 = 48.0;

/// Maximum column width in logical pixels.
pub const MAX_COLUMN_WIDTH: f64 = 1200.0;

/// Stable row identity independent of display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RowID(pub u64);

/// Stable column identity independent of display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DataGridColumnID(pub u64);

/// Which table pane a column or slot belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataGridPane {
    Main,
    Frozen,
}

/// Slot resolved from a pane's table column index (index 0 is the row-number column).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneColumnSlot {
    RowNumbers,
    DataColumn {
        id: DataGridColumnID,
        display_position: usize,
    },
}

/// Column metadata held by [`DataGridColumnSet`].
#[derive(Debug, Clone, PartialEq)]
pub struct DataGridColumn {
    pub id: DataGridColumnID,
    pub data_index: usize,
    pub name: String,
    pub width: f64,
    pub is_frozen: bool,
}

/// Sole converter between data index, display position, and pane.
///
/// Per `data-grid.md`:
/// - Row-number column sits at table index 0 in both panes.
/// - Frozen columns are leading columns occupying at most `2/3` of grid width.
/// - A column moves only within its side of the freeze line.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DataGridColumnSet {
    /// Columns in display order (frozen leading columns first, then scrollable).
    ordered: Vec<DataGridColumn>,
}

impl DataGridColumnSet {
    /// Builds a column set from column names and default widths.
    #[must_use]
    pub fn from_names(names: &[String], default_width: f64) -> Self {
        let width = default_width.clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH);
        let ordered = names
            .iter()
            .enumerate()
            .map(|(idx, name)| DataGridColumn {
                id: DataGridColumnID(idx as u64 + 1),
                data_index: idx,
                name: name.clone(),
                width,
                is_frozen: false,
            })
            .collect();
        Self { ordered }
    }

    /// Returns all columns in display order.
    #[must_use]
    pub fn columns(&self) -> &[DataGridColumn] {
        &self.ordered
    }

    /// Returns the number of frozen leading columns.
    #[must_use]
    pub fn frozen_count(&self) -> usize {
        self.ordered.iter().take_while(|c| c.is_frozen).count()
    }

    /// Total width of currently frozen columns.
    #[must_use]
    pub fn frozen_width(&self) -> f64 {
        self.ordered.iter().filter(|c| c.is_frozen).map(|c| c.width).sum()
    }

    /// Resolves a table index in `pane` to its [`PaneColumnSlot`].
    #[must_use]
    pub fn slot(&self, pane: DataGridPane, table_index: usize) -> Option<PaneColumnSlot> {
        if table_index == 0 {
            return Some(PaneColumnSlot::RowNumbers);
        }
        let display_pos = table_index - 1;
        let col = match pane {
            DataGridPane::Main => self.ordered.get(display_pos)?,
            DataGridPane::Frozen => {
                let col = self.ordered.get(display_pos)?;
                if !col.is_frozen {
                    return None;
                }
                col
            }
        };
        Some(PaneColumnSlot::DataColumn {
            id: col.id,
            display_position: display_pos,
        })
    }

    /// Converts a display position into a table index for `pane`.
    #[must_use]
    pub fn table_index(&self, pane: DataGridPane, display_position: usize) -> Option<usize> {
        let col = self.ordered.get(display_position)?;
        match pane {
            DataGridPane::Main => Some(display_position + 1),
            DataGridPane::Frozen => col.is_frozen.then_some(display_position + 1),
        }
    }

    /// Returns the display position for `id`.
    #[must_use]
    pub fn display_position(&self, id: DataGridColumnID) -> Option<usize> {
        self.ordered.iter().position(|c| c.id == id)
    }

    /// Returns the data index for `id`.
    #[must_use]
    pub fn data_index(&self, id: DataGridColumnID) -> Option<usize> {
        self.ordered.iter().find(|c| c.id == id).map(|c| c.data_index)
    }

    /// Freezes leading columns up to and including `display_pos` if their combined
    /// width does not exceed `viewport_width * MAXIMUM_FROZEN_SHARE`.
    pub fn freeze_up_to(&mut self, display_pos: usize, viewport_width: f64) -> bool {
        if display_pos >= self.ordered.len() {
            return false;
        }
        let needed_width: f64 = self.ordered[..=display_pos].iter().map(|c| c.width).sum();
        if needed_width > viewport_width * MAXIMUM_FROZEN_SHARE {
            return false;
        }
        for (idx, col) in self.ordered.iter_mut().enumerate() {
            col.is_frozen = idx <= display_pos;
        }
        true
    }

    /// Unfreezes all columns.
    pub fn unfreeze_all(&mut self) {
        for col in &mut self.ordered {
            col.is_frozen = false;
        }
    }

    /// Resizes `id` within `[MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH]`.
    pub fn resize_column(&mut self, id: DataGridColumnID, width: f64) -> bool {
        let Some(col) = self.ordered.iter_mut().find(|c| c.id == id) else {
            return false;
        };
        col.width = width.clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH);
        true
    }

    /// Moves a column within its side of the freeze line.
    pub fn move_within_side(&mut self, from_pos: usize, to_pos: usize) -> bool {
        if from_pos >= self.ordered.len() || to_pos >= self.ordered.len() {
            return false;
        }
        if self.ordered[from_pos].is_frozen != self.ordered[to_pos].is_frozen {
            return false;
        }
        let col = self.ordered.remove(from_pos);
        self.ordered.insert(to_pos, col);
        true
    }
}

/// Maps display row positions to stable [`RowID`]s.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DataGridRowOrder {
    rows: Vec<RowID>,
}

impl DataGridRowOrder {
    /// Creates a row order from a sequence of [`RowID`]s.
    #[must_use]
    pub fn new(rows: Vec<RowID>) -> Self {
        Self { rows }
    }

    /// Returns the number of display rows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Returns whether there are zero rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Returns the [`RowID`] at `display_row`.
    #[must_use]
    pub fn row_id(&self, display_row: usize) -> Option<RowID> {
        self.rows.get(display_row).copied()
    }

    /// Returns the display position of `id`.
    #[must_use]
    pub fn display_row(&self, id: RowID) -> Option<usize> {
        self.rows.iter().position(|&r| r == id)
    }

    /// Returns all [`RowID`]s in display order.
    #[must_use]
    pub fn as_slice(&self) -> &[RowID] {
        &self.rows
    }
}

/// Persistence scope for a grid's layout and display formats.
///
/// Per `data-grid.md`: only the data tab of a real, non-temporary table has
/// `Table`; every other grid is `Ephemeral` and never writes saved layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataGridLayoutScope {
    Table(String),
    Ephemeral,
}

/// Delta sent from [`DataGridModel`] to a mounted view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataGridChange {
    ReloadAll,
    RowsUpdated(Vec<RowID>),
    SelectionChanged,
    LayoutChanged,
}

/// Coordinate in the grid identified by stable row and column IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GridCoord {
    pub row: RowID,
    pub col: DataGridColumnID,
}

/// Selection mode per `data-grid.md`.
///
/// Copy, Delete, and row commands read `mode`, never the rectangle's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GridSelectionMode {
    Cursor,
    Cells,
    Rows,
}

/// Pure grid selection state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridSelection {
    pub mode: GridSelectionMode,
    pub anchor: Option<GridCoord>,
    pub focus: Option<GridCoord>,
    pub selected_rows: std::collections::BTreeSet<RowID>,
    pub selected_cols: std::collections::BTreeSet<DataGridColumnID>,
}

impl Default for GridSelection {
    fn default() -> Self {
        Self {
            mode: GridSelectionMode::Cursor,
            anchor: None,
            focus: None,
            selected_rows: std::collections::BTreeSet::new(),
            selected_cols: std::collections::BTreeSet::new(),
        }
    }
}

/// Input events to [`GridSelectionReducer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GridSelectionAction {
    Click(GridCoord),
    ShiftClick(GridCoord),
    DragTo(GridCoord),
    SelectRow(RowID),
    SelectAll,
    RowsSelectedNatively(Vec<RowID>),
    Clear,
}

/// Pure reducer for [`GridSelection`].
#[derive(Debug, Clone, Copy, Default)]
pub struct GridSelectionReducer;

impl GridSelectionReducer {
    /// Applies `action` to `state` using `rows` and `cols` to resolve ranges.
    #[must_use]
    pub fn reduce(
        state: &GridSelection,
        action: GridSelectionAction,
        rows: &DataGridRowOrder,
        cols: &DataGridColumnSet,
    ) -> GridSelection {
        match action {
            GridSelectionAction::Clear => GridSelection::default(),
            GridSelectionAction::Click(coord) => GridSelection {
                mode: GridSelectionMode::Cursor,
                anchor: Some(coord),
                focus: Some(coord),
                selected_rows: std::collections::BTreeSet::from([coord.row]),
                selected_cols: std::collections::BTreeSet::from([coord.col]),
            },
            GridSelectionAction::ShiftClick(coord) | GridSelectionAction::DragTo(coord) => {
                let anchor = state.anchor.unwrap_or(coord);
                Self::range_selection(anchor, coord, rows, cols)
            }
            GridSelectionAction::SelectRow(row_id) => GridSelection {
                mode: GridSelectionMode::Rows,
                anchor: cols.columns().first().map(|c| GridCoord { row: row_id, col: c.id }),
                focus: cols.columns().first().map(|c| GridCoord { row: row_id, col: c.id }),
                selected_rows: std::collections::BTreeSet::from([row_id]),
                selected_cols: cols.columns().iter().map(|c| c.id).collect(),
            },
            GridSelectionAction::SelectAll => GridSelection {
                mode: GridSelectionMode::Rows,
                anchor: rows
                    .row_id(0)
                    .and_then(|r| cols.columns().first().map(|c| GridCoord { row: r, col: c.id })),
                focus: rows
                    .as_slice()
                    .last()
                    .copied()
                    .and_then(|r| cols.columns().last().map(|c| GridCoord { row: r, col: c.id })),
                selected_rows: rows.as_slice().iter().copied().collect(),
                selected_cols: cols.columns().iter().map(|c| c.id).collect(),
            },
            GridSelectionAction::RowsSelectedNatively(row_ids) => {
                let selected_rows: std::collections::BTreeSet<RowID> = row_ids.into_iter().collect();
                let focus_col = state
                    .focus
                    .map(|c| c.col)
                    .or_else(|| cols.columns().first().map(|c| c.id));
                let first_row = selected_rows.iter().next().copied();
                let coord = first_row.and_then(|r| focus_col.map(|c| GridCoord { row: r, col: c }));
                GridSelection {
                    mode: if selected_rows.len() > 1 {
                        GridSelectionMode::Rows
                    } else {
                        GridSelectionMode::Cursor
                    },
                    anchor: coord,
                    focus: coord,
                    selected_rows,
                    selected_cols: focus_col.into_iter().collect(),
                }
            }
        }
    }

    fn range_selection(
        anchor: GridCoord,
        focus: GridCoord,
        rows: &DataGridRowOrder,
        cols: &DataGridColumnSet,
    ) -> GridSelection {
        let Some(r0) = rows.display_row(anchor.row) else {
            return GridSelection::default();
        };
        let Some(r1) = rows.display_row(focus.row) else {
            return GridSelection::default();
        };
        let Some(c0) = cols.display_position(anchor.col) else {
            return GridSelection::default();
        };
        let Some(c1) = cols.display_position(focus.col) else {
            return GridSelection::default();
        };
        let (r_min, r_max) = (r0.min(r1), r0.max(r1));
        let (c_min, c_max) = (c0.min(c1), c0.max(c1));
        let selected_rows = (r_min..=r_max).filter_map(|r| rows.row_id(r)).collect();
        let selected_cols = cols.columns()[c_min..=c_max].iter().map(|c| c.id).collect();
        GridSelection {
            mode: GridSelectionMode::Cells,
            anchor: Some(anchor),
            focus: Some(focus),
            selected_rows,
            selected_cols,
        }
    }
}

/// Paging state for `table_query` (`offset`, `limit`, `table_row_count`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableQueryPage {
    pub offset: u64,
    pub limit: u64,
    pub table_row_count: Option<u64>,
}

impl TableQueryPage {
    /// Creates a page starting at `offset` with `limit` (clamped >= 1).
    #[must_use]
    pub fn new(offset: u64, limit: u64, table_row_count: Option<u64>) -> Self {
        Self {
            offset,
            limit: limit.max(1),
            table_row_count,
        }
    }

    /// Advances to the next page if more rows remain.
    pub fn next_page(&mut self) -> bool {
        let next = self.offset.saturating_add(self.limit);
        if let Some(total) = self.table_row_count
            && next >= total
        {
            return false;
        }
        self.offset = next;
        true
    }

    /// Moves to the previous page, clamping at offset 0.
    pub fn prev_page(&mut self) -> bool {
        if self.offset == 0 {
            return false;
        }
        self.offset = self.offset.saturating_sub(self.limit);
        true
    }

    /// Returns 1-based current page and total pages when `table_row_count` is known.
    #[must_use]
    pub fn page_info(&self) -> (u64, Option<u64>) {
        let current = (self.offset / self.limit) + 1;
        let total = self.table_row_count.map(|n| n.div_ceil(self.limit).max(1));
        (current, total)
    }
}

/// Sort direction for a grid column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    /// Flips ascending <-> descending.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

/// A single sort key in `HeaderSortCycle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortKey {
    pub column_id: DataGridColumnID,
    pub direction: SortDirection,
}

/// Header sort cycle state (plain click replaces & flips; Option-click appends).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HeaderSortCycle {
    pub keys: Vec<SortKey>,
}

impl HeaderSortCycle {
    /// Applies a header click on `column_id`.
    ///
    /// Per `data-grid.md`: plain click keeps only the clicked key and flips on
    /// repeat; `keep_existing` (Option-click) keeps existing keys and appends/flips.
    pub fn click(&mut self, column_id: DataGridColumnID, keep_existing: bool) {
        if !keep_existing {
            let next_dir = match self.keys.as_slice() {
                [only] if only.column_id == column_id => only.direction.toggled(),
                _ => SortDirection::Ascending,
            };
            self.keys = vec![SortKey {
                column_id,
                direction: next_dir,
            }];
            return;
        }
        if let Some(existing) = self.keys.iter_mut().find(|k| k.column_id == column_id) {
            existing.direction = existing.direction.toggled();
        } else {
            self.keys.push(SortKey {
                column_id,
                direction: SortDirection::Ascending,
            });
        }
    }

    /// Clears all sort keys.
    pub fn clear(&mut self) {
        self.keys.clear();
    }
}

/// A single cell edit captured by stable IDs.
#[derive(Debug, Clone, PartialEq)]
pub struct DataGridCellEdit {
    pub row_id: RowID,
    pub column_id: DataGridColumnID,
    pub old_value: tablepro_core::value::Value,
    pub new_value: tablepro_core::value::Value,
}

/// A committed batch of edits (one undo step).
#[derive(Debug, Clone, PartialEq)]
pub struct EditUndoBatch {
    pub action_name: String,
    pub edits: Vec<DataGridCellEdit>,
}

/// Pending row edit batching (`TableEdits`) supporting insert, modify, delete,
/// Commit, and Roll back.
#[derive(Debug, Default, Clone)]
pub struct TableEdits {
    pub modified_cells: std::collections::BTreeMap<(RowID, DataGridColumnID), tablepro_core::value::Value>,
    pub inserted_rows: std::collections::BTreeMap<RowID, Vec<tablepro_core::value::Value>>,
    pub deleted_rows: std::collections::BTreeSet<RowID>,
    undo_stack: Vec<EditUndoBatch>,
    redo_stack: Vec<EditUndoBatch>,
    next_temp_row_id: u64,
}

impl TableEdits {
    /// Creates an empty `TableEdits` buffer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            modified_cells: std::collections::BTreeMap::new(),
            inserted_rows: std::collections::BTreeMap::new(),
            deleted_rows: std::collections::BTreeSet::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            next_temp_row_id: 1_000_000,
        }
    }

    /// Returns whether there are any pending uncommitted edits.
    #[must_use]
    pub fn has_pending_edits(&self) -> bool {
        !self.modified_cells.is_empty() || !self.inserted_rows.is_empty() || !self.deleted_rows.is_empty()
    }

    /// Records a batch of cell modifications as one undo step.
    pub fn commit_cells(&mut self, edits: Vec<DataGridCellEdit>, action_name: impl Into<String>) {
        if edits.is_empty() {
            return;
        }
        for edit in &edits {
            self.modified_cells
                .insert((edit.row_id, edit.column_id), edit.new_value.clone());
        }
        self.undo_stack.push(EditUndoBatch {
            action_name: action_name.into(),
            edits,
        });
        self.redo_stack.clear();
    }

    /// Inserts a new pending row and returns its temporary [`RowID`].
    pub fn insert_row(&mut self, values: Vec<tablepro_core::value::Value>) -> RowID {
        let id = RowID(self.next_temp_row_id);
        self.next_temp_row_id = self.next_temp_row_id.saturating_add(1);
        self.inserted_rows.insert(id, values);
        id
    }

    /// Marks `row_id` for deletion (or removes it if it was a newly inserted row).
    pub fn delete_row(&mut self, row_id: RowID) {
        if self.inserted_rows.remove(&row_id).is_none() {
            self.deleted_rows.insert(row_id);
        }
    }

    /// Rolls back all pending edits.
    pub fn rollback(&mut self) {
        self.modified_cells.clear();
        self.inserted_rows.clear();
        self.deleted_rows.clear();
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// Consumes and returns all pending edits on commit.
    pub fn take_committed(&mut self) -> Self {
        std::mem::take(self)
    }
}
