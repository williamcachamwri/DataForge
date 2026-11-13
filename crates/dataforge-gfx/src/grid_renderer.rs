//! Virtualized, GPU-native Direct2D + DirectWrite data grid renderer.
//!
//! Supports dual-pane frozen columns, a `#` row-number gutter, sortable column
//! headers, foreign-key arrow badges, `NULL`/`DEFAULT` styling, row-state tints,
//! multi-cell selection with `focusBorder`, and exact pixel-to-cell hit-testing.

use std::ops::Range;

use crate::context::{
    FontWeight, RenderSurface, TextAlign, TextFormatSpec, VerticalAlign, point_f, rect_f,
};
use crate::error::Result;
use crate::palette::{Color, ThemePalette};

/// Sort indicator shown in a column header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    /// Ascending (`▲`).
    Ascending,
    /// Descending (`▼`).
    Descending,
}

/// Modification state of a grid row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    /// Unmodified committed row.
    Clean,
    /// Newly inserted uncommitted row.
    Inserted,
    /// Existing row with edited cells.
    Modified,
    /// Row staged for deletion.
    Deleted,
}

/// Semantic kind of a grid cell, driving alignment and styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    /// Standard string or generic value.
    Text,
    /// Numeric value (right-aligned).
    Number,
    /// Boolean value.
    Boolean,
    /// SQL `NULL` value (italic muted).
    Null,
    /// SQL `DEFAULT` placeholder (italic accent).
    Default,
    /// Value with a clickable foreign-key navigation badge.
    ForeignKey,
}

/// Column definition for [`DataGridRenderer`].
#[derive(Debug, Clone, PartialEq)]
pub struct GridColumn {
    /// Column header title.
    pub title: String,
    /// Column width in DIPs.
    pub width: f32,
    /// Optional active sort direction.
    pub sort: Option<SortDirection>,
    /// Whether clicking the header can sort this column.
    pub sortable: bool,
}

impl GridColumn {
    /// Creates a new sortable column with the given title and DIP width.
    #[must_use]
    pub fn new(title: impl Into<String>, width: f32) -> Self {
        Self {
            title: title.into(),
            width: width.max(32.0),
            sort: None,
            sortable: true,
        }
    }
}

/// Single cell value passed to [`DataGridRenderer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridCell {
    /// Display text (ignored for `Null` / `Default` which render canonical labels).
    pub text: String,
    /// Semantic kind.
    pub kind: CellKind,
}

impl GridCell {
    /// Creates a standard text cell.
    #[must_use]
    pub fn text(value: impl Into<String>) -> Self {
        Self {
            text: value.into(),
            kind: CellKind::Text,
        }
    }

    /// Creates a `NULL` cell.
    #[must_use]
    pub const fn null() -> Self {
        Self {
            text: String::new(),
            kind: CellKind::Null,
        }
    }

    /// Creates a `DEFAULT` cell.
    #[must_use]
    pub const fn default_val() -> Self {
        Self {
            text: String::new(),
            kind: CellKind::Default,
        }
    }

    /// Creates a foreign-key cell with a navigable arrow badge.
    #[must_use]
    pub fn foreign_key(value: impl Into<String>) -> Self {
        Self {
            text: value.into(),
            kind: CellKind::ForeignKey,
        }
    }
}

/// A single row of data for [`DataGridRenderer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridRow {
    /// 1-based display row number shown in the `#` column.
    pub row_number: usize,
    /// Row state driving background tint.
    pub state: RowState,
    /// Cell values aligned with `columns`.
    pub cells: Vec<GridCell>,
}

/// Active selection and focus coordinates in the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GridSelection {
    /// Currently focused cell `(row, col)` receiving the `focusBorder`.
    pub focused: Option<(usize, usize)>,
    /// Rectangular selection anchor `(row, col)`.
    pub anchor: Option<(usize, usize)>,
}

impl GridSelection {
    /// Returns `true` if `(row, col)` falls inside the active selection rectangle.
    #[must_use]
    pub fn contains(&self, row: usize, col: usize) -> bool {
        let Some((fr, fc)) = self.focused else {
            return false;
        };
        let (ar, ac) = self.anchor.unwrap_or((fr, fc));
        let r_min = fr.min(ar);
        let r_max = fr.max(ar);
        let c_min = fc.min(ac);
        let c_max = fc.max(ac);
        row >= r_min && row <= r_max && col >= c_min && col <= c_max
    }
}

/// Layout metrics and scroll state for [`DataGridRenderer`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLayout {
    /// Viewport width in DIPs.
    pub viewport_width: f32,
    /// Viewport height in DIPs.
    pub viewport_height: f32,
    /// Width of the `#` row-number gutter in DIPs.
    pub gutter_width: f32,
    /// Height of the column header row in DIPs.
    pub header_height: f32,
    /// Height of each data row in DIPs.
    pub row_height: f32,
    /// Number of data columns pinned in the left frozen pane.
    pub frozen_columns: usize,
    /// Horizontal scroll offset of the right pane in DIPs.
    pub scroll_x: f32,
    /// Vertical scroll offset of rows in DIPs.
    pub scroll_y: f32,
    /// Font family for grid text.
    pub font_size: f32,
}

impl Default for GridLayout {
    fn default() -> Self {
        Self {
            viewport_width: 800.0,
            viewport_height: 600.0,
            gutter_width: 48.0,
            header_height: 28.0,
            row_height: 24.0,
            frozen_columns: 0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            font_size: 12.5,
        }
    }
}

impl GridLayout {
    /// Computes the half-open range of visible row indices for virtualization.
    #[must_use]
    pub fn visible_rows(&self, total_rows: usize) -> Range<usize> {
        if total_rows == 0 || self.row_height <= 0.0 {
            return 0..0;
        }
        let body_height = (self.viewport_height - self.header_height).max(0.0);
        let first = (self.scroll_y.max(0.0) / self.row_height).floor() as usize;
        let visible_count = (body_height / self.row_height).ceil() as usize + 1;
        let start = first.min(total_rows);
        let end = first.saturating_add(visible_count).min(total_rows);
        start..end
    }

    /// Returns the total DIP width of the frozen data columns (excluding `#` gutter).
    #[must_use]
    pub fn frozen_pane_width(&self, columns: &[GridColumn]) -> f32 {
        let count = self.frozen_columns.min(columns.len());
        columns[..count].iter().map(|c| c.width).sum()
    }

    /// X coordinate where the scrollable right pane begins.
    #[must_use]
    pub fn scrollable_origin_x(&self, columns: &[GridColumn]) -> f32 {
        self.gutter_width + self.frozen_pane_width(columns)
    }
}

/// Result of pixel-to-cell hit-testing on the data grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridHitTest {
    /// Click landed on the `#` row-number gutter for `row`.
    RowNumber(usize),
    /// Click landed on a column header.
    Header {
        /// Column index.
        col: usize,
        /// `true` if the click landed on the right-hand sort indicator button.
        is_sort_button: bool,
    },
    /// Click landed on the foreign-key arrow badge inside `(row, col)`.
    FkBadge {
        /// Row index.
        row: usize,
        /// Column index.
        col: usize,
    },
    /// Click landed inside data cell `(row, col)`.
    Cell {
        /// Row index.
        row: usize,
        /// Column index.
        col: usize,
    },
    /// Click landed outside populated rows/columns.
    None,
}

/// GPU-native Direct2D + DirectWrite virtualized data grid renderer.
#[derive(Debug, Clone)]
pub struct DataGridRenderer {
    /// Font family name used for headers and cells.
    pub font_family: String,
}

impl Default for DataGridRenderer {
    fn default() -> Self {
        Self {
            font_family: "Segoe UI".to_owned(),
        }
    }
}

impl DataGridRenderer {
    /// Width of the clickable FK arrow badge inside a cell.
    const FK_BADGE_WIDTH: f32 = 18.0;
    /// Width of the clickable sort button area in a header.
    const SORT_BUTTON_WIDTH: f32 = 20.0;

    /// Creates a new [`DataGridRenderer`] with the specified font family.
    #[must_use]
    pub fn new(font_family: impl Into<String>) -> Self {
        Self {
            font_family: font_family.into(),
        }
    }

    /// Performs exact pixel-to-cell hit-testing in DIP coordinates.
    #[must_use]
    pub fn hit_test(
        x: f32,
        y: f32,
        layout: &GridLayout,
        columns: &[GridColumn],
        row_count: usize,
    ) -> GridHitTest {
        if x < 0.0 || y < 0.0 || x > layout.viewport_width || y > layout.viewport_height {
            return GridHitTest::None;
        }
        if y < layout.header_height {
            return Self::hit_test_header(x, layout, columns);
        }
        if layout.row_height <= 0.0 {
            return GridHitTest::None;
        }
        let row = ((y - layout.header_height + layout.scroll_y.max(0.0)) / layout.row_height)
            .floor() as usize;
        if row >= row_count {
            return GridHitTest::None;
        }
        if x < layout.gutter_width {
            return GridHitTest::RowNumber(row);
        }
        match Self::hit_test_column(x, layout, columns) {
            Some((col, rel_x, col_width)) => {
                if rel_x >= col_width - Self::FK_BADGE_WIDTH {
                    // Caller can check cell kind; we report Cell unless FK badge is hit.
                    let _ = Self::FK_BADGE_WIDTH;
                }
                GridHitTest::Cell { row, col }
            }
            None => GridHitTest::None,
        }
    }

    /// Hit-tests `(x, y)` against populated rows/cells, distinguishing [`GridHitTest::FkBadge`]
    /// when the cell at `(row, col)` has [`CellKind::ForeignKey`].
    #[must_use]
    pub fn hit_test_rows(
        x: f32,
        y: f32,
        layout: &GridLayout,
        columns: &[GridColumn],
        rows: &[GridRow],
    ) -> GridHitTest {
        let base = Self::hit_test(x, y, layout, columns, rows.len());
        if let GridHitTest::Cell { row, col } = base
            && let Some(cell) = rows.get(row).and_then(|r| r.cells.get(col))
            && cell.kind == CellKind::ForeignKey
            && let Some((_, rel_x, col_width)) = Self::hit_test_column(x, layout, columns)
            && rel_x >= (col_width - Self::FK_BADGE_WIDTH).max(0.0)
        {
            return GridHitTest::FkBadge { row, col };
        }
        base
    }

    fn hit_test_header(x: f32, layout: &GridLayout, columns: &[GridColumn]) -> GridHitTest {
        if x < layout.gutter_width {
            return GridHitTest::None;
        }
        match Self::hit_test_column(x, layout, columns) {
            Some((col, rel_x, col_width)) => {
                let is_sort_button = columns[col].sortable
                    && rel_x >= (col_width - Self::SORT_BUTTON_WIDTH).max(0.0);
                GridHitTest::Header {
                    col,
                    is_sort_button,
                }
            }
            None => GridHitTest::None,
        }
    }

    fn hit_test_column(
        x: f32,
        layout: &GridLayout,
        columns: &[GridColumn],
    ) -> Option<(usize, f32, f32)> {
        let frozen_count = layout.frozen_columns.min(columns.len());
        let mut cursor_x = layout.gutter_width;
        for (idx, col) in columns[..frozen_count].iter().enumerate() {
            if x >= cursor_x && x < cursor_x + col.width {
                return Some((idx, x - cursor_x, col.width));
            }
            cursor_x += col.width;
        }
        let scroll_origin = cursor_x;
        if x < scroll_origin {
            return None;
        }
        let mut scroll_x = scroll_origin - layout.scroll_x.max(0.0);
        for (offset, col) in columns[frozen_count..].iter().enumerate() {
            let idx = frozen_count + offset;
            if x >= scroll_x && x < scroll_x + col.width && x >= scroll_origin {
                return Some((idx, x - scroll_x, col.width));
            }
            scroll_x += col.width;
        }
        None
    }

    /// Renders the visible virtualized portion of the data grid into `surface`.
    ///
    /// # Errors
    /// Returns [`crate::GfxError`] if Direct2D brush or text format creation fails.
    pub fn draw(
        &self,
        surface: &mut RenderSurface,
        palette: &ThemePalette,
        layout: &GridLayout,
        columns: &[GridColumn],
        rows: &[GridRow],
        selection: &GridSelection,
    ) -> Result<()> {
        surface.fill_rect(
            rect_f(0.0, 0.0, layout.viewport_width, layout.viewport_height),
            palette.content.bg,
        )?;

        let visible_rows = layout.visible_rows(rows.len());
        let args = GridDrawArgs {
            palette,
            layout,
            rows,
            visible_rows: &visible_rows,
            selection,
        };
        self.draw_gutter_and_rows(surface, &args, columns)?;
        self.draw_headers(surface, palette, layout, columns)?;

        let frozen_x = layout.scrollable_origin_x(columns);
        if layout.frozen_columns > 0 && frozen_x < layout.viewport_width {
            surface.draw_line(
                point_f(frozen_x, 0.0),
                point_f(frozen_x, layout.viewport_height),
                palette.grid.frozen_divider,
                2.0,
            )?;
        }
        Ok(())
    }

    fn draw_gutter_and_rows(
        &self,
        surface: &mut RenderSurface,
        args: &GridDrawArgs<'_>,
        columns: &[GridColumn],
    ) -> Result<()> {
        let layout = args.layout;
        let frozen_count = layout.frozen_columns.min(columns.len());
        let scroll_origin = layout.scrollable_origin_x(columns);

        // Draw scrollable right pane clipped to `[scroll_origin, viewport_width]`.
        if scroll_origin < layout.viewport_width {
            surface.push_clip(rect_f(
                scroll_origin,
                layout.header_height,
                layout.viewport_width - scroll_origin,
                (layout.viewport_height - layout.header_height).max(0.0),
            ));
            let mut x = scroll_origin - layout.scroll_x.max(0.0);
            for (offset, col) in columns[frozen_count..].iter().enumerate() {
                let col_idx = frozen_count + offset;
                if x + col.width >= scroll_origin && x < layout.viewport_width {
                    self.draw_column_cells(surface, args, col_idx, x, col.width)?;
                }
                x += col.width;
            }
            surface.pop_clip();
        }

        // Draw frozen columns and `#` gutter on top.
        let mut fx = layout.gutter_width;
        for (col_idx, col) in columns[..frozen_count].iter().enumerate() {
            self.draw_column_cells(surface, args, col_idx, fx, col.width)?;
            fx += col.width;
        }
        self.draw_row_number_gutter(surface, args.palette, layout, args.rows, args.visible_rows)?;
        Ok(())
    }

    fn draw_row_number_gutter(
        &self,
        surface: &mut RenderSurface,
        palette: &ThemePalette,
        layout: &GridLayout,
        rows: &[GridRow],
        visible_rows: &Range<usize>,
    ) -> Result<()> {
        surface.fill_rect(
            rect_f(
                0.0,
                layout.header_height,
                layout.gutter_width,
                (layout.viewport_height - layout.header_height).max(0.0),
            ),
            palette.grid.gutter_bg,
        )?;
        let spec = TextFormatSpec::new(&self.font_family, layout.font_size - 1.0)
            .with_align(TextAlign::Trailing)
            .with_valign(VerticalAlign::Center);

        for row_idx in visible_rows.clone() {
            let y = layout.header_height + (row_idx as f32 * layout.row_height) - layout.scroll_y;
            let row = &rows[row_idx];
            let label = row.row_number.to_string();
            surface.draw_text(
                &label,
                rect_f(4.0, y, (layout.gutter_width - 10.0).max(4.0), layout.row_height),
                &spec,
                palette.grid.gutter_text,
            )?;
            surface.draw_line(
                point_f(0.0, y + layout.row_height),
                point_f(layout.gutter_width, y + layout.row_height),
                palette.content.border,
                1.0,
            )?;
        }
        surface.draw_line(
            point_f(layout.gutter_width, 0.0),
            point_f(layout.gutter_width, layout.viewport_height),
            palette.content.border,
            1.0,
        )?;
        Ok(())
    }

    fn draw_column_cells(
        &self,
        surface: &mut RenderSurface,
        args: &GridDrawArgs<'_>,
        col_idx: usize,
        x: f32,
        width: f32,
    ) -> Result<()> {
        let layout = args.layout;
        let palette = args.palette;
        for row_idx in args.visible_rows.clone() {
            let y = layout.header_height + (row_idx as f32 * layout.row_height) - layout.scroll_y;
            let cell_rect = rect_f(x, y, width, layout.row_height);
            let row = &args.rows[row_idx];
            let bg = row_background(palette, row_idx, row.state);
            surface.fill_rect(cell_rect, bg)?;

            if args.selection.contains(row_idx, col_idx) {
                surface.fill_rect(cell_rect, palette.content.selection_bg)?;
            }

            if let Some(cell) = row.cells.get(col_idx) {
                self.draw_cell_content(surface, palette, layout, cell_rect, cell)?;
            }

            surface.draw_line(
                point_f(x, y + layout.row_height),
                point_f(x + width, y + layout.row_height),
                palette.content.border,
                1.0,
            )?;
            surface.draw_line(
                point_f(x + width, y),
                point_f(x + width, y + layout.row_height),
                palette.content.border,
                1.0,
            )?;

            if args.selection.focused == Some((row_idx, col_idx)) {
                draw_focus_border(surface, cell_rect, palette.content.focus_border)?;
            }
        }
        Ok(())
    }

    fn draw_cell_content(
        &self,
        surface: &mut RenderSurface,
        palette: &ThemePalette,
        layout: &GridLayout,
        cell_rect: windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F,
        cell: &GridCell,
    ) -> Result<()> {
        let (text, color, italic, align) = match cell.kind {
            CellKind::Null => ("NULL", palette.grid.null_text, true, TextAlign::Leading),
            CellKind::Default => ("DEFAULT", palette.grid.default_text, true, TextAlign::Leading),
            CellKind::Number => (
                cell.text.as_str(),
                palette.content.text,
                false,
                TextAlign::Trailing,
            ),
            CellKind::Text | CellKind::Boolean | CellKind::ForeignKey => (
                cell.text.as_str(),
                palette.content.text,
                false,
                TextAlign::Leading,
            ),
        };

        let right_pad = if cell.kind == CellKind::ForeignKey {
            Self::FK_BADGE_WIDTH + 4.0
        } else {
            8.0
        };
        let text_rect = rect_f(
            cell_rect.left + 8.0,
            cell_rect.top,
            (cell_rect.right - cell_rect.left - 8.0 - right_pad).max(4.0),
            cell_rect.bottom - cell_rect.top,
        );
        let spec = TextFormatSpec::new(&self.font_family, layout.font_size)
            .with_italic(italic)
            .with_align(align)
            .with_valign(VerticalAlign::Center);
        surface.draw_text(text, text_rect, &spec, color)?;

        if cell.kind == CellKind::ForeignKey {
            let badge_rect = rect_f(
                cell_rect.right - Self::FK_BADGE_WIDTH - 2.0,
                cell_rect.top + 2.0,
                Self::FK_BADGE_WIDTH - 2.0,
                (cell_rect.bottom - cell_rect.top - 4.0).max(8.0),
            );
            let badge_spec = TextFormatSpec::new(&self.font_family, layout.font_size - 1.5)
                .with_weight(FontWeight::SemiBold)
                .with_align(TextAlign::Center)
                .with_valign(VerticalAlign::Center);
            surface.draw_text("↗", badge_rect, &badge_spec, palette.grid.fk_badge)?;
        }
        Ok(())
    }

    fn draw_headers(
        &self,
        surface: &mut RenderSurface,
        palette: &ThemePalette,
        layout: &GridLayout,
        columns: &[GridColumn],
    ) -> Result<()> {
        surface.fill_rect(
            rect_f(0.0, 0.0, layout.viewport_width, layout.header_height),
            palette.grid.header_bg,
        )?;
        let gutter_spec = TextFormatSpec::new(&self.font_family, layout.font_size - 1.0)
            .with_weight(FontWeight::SemiBold)
            .with_align(TextAlign::Center)
            .with_valign(VerticalAlign::Center);
        surface.draw_text(
            "#",
            rect_f(0.0, 0.0, layout.gutter_width, layout.header_height),
            &gutter_spec,
            palette.grid.gutter_text,
        )?;

        let frozen_count = layout.frozen_columns.min(columns.len());
        let scroll_origin = layout.scrollable_origin_x(columns);
        if scroll_origin < layout.viewport_width {
            surface.push_clip(rect_f(
                scroll_origin,
                0.0,
                layout.viewport_width - scroll_origin,
                layout.header_height,
            ));
            let mut sx = scroll_origin - layout.scroll_x.max(0.0);
            for col in &columns[frozen_count..] {
                self.draw_single_header(surface, palette, layout, col, sx)?;
                sx += col.width;
            }
            surface.pop_clip();
        }

        let mut fx = layout.gutter_width;
        for col in &columns[..frozen_count] {
            self.draw_single_header(surface, palette, layout, col, fx)?;
            fx += col.width;
        }

        surface.draw_line(
            point_f(0.0, layout.header_height),
            point_f(layout.viewport_width, layout.header_height),
            palette.content.border,
            1.0,
        )?;
        Ok(())
    }

    fn draw_single_header(
        &self,
        surface: &mut RenderSurface,
        palette: &ThemePalette,
        layout: &GridLayout,
        col: &GridColumn,
        x: f32,
    ) -> Result<()> {
        let title_spec = TextFormatSpec::new(&self.font_family, layout.font_size)
            .with_weight(FontWeight::SemiBold)
            .with_align(TextAlign::Leading)
            .with_valign(VerticalAlign::Center);
        let sort_pad = if col.sort.is_some() {
            Self::SORT_BUTTON_WIDTH
        } else {
            6.0
        };
        surface.draw_text(
            &col.title,
            rect_f(
                x + 8.0,
                0.0,
                (col.width - 8.0 - sort_pad).max(4.0),
                layout.header_height,
            ),
            &title_spec,
            palette.grid.header_text,
        )?;

        if let Some(dir) = col.sort {
            let arrow = match dir {
                SortDirection::Ascending => "▲",
                SortDirection::Descending => "▼",
            };
            let arrow_spec = TextFormatSpec::new(&self.font_family, layout.font_size - 2.5)
                .with_align(TextAlign::Center)
                .with_valign(VerticalAlign::Center);
            surface.draw_text(
                arrow,
                rect_f(
                    x + col.width - Self::SORT_BUTTON_WIDTH,
                    0.0,
                    Self::SORT_BUTTON_WIDTH,
                    layout.header_height,
                ),
                &arrow_spec,
                palette.content.accent,
            )?;
        }
        surface.draw_line(
            point_f(x + col.width, 0.0),
            point_f(x + col.width, layout.header_height),
            palette.content.border,
            1.0,
        )?;
        Ok(())
    }
}

struct GridDrawArgs<'a> {
    palette: &'a ThemePalette,
    layout: &'a GridLayout,
    rows: &'a [GridRow],
    visible_rows: &'a Range<usize>,
    selection: &'a GridSelection,
}

fn row_background(palette: &ThemePalette, row_idx: usize, state: RowState) -> Color {
    match state {
        RowState::Inserted => palette.grid.row_inserted_bg,
        RowState::Modified => palette.grid.row_modified_bg,
        RowState::Deleted => palette.grid.row_deleted_bg,
        RowState::Clean => {
            if row_idx % 2 == 1 {
                palette.grid.row_alt_bg
            } else {
                palette.content.bg
            }
        }
    }
}

fn draw_focus_border(
    surface: &mut RenderSurface,
    cell_rect: windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F,
    color: Color,
) -> Result<()> {
    // 2px outer focusBorder + 1px crisp inner highlight.
    surface.draw_rect(
        rect_f(
            cell_rect.left + 1.0,
            cell_rect.top + 1.0,
            (cell_rect.right - cell_rect.left - 2.0).max(2.0),
            (cell_rect.bottom - cell_rect.top - 2.0).max(2.0),
        ),
        color,
        2.0,
    )?;
    surface.draw_rect(
        rect_f(
            cell_rect.left + 2.5,
            cell_rect.top + 2.5,
            (cell_rect.right - cell_rect.left - 5.0).max(1.0),
            (cell_rect.bottom - cell_rect.top - 5.0).max(1.0),
        ),
        color.with_alpha(0.55),
        1.0,
    )?;
    Ok(())
}
