//! GPU-native Direct2D + DirectWrite SQL editor renderer.
//!
//! Features DirectWrite cached line layouts with incremental invalidation,
//! syntax token coloring, gutter line numbers, multi-cursor carets, selection
//! highlights, Vim block cursor mode, floating autocomplete popup, and
//! pixel-to-offset hit-testing.

use std::collections::HashMap;
use std::ops::Range;

use windows::Win32::Graphics::DirectWrite::{DWRITE_TEXT_RANGE, IDWriteTextLayout};

use crate::context::{
    FontWeight, RenderSurface, TextAlign, TextFormatSpec, VerticalAlign, point_f, rect_f,
};
use crate::error::Result;
use crate::palette::{Color, ThemePalette};

/// Maximum number of DirectWrite line layouts retained in [`LineLayoutCache`].
const MAX_CACHED_LINE_LAYOUTS: usize = 512;

/// SQL syntax token classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SqlTokenKind {
    /// SQL keyword (`SELECT`, `FROM`, `WHERE`, ...).
    Keyword,
    /// Table, column, or schema identifier.
    Identifier,
    /// Single-quoted string literal.
    StringLiteral,
    /// Numeric literal.
    Number,
    /// `--` or `/* */` comment.
    Comment,
    /// Operator (`=`, `<>`, `+`, `-`, `*`, `/`, ...).
    Operator,
    /// Punctuation (`,`, `;`, `(`, `)`, `.`).
    Punctuation,
    /// Built-in SQL function (`COUNT`, `SUM`, `COALESCE`, ...).
    Function,
    /// SQL data type (`INT`, `TEXT`, `UUID`, `TIMESTAMPTZ`, ...).
    TypeName,
    /// Whitespace.
    Whitespace,
}

/// Colored span within a single editor line, measured in UTF-16 code units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntaxSpan {
    /// Starting UTF-16 offset within the line.
    pub start: u32,
    /// Length in UTF-16 code units.
    pub length: u32,
    /// Token classification.
    pub kind: SqlTokenKind,
}

/// A single line of SQL text with its incremental revision and syntax spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorLine {
    /// Line text (without trailing newline).
    pub text: String,
    /// Monotonic revision bumped whenever this line is edited.
    pub revision: u64,
    /// Syntax spans covering `text`.
    pub spans: Vec<SyntaxSpan>,
}

impl EditorLine {
    /// Creates a new [`EditorLine`] and tokenizes it with [`tokenize_sql_line`].
    #[must_use]
    pub fn new(text: impl Into<String>, revision: u64) -> Self {
        let text = text.into();
        let spans = tokenize_sql_line(&text);
        Self {
            text,
            revision,
            spans,
        }
    }
}

/// Caret shape mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorMode {
    /// Standard 2px vertical insertion bar.
    Bar,
    /// Vim normal-mode solid block cursor covering one character cell.
    VimBlock,
}

/// A single cursor position in `(line, col)` UTF-16 / char-column coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorCursor {
    /// 0-based line index.
    pub line: usize,
    /// 0-based column offset within the line.
    pub col: usize,
    /// `true` for the primary cursor (highlights active line).
    pub is_primary: bool,
}

/// A text selection range between `(start_line, start_col)` and `(end_line, end_col)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorSelection {
    /// Anchor `(line, col)`.
    pub anchor: (usize, usize),
    /// Active head `(line, col)`.
    pub head: (usize, usize),
}

impl EditorSelection {
    /// Returns normalized `(start, end)` ordered top-to-bottom, left-to-right.
    #[must_use]
    pub fn normalized(&self) -> ((usize, usize), (usize, usize)) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    /// Returns `true` if the selection is empty (`anchor == head`).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.anchor.0 == self.head.0 && self.anchor.1 == self.head.1
    }
}

/// Completion item kind badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    /// SQL keyword.
    Keyword,
    /// Table or view.
    Table,
    /// Column name.
    Column,
    /// SQL function.
    Function,
}

impl CompletionKind {
    const fn badge(self) -> &'static str {
        match self {
            Self::Keyword => "KW",
            Self::Table => "TB",
            Self::Column => "CL",
            Self::Function => "FN",
        }
    }
}

/// Single entry in [`AutocompletePopup`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutocompleteItem {
    /// Primary completion label.
    pub label: String,
    /// Secondary type/table detail.
    pub detail: String,
    /// Item kind.
    pub kind: CompletionKind,
}

/// Floating autocomplete popup state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AutocompletePopup {
    /// Whether the popup is currently shown.
    pub visible: bool,
    /// Anchor `(line, col)` in document coordinates.
    pub anchor: (usize, usize),
    /// Candidate items.
    pub items: Vec<AutocompleteItem>,
    /// Index of the highlighted item.
    pub selected: usize,
}

/// Layout metrics and scroll state for [`SqlEditorRenderer`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorLayout {
    /// Viewport width in DIPs.
    pub viewport_width: f32,
    /// Viewport height in DIPs.
    pub viewport_height: f32,
    /// Line-number gutter width in DIPs.
    pub gutter_width: f32,
    /// Left padding between gutter divider and text in DIPs.
    pub text_left_pad: f32,
    /// Height of a single line in DIPs.
    pub line_height: f32,
    /// Monospace character advance width in DIPs (for hit-testing & Vim block).
    pub char_width: f32,
    /// Font size in DIPs.
    pub font_size: f32,
    /// Horizontal scroll offset in DIPs.
    pub scroll_x: f32,
    /// Vertical scroll offset in DIPs.
    pub scroll_y: f32,
}

impl Default for EditorLayout {
    fn default() -> Self {
        Self {
            viewport_width: 800.0,
            viewport_height: 600.0,
            gutter_width: 48.0,
            text_left_pad: 10.0,
            line_height: 20.0,
            char_width: 7.8,
            font_size: 13.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
        }
    }
}

impl EditorLayout {
    /// Computes the half-open range of visible line indices.
    #[must_use]
    pub fn visible_lines(&self, total_lines: usize) -> Range<usize> {
        if total_lines == 0 || self.line_height <= 0.0 {
            return 0..0;
        }
        let first = (self.scroll_y.max(0.0) / self.line_height).floor() as usize;
        let visible_count = (self.viewport_height / self.line_height).ceil() as usize + 1;
        let start = first.min(total_lines);
        let end = first.saturating_add(visible_count).min(total_lines);
        start..end
    }

    /// X origin of column 0 in viewport DIPs.
    #[must_use]
    pub fn text_origin_x(&self) -> f32 {
        self.gutter_width + self.text_left_pad - self.scroll_x.max(0.0)
    }

    /// Computes the bounding rectangle `(x, y, width, height)` of the autocomplete popup.
    #[must_use]
    pub fn popup_rect(&self, popup: &AutocompletePopup) -> (f32, f32, f32, f32) {
        let width = 280.0_f32.min((self.viewport_width - 16.0).max(120.0));
        let row_h = 22.0;
        let visible_items = popup.items.len().clamp(1, 8);
        let height = visible_items as f32 * row_h + 8.0;

        let raw_x = self.text_origin_x() + popup.anchor.1 as f32 * self.char_width;
        let raw_y = (popup.anchor.0 as f32 + 1.0) * self.line_height - self.scroll_y;
        let x = raw_x.clamp(
            self.gutter_width + 4.0,
            (self.viewport_width - width - 4.0).max(self.gutter_width + 4.0),
        );
        let y = if raw_y + height > self.viewport_height && raw_y > height + self.line_height {
            raw_y - self.line_height - height
        } else {
            raw_y.max(4.0)
        };
        (x, y, width, height)
    }
}

/// Result of pixel-to-offset hit-testing on the SQL editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorHitTest {
    /// Click landed on an item in the autocomplete popup.
    AutocompleteItem(usize),
    /// Click landed in the line-number gutter for `line`.
    Gutter(usize),
    /// Click landed in the text area at `(line, col)`.
    Text {
        /// 0-based line index.
        line: usize,
        /// 0-based column offset clamped to the line length.
        col: usize,
    },
    /// Click landed outside the editor viewport.
    None,
}

struct CachedLineLayout {
    revision: u64,
    layout: IDWriteTextLayout,
}

/// Bounded cache of DirectWrite `IDWriteTextLayout` objects per line revision.
#[derive(Default)]
pub struct LineLayoutCache {
    entries: HashMap<usize, CachedLineLayout>,
}

impl LineLayoutCache {
    /// Creates an empty [`LineLayoutCache`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of currently cached line layouts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if no line layouts are cached.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Invalidates cached layouts from `start_line` onward (incremental edit).
    pub fn invalidate_from(&mut self, start_line: usize) {
        self.entries.retain(|&line_idx, _| line_idx < start_line);
    }

    /// Clears all cached line layouts.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Retrieves a cached layout if its revision matches, otherwise builds and caches it.
    ///
    /// # Errors
    /// Returns [`crate::GfxError::Win32`] if DirectWrite fails to create the layout.
    pub fn get_or_build(
        &mut self,
        surface: &mut RenderSurface,
        line_idx: usize,
        line: &EditorLine,
        spec: &TextFormatSpec,
    ) -> Result<IDWriteTextLayout> {
        if let Some(entry) = self.entries.get(&line_idx)
            && entry.revision == line.revision
        {
            return Ok(entry.layout.clone());
        }
        if self.entries.len() >= MAX_CACHED_LINE_LAYOUTS {
            self.entries.clear();
        }
        let format = surface.text_format(spec)?;
        let layout = surface.create_text_layout(&line.text, &format, 10_000.0, 100.0)?;
        self.entries.insert(
            line_idx,
            CachedLineLayout {
                revision: line.revision,
                layout: layout.clone(),
            },
        );
        Ok(layout)
    }
}

/// Immutable snapshot of SQL editor state passed to [`SqlEditorRenderer::draw`].
#[derive(Debug, Clone, Copy)]
pub struct EditorRenderState<'a> {
    /// Active theme palette.
    pub palette: &'a ThemePalette,
    /// Viewport and scroll layout metrics.
    pub layout: &'a EditorLayout,
    /// Document lines.
    pub lines: &'a [EditorLine],
    /// Active cursors.
    pub cursors: &'a [EditorCursor],
    /// Active selections.
    pub selections: &'a [EditorSelection],
    /// Caret rendering mode (`Bar` or `VimBlock`).
    pub cursor_mode: CursorMode,
    /// Optional floating autocomplete popup.
    pub popup: Option<&'a AutocompletePopup>,
}

/// GPU-native Direct2D + DirectWrite SQL editor renderer.
pub struct SqlEditorRenderer {
    /// Monospace font family name.
    pub font_family: String,
    /// Cached per-line DirectWrite layouts.
    pub line_cache: LineLayoutCache,
}

impl Default for SqlEditorRenderer {
    fn default() -> Self {
        Self {
            font_family: "Cascadia Mono".to_owned(),
            line_cache: LineLayoutCache::new(),
        }
    }
}

impl SqlEditorRenderer {
    /// Creates a new [`SqlEditorRenderer`] with the specified monospace font family.
    #[must_use]
    pub fn new(font_family: impl Into<String>) -> Self {
        Self {
            font_family: font_family.into(),
            line_cache: LineLayoutCache::new(),
        }
    }

    /// Performs pixel-to-offset hit-testing in DIP coordinates.
    #[must_use]
    pub fn hit_test(
        x: f32,
        y: f32,
        layout: &EditorLayout,
        lines: &[EditorLine],
        popup: Option<&AutocompletePopup>,
    ) -> EditorHitTest {
        if x < 0.0 || y < 0.0 || x > layout.viewport_width || y > layout.viewport_height {
            return EditorHitTest::None;
        }
        if let Some(pop) = popup
            && pop.visible
            && !pop.items.is_empty()
        {
            let (px, py, pw, ph) = layout.popup_rect(pop);
            if x >= px && x <= px + pw && y >= py && y <= py + ph {
                let row_h = 22.0;
                let idx = ((y - py - 4.0).max(0.0) / row_h).floor() as usize;
                let clamped = idx.min(pop.items.len() - 1);
                return EditorHitTest::AutocompleteItem(clamped);
            }
        }
        if lines.is_empty() || layout.line_height <= 0.0 {
            return EditorHitTest::None;
        }
        let raw_line = ((y + layout.scroll_y.max(0.0)) / layout.line_height).floor() as usize;
        let line = raw_line.min(lines.len() - 1);
        if x < layout.gutter_width {
            return EditorHitTest::Gutter(line);
        }
        let rel_x = (x - layout.text_origin_x()).max(0.0);
        let raw_col = if layout.char_width > 0.0 {
            (rel_x / layout.char_width).round() as usize
        } else {
            0
        };
        let line_chars = lines[line].text.chars().count();
        EditorHitTest::Text {
            line,
            col: raw_col.min(line_chars),
        }
    }

    /// Renders the SQL editor (gutter, lines, selections, cursors, and popup).
    ///
    /// # Errors
    /// Returns [`crate::GfxError`] if Direct2D or DirectWrite resource creation fails.
    pub fn draw(
        &mut self,
        surface: &mut RenderSurface,
        state: &EditorRenderState<'_>,
    ) -> Result<()> {
        let palette = state.palette;
        let layout = state.layout;
        surface.fill_rect(
            rect_f(0.0, 0.0, layout.viewport_width, layout.viewport_height),
            palette.content.bg,
        )?;

        let visible = layout.visible_lines(state.lines.len());
        self.draw_active_line_bg(surface, palette, layout, state.cursors, &visible)?;
        self.draw_selections(
            surface,
            palette,
            layout,
            state.lines,
            state.selections,
            &visible,
        )?;
        self.draw_text_lines(surface, palette, layout, state.lines, &visible)?;
        self.draw_cursors(surface, state, &visible)?;
        self.draw_gutter(surface, palette, layout, state.cursors, &visible)?;

        if let Some(pop) = state.popup
            && pop.visible
            && !pop.items.is_empty()
        {
            self.draw_autocomplete_popup(surface, palette, layout, pop)?;
        }
        Ok(())
    }

    fn draw_active_line_bg(
        &self,
        surface: &mut RenderSurface,
        palette: &ThemePalette,
        layout: &EditorLayout,
        cursors: &[EditorCursor],
        visible: &Range<usize>,
    ) -> Result<()> {
        if let Some(primary) = cursors.iter().find(|c| c.is_primary)
            && visible.contains(&primary.line)
        {
            let y = primary.line as f32 * layout.line_height - layout.scroll_y;
            surface.fill_rect(
                rect_f(
                    layout.gutter_width,
                    y,
                    (layout.viewport_width - layout.gutter_width).max(0.0),
                    layout.line_height,
                ),
                palette.syntax.current_line_bg,
            )?;
        }
        Ok(())
    }

    fn draw_selections(
        &self,
        surface: &mut RenderSurface,
        palette: &ThemePalette,
        layout: &EditorLayout,
        lines: &[EditorLine],
        selections: &[EditorSelection],
        visible: &Range<usize>,
    ) -> Result<()> {
        let origin_x = layout.text_origin_x();
        for sel in selections {
            if sel.is_empty() {
                continue;
            }
            let (start, end) = sel.normalized();
            let first = start.0.max(visible.start);
            let last = end.0.min(visible.end.saturating_sub(1));
            for line_idx in first..=last {
                let line_len = lines.get(line_idx).map_or(0, |l| l.text.chars().count());
                let c0 = if line_idx == start.0 { start.1 } else { 0 };
                let c1 = if line_idx == end.0 {
                    end.1
                } else {
                    line_len + 1
                };
                if c1 <= c0 {
                    continue;
                }
                let x = origin_x + c0 as f32 * layout.char_width;
                let w = (c1 - c0) as f32 * layout.char_width;
                let y = line_idx as f32 * layout.line_height - layout.scroll_y;
                surface.fill_rect(
                    rect_f(x, y, w, layout.line_height),
                    palette.content.selection_bg,
                )?;
            }
        }
        Ok(())
    }

    fn draw_text_lines(
        &mut self,
        surface: &mut RenderSurface,
        palette: &ThemePalette,
        layout: &EditorLayout,
        lines: &[EditorLine],
        visible: &Range<usize>,
    ) -> Result<()> {
        let spec = TextFormatSpec::new(&self.font_family, layout.font_size)
            .with_align(TextAlign::Leading)
            .with_valign(VerticalAlign::Center);
        let origin_x = layout.text_origin_x();

        surface.push_clip(rect_f(
            layout.gutter_width,
            0.0,
            (layout.viewport_width - layout.gutter_width).max(0.0),
            layout.viewport_height,
        ));
        for line_idx in visible.clone() {
            let line = &lines[line_idx];
            let dwrite_layout = self
                .line_cache
                .get_or_build(surface, line_idx, line, &spec)?;
            apply_syntax_colors(surface, palette, &dwrite_layout, &line.spans)?;
            let y = line_idx as f32 * layout.line_height - layout.scroll_y;
            surface.draw_text_layout(point_f(origin_x, y), &dwrite_layout, palette.content.text)?;
        }
        surface.pop_clip();
        Ok(())
    }

    fn draw_cursors(
        &self,
        surface: &mut RenderSurface,
        state: &EditorRenderState<'_>,
        visible: &Range<usize>,
    ) -> Result<()> {
        let layout = state.layout;
        let palette = state.palette;
        let origin_x = layout.text_origin_x();
        for cursor in state.cursors {
            if !visible.contains(&cursor.line) {
                continue;
            }
            let x = origin_x + cursor.col as f32 * layout.char_width;
            let y = cursor.line as f32 * layout.line_height - layout.scroll_y;
            match state.cursor_mode {
                CursorMode::Bar => {
                    surface.fill_rect(
                        rect_f(x, y + 2.0, 2.0, (layout.line_height - 4.0).max(2.0)),
                        palette.content.accent,
                    )?;
                }
                CursorMode::VimBlock => {
                    let block_rect = rect_f(x, y, layout.char_width.max(6.0), layout.line_height);
                    surface.fill_rect(block_rect, palette.content.accent)?;
                    if let Some(ch) = state
                        .lines
                        .get(cursor.line)
                        .and_then(|l| l.text.chars().nth(cursor.col))
                    {
                        let mut buf = [0u8; 4];
                        let s = ch.encode_utf8(&mut buf);
                        let spec = TextFormatSpec::new(&self.font_family, layout.font_size)
                            .with_align(TextAlign::Center)
                            .with_valign(VerticalAlign::Center);
                        surface.draw_text(s, block_rect, &spec, palette.content.on_accent)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn draw_gutter(
        &self,
        surface: &mut RenderSurface,
        palette: &ThemePalette,
        layout: &EditorLayout,
        cursors: &[EditorCursor],
        visible: &Range<usize>,
    ) -> Result<()> {
        surface.fill_rect(
            rect_f(0.0, 0.0, layout.gutter_width, layout.viewport_height),
            palette.grid.gutter_bg,
        )?;
        let active_line = cursors.iter().find(|c| c.is_primary).map(|c| c.line);
        let spec = TextFormatSpec::new(&self.font_family, layout.font_size - 1.0)
            .with_align(TextAlign::Trailing)
            .with_valign(VerticalAlign::Center);

        for line_idx in visible.clone() {
            let y = line_idx as f32 * layout.line_height - layout.scroll_y;
            let is_active = active_line == Some(line_idx);
            let color = if is_active {
                palette.syntax.active_line_number
            } else {
                palette.grid.gutter_text
            };
            let label = (line_idx + 1).to_string();
            surface.draw_text(
                &label,
                rect_f(
                    4.0,
                    y,
                    (layout.gutter_width - 12.0).max(4.0),
                    layout.line_height,
                ),
                &spec,
                color,
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

    fn draw_autocomplete_popup(
        &self,
        surface: &mut RenderSurface,
        palette: &ThemePalette,
        layout: &EditorLayout,
        popup: &AutocompletePopup,
    ) -> Result<()> {
        let (px, py, pw, ph) = layout.popup_rect(popup);
        let popup_rect = rect_f(px, py, pw, ph);
        surface.fill_rounded_rect(popup_rect, 6.0, palette.content.surface_elevated)?;
        surface.draw_rounded_rect(popup_rect, 6.0, palette.content.border, 1.0)?;

        let row_h = 22.0;
        let label_spec = TextFormatSpec::new(&self.font_family, layout.font_size - 0.5)
            .with_align(TextAlign::Leading)
            .with_valign(VerticalAlign::Center);
        let badge_spec = TextFormatSpec::new(&self.font_family, layout.font_size - 2.0)
            .with_weight(FontWeight::SemiBold)
            .with_align(TextAlign::Center)
            .with_valign(VerticalAlign::Center);
        let detail_spec = TextFormatSpec::new(&self.font_family, layout.font_size - 1.5)
            .with_align(TextAlign::Trailing)
            .with_valign(VerticalAlign::Center);

        for (idx, item) in popup.items.iter().take(8).enumerate() {
            let ry = py + 4.0 + idx as f32 * row_h;
            let row_rect = rect_f(px + 4.0, ry, pw - 8.0, row_h);
            if idx == popup.selected {
                surface.fill_rounded_rect(row_rect, 4.0, palette.content.selection_bg)?;
            }
            surface.draw_text(
                item.kind.badge(),
                rect_f(px + 6.0, ry, 24.0, row_h),
                &badge_spec,
                palette.content.accent,
            )?;
            surface.draw_text(
                &item.label,
                rect_f(px + 34.0, ry, (pw - 110.0).max(40.0), row_h),
                &label_spec,
                palette.content.text,
            )?;
            surface.draw_text(
                &item.detail,
                rect_f(px + pw - 76.0, ry, 68.0, row_h),
                &detail_spec,
                palette.content.text_muted,
            )?;
        }
        Ok(())
    }
}

fn apply_syntax_colors(
    surface: &mut RenderSurface,
    palette: &ThemePalette,
    dwrite_layout: &IDWriteTextLayout,
    spans: &[SyntaxSpan],
) -> Result<()> {
    for span in spans {
        if span.length == 0 || span.kind == SqlTokenKind::Whitespace {
            continue;
        }
        let color = token_color(palette, span.kind);
        let brush = surface.brush(color)?;
        let range = DWRITE_TEXT_RANGE {
            startPosition: span.start,
            length: span.length,
        };
        // SAFETY: `dwrite_layout` and `brush` are valid COM objects.
        unsafe { dwrite_layout.SetDrawingEffect(&brush, range)? };
    }
    Ok(())
}

fn token_color(palette: &ThemePalette, kind: SqlTokenKind) -> Color {
    match kind {
        SqlTokenKind::Keyword => palette.syntax.keyword,
        SqlTokenKind::Identifier | SqlTokenKind::Whitespace => palette.syntax.identifier,
        SqlTokenKind::StringLiteral => palette.syntax.string_literal,
        SqlTokenKind::Number => palette.syntax.number,
        SqlTokenKind::Comment => palette.syntax.comment,
        SqlTokenKind::Operator => palette.syntax.operator,
        SqlTokenKind::Punctuation => palette.syntax.punctuation,
        SqlTokenKind::Function => palette.syntax.function,
        SqlTokenKind::TypeName => palette.syntax.type_name,
    }
}

/// Fast incremental single-line SQL tokenizer producing UTF-16 [`SyntaxSpan`]s.
#[must_use]
pub fn tokenize_sql_line(line: &str) -> Vec<SyntaxSpan> {
    let mut spans = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut utf16_pos: u32 = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if b == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            let rem_utf16 = line[i..].encode_utf16().count() as u32;
            spans.push(SyntaxSpan {
                start: utf16_pos,
                length: rem_utf16,
                kind: SqlTokenKind::Comment,
            });
            break;
        }
        if b == b'\'' {
            let j = scan_string_literal(bytes, i);
            let len_u16 = line[i..j].encode_utf16().count() as u32;
            spans.push(SyntaxSpan {
                start: utf16_pos,
                length: len_u16,
                kind: SqlTokenKind::StringLiteral,
            });
            utf16_pos += len_u16;
            i = j;
            continue;
        }
        if b.is_ascii_whitespace() {
            let j = scan_while(bytes, i, |c| c.is_ascii_whitespace());
            let len_u16 = (j - i) as u32;
            spans.push(SyntaxSpan {
                start: utf16_pos,
                length: len_u16,
                kind: SqlTokenKind::Whitespace,
            });
            utf16_pos += len_u16;
            i = j;
            continue;
        }
        if b.is_ascii_digit() {
            let j = scan_while(bytes, i, |c| c.is_ascii_digit() || c == b'.');
            let len_u16 = (j - i) as u32;
            spans.push(SyntaxSpan {
                start: utf16_pos,
                length: len_u16,
                kind: SqlTokenKind::Number,
            });
            utf16_pos += len_u16;
            i = j;
            continue;
        }
        if b.is_ascii_alphabetic() || b == b'_' {
            let j = scan_while(bytes, i, |c| c.is_ascii_alphanumeric() || c == b'_');
            let word = &line[i..j];
            let kind = classify_word(word);
            let len_u16 = (j - i) as u32;
            spans.push(SyntaxSpan {
                start: utf16_pos,
                length: len_u16,
                kind,
            });
            utf16_pos += len_u16;
            i = j;
            continue;
        }
        let ch_len = line[i..].chars().next().map_or(1, char::len_utf8);
        let len_u16 = line[i..i + ch_len].encode_utf16().count() as u32;
        let kind = if matches!(b, b',' | b';' | b'(' | b')' | b'.') {
            SqlTokenKind::Punctuation
        } else {
            SqlTokenKind::Operator
        };
        spans.push(SyntaxSpan {
            start: utf16_pos,
            length: len_u16,
            kind,
        });
        utf16_pos += len_u16;
        i += ch_len;
    }
    spans
}

fn scan_string_literal(bytes: &[u8], start: usize) -> usize {
    let mut j = start + 1;
    while j < bytes.len() {
        if bytes[j] == b'\'' {
            j += 1;
            if j < bytes.len() && bytes[j] == b'\'' {
                j += 1;
                continue;
            }
            break;
        }
        j += 1;
    }
    j
}

fn scan_while(bytes: &[u8], start: usize, pred: impl Fn(u8) -> bool) -> usize {
    let mut j = start;
    while j < bytes.len() && pred(bytes[j]) {
        j += 1;
    }
    j
}

fn classify_word(word: &str) -> SqlTokenKind {
    let upper = word.to_ascii_uppercase();
    match upper.as_str() {
        "SELECT" | "FROM" | "WHERE" | "INSERT" | "INTO" | "VALUES" | "UPDATE" | "SET"
        | "DELETE" | "CREATE" | "ALTER" | "DROP" | "TABLE" | "INDEX" | "VIEW" | "JOIN"
        | "INNER" | "LEFT" | "RIGHT" | "FULL" | "OUTER" | "ON" | "AS" | "AND" | "OR"
        | "NOT" | "NULL" | "IS" | "IN" | "EXISTS" | "ORDER" | "BY" | "GROUP" | "HAVING"
        | "LIMIT" | "OFFSET" | "DISTINCT" | "UNION" | "ALL" | "CASE" | "WHEN" | "THEN"
        | "ELSE" | "END" | "PRIMARY" | "KEY" | "FOREIGN" | "REFERENCES" | "DEFAULT" => {
            SqlTokenKind::Keyword
        }
        "COUNT" | "SUM" | "AVG" | "MIN" | "MAX" | "COALESCE" | "NULLIF" | "LOWER" | "UPPER"
        | "NOW" | "CAST" => SqlTokenKind::Function,
        "INT" | "INTEGER" | "BIGINT" | "SMALLINT" | "TEXT" | "VARCHAR" | "CHAR" | "BOOLEAN"
        | "BOOL" | "UUID" | "JSON" | "JSONB" | "TIMESTAMP" | "TIMESTAMPTZ" | "DATE"
        | "NUMERIC" | "DECIMAL" | "FLOAT" | "DOUBLE" => SqlTokenKind::TypeName,
        _ => SqlTokenKind::Identifier,
    }
}
