//! Interactive custom GPUI SQL editor surface backed by `tableminus-state::editor`
//! (`Rope`, `UndoHistory`, `IncrementalLineHighlighter`, `VimState`, `AutocompleteState`),
//! [`crate::ui::syntax_bridge`], schema-aware autocomplete, query history / saved queries,
//! non-blocking streaming SQL execution, and vertical split pairing with the results pane.

use std::collections::BTreeMap;
use std::ops::Range;
use std::time::Instant;

use herogpui::Focusable;
use herogpui::components::{
    Button, Chip, ChipLabel, ChipVariant, Icon, IconName, IconSize, Input, InputState, ListBox,
    ListBoxItem, Popover, PopoverPlacement, ResizablePanel, ResizablePanelGroup, TabItem, Tabs,
    TabsSize, TabsVariant, Tooltip,
};
use herogpui::core::{
    Color as HeroColor, FieldVariant, Orientation as HeroOrientation, SelectionMode,
    Size as HeroSize, Variant, with_alpha,
};
use herogpui::gpui::{
    self, AnyElement, App, AppContext as _, BorderStyle, Bounds, ClipboardItem, ContentMask,
    Context, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler, FocusHandle,
    Font, FontFeatures, FontWeight, GlobalElementId, Hsla, InspectorElementId, InteractiveElement,
    IntoElement, KeyDownEvent, Keystroke, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, Pixels, Point, ScrollEvent, ShapedLine, SharedString,
    StatefulInteractiveElement, Style, Styled, TextAlign, TextRun, UTF16Selection, WeakEntity,
    Window, div, fill, outline, point, px, relative, size,
};
use herogpui::theme::ActiveTheme;
use tableminus_gfx::editor_renderer::{CursorMode, EditorCursor, EditorSelection};
use tableminus_services::{FindReplaceOptions, find_matches};
use tableminus_state::editor::{
    AutocompleteContext, AutocompleteState, CursorPosition, IncrementalLineHighlighter, Rope,
    SchemaCompletionItem, SchemaCompletionKind, UndoHistory, VimAction, VimMode, VimMotion,
    VimOperator, VimState,
};
use tableminus_state::tabs::{TabId, TabKind};
use tablepro_core::meta::ObjectKind;
use tablepro_core::sql_syntax::SqlGrammar;
use tablepro_core::{EditorSessionOptions, ResultEvent, RunOptions, RunSummary};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::shell_state::ToastSeverity;
use super::syntax_bridge::{build_line_text_runs, highlight_sql_line};
use super::theme::{self, ContentThemePalette, editor_gpui_font};
use super::{TableMinusWorkspace, actions, results_pane};

const TOP_PAD_PX: f32 = 10.0;
const BOTTOM_PAD_PX: f32 = 18.0;
const CODE_LEFT_PAD_PX: f32 = 12.0;
const CODE_RIGHT_PAD_PX: f32 = 16.0;
const MIN_GUTTER_WIDTH_PX: f32 = 46.0;
const GUTTER_RIGHT_PAD_PX: f32 = 10.0;
const GUTTER_LEFT_PAD_PX: f32 = 10.0;
const DRAWER_WIDTH_PX: f32 = 276.0;
const AUTOCOMPLETE_MAX_VISIBLE: usize = 8;
const AUTOCOMPLETE_WIDTH_PX: f32 = 316.0;
const AUTOCOMPLETE_ROW_HEIGHT_PX: f32 = 28.0;
const BRACKET_SEARCH_LIMIT: usize = 4096;

const SQL_KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "JOIN",
    "INNER JOIN",
    "LEFT JOIN",
    "RIGHT JOIN",
    "FULL JOIN",
    "CROSS JOIN",
    "ON",
    "GROUP BY",
    "ORDER BY",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "AS",
    "DISTINCT",
    "UNION",
    "UNION ALL",
    "WITH",
    "RETURNING",
    "AND",
    "OR",
    "NOT",
    "IN",
    "EXISTS",
    "BETWEEN",
    "LIKE",
    "ILIKE",
    "IS NULL",
    "IS NOT NULL",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "CREATE TABLE",
    "ALTER TABLE",
    "DROP TABLE",
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "EXPLAIN",
    "ANALYZE",
];

#[derive(Debug, Clone)]
struct TabDocSnapshot {
    text: String,
    cursors: Vec<CursorPosition>,
    scroll_y: f32,
    scroll_x: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum EditorDrawerTab {
    #[default]
    Saved,
    History,
}

#[derive(Clone)]
struct ShapedEditorRow {
    line_idx: usize,
    text: String,
    shaped: ShapedLine,
    gutter_shaped: ShapedLine,
}

#[derive(Clone)]
struct EditorPrepaintState {
    first_line: usize,
    gutter_width_px: f32,
    line_height_px: f32,
    rows: Vec<ShapedEditorRow>,
    selection_quads: Vec<(Bounds<Pixels>, Hsla)>,
    caret_quads: Vec<(Bounds<Pixels>, Hsla)>,
    bracket_quads: Vec<(Bounds<Pixels>, Hsla)>,
    active_line_band: Option<Bounds<Pixels>>,
    empty_hint: Option<(ShapedLine, Point<Pixels>)>,
    marked_underline: Option<(Point<Pixels>, Pixels, Hsla)>,
}

/// Persistent UI state for the SQL editor surface.
#[derive(Debug)]
pub struct SqlEditorViewState {
    pub highlighter: IncrementalLineHighlighter,
    doc: Entity<SqlEditorDocEntity>,
    show_drawer: bool,
    drawer_tab: EditorDrawerTab,
    drawer_search_input: Entity<InputState>,
    save_popover_open: bool,
    save_name_input: Entity<InputState>,
    find_replace_open: bool,
    find_input: Entity<InputState>,
    replace_input: Entity<InputState>,
    case_sensitive: bool,
    whole_word: bool,
    match_count: usize,
    active_match_idx: usize,
    find_matches_positions: Vec<Range<usize>>,
}

impl SqlEditorViewState {
    /// Initializes SQL editor rendering and input state owned by [`TableMinusWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, cx: &mut Context<TableMinusWorkspace>) -> Self {
        let weak_ws = cx.weak_entity();
        let doc = cx.new(|cx| SqlEditorDocEntity::new(weak_ws, cx));
        let drawer_search_input = cx.new(|cx| InputState::new(cx));
        let save_name_input = cx.new(|cx| InputState::new(cx));
        let find_input = cx.new(|cx| InputState::new(cx));
        let replace_input = cx.new(|cx| InputState::new(cx));
        Self {
            highlighter: IncrementalLineHighlighter::new(SqlGrammar::PostgreSql),
            doc,
            show_drawer: false,
            drawer_tab: EditorDrawerTab::Saved,
            drawer_search_input,
            save_popover_open: false,
            save_name_input,
            find_replace_open: false,
            find_input,
            replace_input,
            case_sensitive: false,
            whole_word: false,
            match_count: 0,
            active_match_idx: 0,
            find_matches_positions: Vec::new(),
        }
    }

    /// Returns the live SQL text buffer currently in the editor.
    #[must_use]
    pub fn current_text(&self, cx: &App) -> String {
        self.doc.read(cx).rope.text()
    }

    /// Collects unsaved SQL drafts across all open query tabs, including the active editor rope.
    #[must_use]
    pub fn collect_tab_drafts(&self, cx: &App) -> std::collections::HashMap<TabId, String> {
        let doc = self.doc.read(cx);
        let mut drafts: std::collections::HashMap<TabId, String> = doc
            .tab_buffers
            .iter()
            .map(|(id, snap)| (*id, snap.text.clone()))
            .collect();
        if let Some(active_id) = doc.active_tab_id {
            drafts.insert(active_id, doc.rope.text());
        }
        drafts
    }
}

/// Entity owning the live SQL buffer, rope, undo stack, Vim mode, autocomplete, and IME state.
pub struct SqlEditorDocEntity {
    rope: Rope,
    line_starts: Vec<usize>,
    cursors: Vec<CursorPosition>,
    history: UndoHistory,
    highlighter: IncrementalLineHighlighter,
    grammar: SqlGrammar,
    vim: VimState,
    autocomplete: AutocompleteState,
    marked_range_utf16: Option<Range<usize>>,
    focus_handle: FocusHandle,
    scroll_y: f32,
    scroll_x: f32,
    font: Font,
    font_size_px: f32,
    line_height_px: f32,
    char_advance_px: f32,
    gutter_width_px: f32,
    last_bounds: Bounds<Pixels>,
    dragging: bool,
    palette: ContentThemePalette,
    weak_workspace: Option<WeakEntity<TableMinusWorkspace>>,
    tab_buffers: BTreeMap<TabId, TabDocSnapshot>,
    active_tab_id: Option<TabId>,
    last_controller_sql: String,
    catalog_fingerprint: u64,
    suppress_autocomplete: bool,
    last_shaped_rows: Vec<ShapedEditorRow>,
    empty_hint_text: &'static str,
    autocomplete_insert_hint: &'static str,
    autocomplete_dismiss_hint: &'static str,
}

impl SqlEditorDocEntity {
    fn new(weak_workspace: WeakEntity<TableMinusWorkspace>, cx: &mut Context<Self>) -> Self {
        let rope = Rope::new();
        let line_starts = compute_line_starts(&rope);
        let default_strings = crate::i18n::Strings::for_locale(crate::i18n::Locale::EnUs);
        Self {
            rope,
            line_starts,
            cursors: vec![CursorPosition::caret(0)],
            history: UndoHistory::new(),
            highlighter: IncrementalLineHighlighter::new(SqlGrammar::PostgreSql),
            grammar: SqlGrammar::PostgreSql,
            vim: VimState::new(false),
            autocomplete: AutocompleteState::with_catalog(Vec::new()),
            marked_range_utf16: None,
            focus_handle: cx.focus_handle(),
            scroll_y: 0.0,
            scroll_x: 0.0,
            font: gpui::font("Cascadia Code"),
            font_size_px: 13.5,
            line_height_px: 20.5,
            char_advance_px: 8.1,
            gutter_width_px: MIN_GUTTER_WIDTH_PX,
            last_bounds: Bounds::default(),
            dragging: false,
            palette: ContentThemePalette::from_definition(
                &tableminus_state::theme::ThemeDefinition::builtin_dark(),
            ),
            weak_workspace: Some(weak_workspace),
            tab_buffers: BTreeMap::new(),
            active_tab_id: None,
            last_controller_sql: String::new(),
            catalog_fingerprint: 0,
            suppress_autocomplete: false,
            last_shaped_rows: Vec::new(),
            empty_hint_text: default_strings.sql_editor_placeholder,
            autocomplete_insert_hint: default_strings.autocomplete_insert_hint,
            autocomplete_dismiss_hint: default_strings.autocomplete_dismiss_hint,
        }
    }

    fn primary_cursor(&self) -> CursorPosition {
        self.cursors
            .first()
            .copied()
            .unwrap_or_else(|| CursorPosition::caret(0))
    }

    fn rebuild_line_starts(&mut self) {
        self.line_starts = compute_line_starts(&self.rope);
        let max_len = self.rope.len_chars();
        for c in &mut self.cursors {
            c.anchor = c.anchor.min(max_len);
            c.head = c.head.min(max_len);
        }
        if self.cursors.is_empty() {
            self.cursors.push(CursorPosition::caret(0));
        }
    }

    fn char_to_line_col(&self, char_idx: usize) -> (usize, usize) {
        let clamped = char_idx.min(self.rope.len_chars());
        let line = self
            .line_starts
            .partition_point(|&start| start <= clamped)
            .saturating_sub(1);
        let col = clamped.saturating_sub(self.line_starts[line]);
        (line, col)
    }

    fn line_range_chars(&self, line_idx: usize) -> (usize, usize) {
        let count = self.line_starts.len();
        if count == 0 || line_idx >= count {
            let end = self.rope.len_chars();
            return (end, end);
        }
        let start = self.line_starts[line_idx];
        let end = if line_idx + 1 < count {
            self.line_starts[line_idx + 1].saturating_sub(1)
        } else {
            self.rope.len_chars()
        };
        (start, end.max(start))
    }

    fn line_col_to_char(&self, line_idx: usize, col: usize) -> usize {
        let line = line_idx.min(self.line_starts.len().saturating_sub(1));
        let (start, end) = self.line_range_chars(line);
        (start + col).min(end)
    }

    fn after_text_mutation(&mut self, update_ac: bool, cx: &mut Context<Self>) {
        self.rebuild_line_starts();
        let last_line = self.line_starts.len().saturating_sub(1);
        self.highlighter.invalidate_lines(0, last_line);
        self.ensure_primary_caret_visible();
        if update_ac {
            self.refresh_autocomplete_from_cursor();
        }
        self.push_sync_to_workspace(cx);
        cx.notify();
    }

    fn ensure_primary_caret_visible(&mut self) {
        let (line, col) = self.char_to_line_col(self.primary_cursor().head);
        let view_h = f32::from(self.last_bounds.size.height);
        if view_h > self.line_height_px * 2.0 {
            let top = line as f32 * self.line_height_px;
            let bottom = top + self.line_height_px + TOP_PAD_PX + BOTTOM_PAD_PX;
            if top < self.scroll_y {
                self.scroll_y = top;
            } else if bottom > self.scroll_y + view_h {
                self.scroll_y = (bottom - view_h).max(0.0);
            }
        }
        let code_w = (f32::from(self.last_bounds.size.width)
            - self.gutter_width_px
            - CODE_LEFT_PAD_PX
            - CODE_RIGHT_PAD_PX)
            .max(40.0);
        let approx_x = col as f32 * self.char_advance_px;
        if approx_x < self.scroll_x {
            self.scroll_x = approx_x.max(0.0);
        } else if approx_x + 24.0 > self.scroll_x + code_w {
            self.scroll_x = (approx_x + 24.0 - code_w).max(0.0);
        }
    }

    fn refresh_autocomplete_from_cursor(&mut self) {
        if self.suppress_autocomplete || (self.vim.enabled && self.vim.mode != VimMode::Insert) {
            self.autocomplete.dismiss();
            return;
        }
        let head = self.primary_cursor().head;
        let prefix_opt = identifier_prefix_before(&self.rope, head);
        let context = detect_sql_autocomplete_context(&self.rope, head, prefix_opt.as_deref());
        match prefix_opt {
            Some(prefix) => self
                .autocomplete
                .update_prefix_with_context(&prefix, context),
            None if context == AutocompleteContext::TableExpecting => {
                self.autocomplete
                    .update_prefix_with_context("", AutocompleteContext::TableExpecting);
            }
            None => self.autocomplete.dismiss(),
        }
    }

    fn push_sync_to_workspace(&mut self, cx: &mut Context<Self>) {
        let text = self.rope.text();
        self.last_controller_sql = text.clone();
        if let Some(tab_id) = self.active_tab_id {
            self.tab_buffers.insert(
                tab_id,
                TabDocSnapshot {
                    text: text.clone(),
                    cursors: self.cursors.clone(),
                    scroll_y: self.scroll_y,
                    scroll_x: self.scroll_x,
                },
            );
        }
        let Some(weak_ws) = self.weak_workspace.clone() else {
            return;
        };
        let cursors: Vec<EditorCursor> = self
            .cursors
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let (line, col) = self.char_to_line_col(c.head);
                EditorCursor {
                    line,
                    col,
                    is_primary: i == 0,
                }
            })
            .collect();
        let selections: Vec<EditorSelection> = self
            .cursors
            .iter()
            .filter(|c| c.anchor != c.head)
            .map(|c| EditorSelection {
                anchor: self.char_to_line_col(c.anchor),
                head: self.char_to_line_col(c.head),
            })
            .collect();
        let vim_mode = self.vim.enabled.then(|| to_ctrl_vim_mode(self.vim.mode));
        let cursor_mode = match self.vim.enabled {
            true if self.vim.mode != VimMode::Insert => CursorMode::VimBlock,
            _ => CursorMode::Bar,
        };
        let _ = weak_ws.update(cx, |ws, cx| {
            ws.controller.set_editor_sql(&text);
            ws.controller.editor_cursors = cursors;
            ws.controller.editor_selections = selections;
            ws.controller.vim_mode = vim_mode;
            ws.controller.editor_cursor_mode = cursor_mode;
            ws.sql_editor_ui.highlighter.invalidate_lines(0, usize::MAX);
            cx.notify();
        });
    }

    fn apply_insert(&mut self, text: &str, cx: &mut Context<Self>) {
        self.suppress_autocomplete = text.ends_with([' ', ';', '(', ')', ',', '\n']);
        self.history
            .apply_multi_caret_insert(&mut self.rope, &mut self.cursors, text);
        dedup_collapsed_cursors(&mut self.cursors);
        self.after_text_mutation(!self.suppress_autocomplete, cx);
    }

    fn apply_auto_indent_newline(&mut self, cx: &mut Context<Self>) {
        let (line_idx, _) = self.char_to_line_col(self.primary_cursor().head);
        let indent: String = self
            .rope
            .line(line_idx)
            .unwrap_or_default()
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let nl = format!("\n{indent}");
        self.suppress_autocomplete = true;
        self.apply_insert(&nl, cx);
    }

    fn apply_backspace(&mut self, by_word: bool, cx: &mut Context<Self>) {
        let len = self.rope.len_chars();
        for cursor in &mut self.cursors {
            if cursor.anchor == cursor.head && cursor.head > 0 {
                let target = if by_word {
                    prev_word_boundary(&self.rope, cursor.head)
                } else {
                    cursor.head.saturating_sub(1)
                };
                cursor.anchor = target.min(len);
            }
        }
        self.suppress_autocomplete = false;
        self.history
            .apply_multi_caret_insert(&mut self.rope, &mut self.cursors, "");
        dedup_collapsed_cursors(&mut self.cursors);
        self.after_text_mutation(true, cx);
    }

    fn apply_delete_forward(&mut self, by_word: bool, cx: &mut Context<Self>) {
        let len = self.rope.len_chars();
        for cursor in &mut self.cursors {
            if cursor.anchor == cursor.head && cursor.head < len {
                let target = if by_word {
                    next_word_boundary(&self.rope, cursor.head)
                } else {
                    (cursor.head + 1).min(len)
                };
                cursor.anchor = cursor.head;
                cursor.head = target;
            }
        }
        self.suppress_autocomplete = true;
        self.history
            .apply_multi_caret_insert(&mut self.rope, &mut self.cursors, "");
        dedup_collapsed_cursors(&mut self.cursors);
        self.after_text_mutation(false, cx);
    }

    fn apply_accept_autocomplete(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(item) = self.autocomplete.accept() else {
            return false;
        };
        let prefix_len = self
            .primary_cursor()
            .head
            .min(self.rope.len_chars())
            .checked_sub(0)
            .and_then(|head| identifier_prefix_before(&self.rope, head).map(|p| p.chars().count()))
            .unwrap_or(0);
        for cursor in &mut self.cursors {
            if cursor.anchor == cursor.head {
                cursor.anchor = cursor.head.saturating_sub(prefix_len);
            }
        }
        let should_chain_table_popup = item.kind == SchemaCompletionKind::Snippet
            && (item.insert_text.ends_with("FROM ") || item.insert_text.ends_with("JOIN "));
        self.suppress_autocomplete = !should_chain_table_popup;
        self.history
            .apply_multi_caret_insert(&mut self.rope, &mut self.cursors, &item.insert_text);
        dedup_collapsed_cursors(&mut self.cursors);
        self.after_text_mutation(false, cx);
        if should_chain_table_popup {
            self.refresh_autocomplete_from_cursor();
        }
        true
    }

    fn accept_autocomplete_at(&mut self, idx: usize, cx: &mut Context<Self>) -> bool {
        if idx < self.autocomplete.items().len() {
            self.autocomplete.selected_index = idx;
            return self.apply_accept_autocomplete(cx);
        }
        false
    }

    fn apply_undo(&mut self, cx: &mut Context<Self>) {
        if self.history.undo(&mut self.rope, &mut self.cursors) {
            self.suppress_autocomplete = true;
            self.after_text_mutation(false, cx);
        }
    }

    fn apply_redo(&mut self, cx: &mut Context<Self>) {
        if self.history.redo(&mut self.rope, &mut self.cursors) {
            self.suppress_autocomplete = true;
            self.after_text_mutation(false, cx);
        }
    }

    fn select_all(&mut self, cx: &mut Context<Self>) {
        self.cursors = vec![CursorPosition {
            anchor: 0,
            head: self.rope.len_chars(),
        }];
        self.autocomplete.dismiss();
        self.push_sync_to_workspace(cx);
        cx.notify();
    }

    fn copy_selection(&self, cx: &mut App) -> String {
        let mut parts = Vec::new();
        for c in &self.cursors {
            let (s, e) = c.range();
            if e > s {
                parts.push(self.rope.slice(s, e));
            }
        }
        let text = if parts.is_empty() {
            let (line, _) = self.char_to_line_col(self.primary_cursor().head);
            self.rope.line(line).unwrap_or_default()
        } else {
            parts.join("\n")
        };
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        }
        text
    }

    fn cut_selection(&mut self, cx: &mut Context<Self>) {
        let has_sel = self.cursors.iter().any(|c| c.anchor != c.head);
        if has_sel {
            let _ = self.copy_selection(cx);
            self.history
                .apply_multi_caret_insert(&mut self.rope, &mut self.cursors, "");
            self.after_text_mutation(false, cx);
        }
    }

    fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(item) = cx.read_from_clipboard()
            && let Some(text) = item.text()
        {
            self.apply_insert(&text, cx);
        }
    }

    fn select_next_occurrence(&mut self, cx: &mut Context<Self>) {
        let mut primary = self.primary_cursor();
        if primary.anchor == primary.head {
            let (ws, we) = word_range_at(&self.rope, primary.head);
            if we > ws {
                primary = CursorPosition {
                    anchor: ws,
                    head: we,
                };
                if let Some(first) = self.cursors.first_mut() {
                    *first = primary;
                }
                self.push_sync_to_workspace(cx);
                cx.notify();
            }
            return;
        }
        let (s, e) = primary.range();
        let needle = self.rope.slice(s, e);
        if needle.is_empty() {
            return;
        }
        let full = self.rope.text();
        let search_from_char = self.cursors.iter().map(|c| c.range().1).max().unwrap_or(e);
        let byte_start = char_to_byte_idx(&full, search_from_char);
        let found_byte = full[byte_start..]
            .find(&needle)
            .map(|rel| byte_start + rel)
            .or_else(|| full.find(&needle));
        if let Some(b_idx) = found_byte {
            let start_char = full[..b_idx].chars().count();
            let end_char = start_char + needle.chars().count();
            let cand = CursorPosition {
                anchor: start_char,
                head: end_char,
            };
            if !self.cursors.contains(&cand) {
                self.cursors.push(cand);
                self.ensure_primary_caret_visible();
                self.push_sync_to_workspace(cx);
                cx.notify();
            }
        }
    }

    fn move_cursors(&mut self, dir: MoveDir, extend: bool, by_word: bool, cx: &mut Context<Self>) {
        let len = self.rope.len_chars();
        let line_count = self.line_starts.len();
        let page_lines = ((f32::from(self.last_bounds.size.height) / self.line_height_px).floor()
            as usize)
            .saturating_sub(2)
            .max(4);
        let snapshot_starts = self.line_starts.clone();
        for idx in 0..self.cursors.len() {
            let cur = self.cursors[idx];
            let next_head = match dir {
                MoveDir::Left => {
                    if !extend && cur.anchor != cur.head {
                        cur.range().0
                    } else if by_word {
                        prev_word_boundary(&self.rope, cur.head)
                    } else {
                        cur.head.saturating_sub(1)
                    }
                }
                MoveDir::Right => {
                    if !extend && cur.anchor != cur.head {
                        cur.range().1
                    } else if by_word {
                        next_word_boundary(&self.rope, cur.head)
                    } else {
                        (cur.head + 1).min(len)
                    }
                }
                MoveDir::Up => move_vertical(cur.head, -1, &self.rope, &snapshot_starts),
                MoveDir::Down => move_vertical(cur.head, 1, &self.rope, &snapshot_starts),
                MoveDir::PageUp => move_vertical(
                    cur.head,
                    -(page_lines as isize),
                    &self.rope,
                    &snapshot_starts,
                ),
                MoveDir::PageDown => {
                    move_vertical(cur.head, page_lines as isize, &self.rope, &snapshot_starts)
                }
                MoveDir::LineStart => {
                    let (line, _) = self.char_to_line_col(cur.head);
                    snapshot_starts.get(line).copied().unwrap_or(0)
                }
                MoveDir::LineEnd => {
                    let (line, _) = self.char_to_line_col(cur.head);
                    if line + 1 < line_count {
                        snapshot_starts[line + 1].saturating_sub(1)
                    } else {
                        len
                    }
                }
                MoveDir::DocStart => 0,
                MoveDir::DocEnd => len,
            };
            self.cursors[idx].head = next_head.min(len);
            if !extend {
                self.cursors[idx].anchor = self.cursors[idx].head;
            }
        }
        if !extend {
            dedup_collapsed_cursors(&mut self.cursors);
        }
        self.autocomplete.dismiss();
        self.ensure_primary_caret_visible();
        self.push_sync_to_workspace(cx);
        cx.notify();
    }

    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ks = &event.keystroke;
        if self.handle_shortcut_key(ks, window, cx) {
            cx.stop_propagation();
            return;
        }
        if self.handle_autocomplete_key(ks, cx) {
            cx.stop_propagation();
            return;
        }
        if self.vim.enabled && self.vim.mode != VimMode::Insert {
            if self.handle_vim_key(ks, cx) {
                cx.stop_propagation();
            }
            return;
        }
        if self.handle_standard_nav_key(ks, cx) {
            cx.stop_propagation();
        }
    }

    fn handle_shortcut_key(
        &mut self,
        ks: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let key = ks.key.as_str();
        let ctrl = ks.modifiers.control || ks.modifiers.platform;
        let shift = ks.modifiers.shift;
        if ctrl && !ks.modifiers.alt {
            match key {
                "space" => {
                    let head = self.primary_cursor().head;
                    let prefix_opt = identifier_prefix_before(&self.rope, head);
                    let context =
                        detect_sql_autocomplete_context(&self.rope, head, prefix_opt.as_deref());
                    self.suppress_autocomplete = false;
                    self.autocomplete
                        .update_prefix_with_context(prefix_opt.as_deref().unwrap_or(""), context);
                    cx.notify();
                    return true;
                }
                "z" if !shift => {
                    self.apply_undo(cx);
                    return true;
                }
                "z" if shift => {
                    self.apply_redo(cx);
                    return true;
                }
                "y" => {
                    self.apply_redo(cx);
                    return true;
                }
                "a" => {
                    self.select_all(cx);
                    return true;
                }
                "c" => {
                    let _ = self.copy_selection(cx);
                    return true;
                }
                "x" => {
                    self.cut_selection(cx);
                    return true;
                }
                "v" => {
                    self.paste_clipboard(cx);
                    return true;
                }
                "d" => {
                    self.select_next_occurrence(cx);
                    return true;
                }
                "f" | "h" => {
                    let selected = {
                        let (s, e) = self.primary_cursor().range();
                        (e > s).then(|| self.rope.slice(s, e))
                    };
                    let doc_text = self.rope.text();
                    if let Some(weak) = self.weak_workspace.clone() {
                        let _ = weak.update(cx, |ws, cx| {
                            open_find_replace_with_text(ws, window, &doc_text, selected, cx);
                        });
                    }
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    fn handle_autocomplete_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        if !self.autocomplete.is_open {
            return false;
        }
        let ctrl = ks.modifiers.control || ks.modifiers.platform;
        match ks.key.as_str() {
            "down" => {
                self.autocomplete.move_next();
                cx.notify();
                true
            }
            "up" => {
                self.autocomplete.move_prev();
                cx.notify();
                true
            }
            "n" if ctrl => {
                self.autocomplete.move_next();
                cx.notify();
                true
            }
            "p" if ctrl => {
                self.autocomplete.move_prev();
                cx.notify();
                true
            }
            "tab" | "enter" if !ctrl => self.apply_accept_autocomplete(cx),
            "escape" => {
                self.autocomplete.dismiss();
                self.suppress_autocomplete = true;
                cx.notify();
                true
            }
            _ => false,
        }
    }

    fn handle_vim_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        if let Some(token) = keystroke_to_vim_token(ks) {
            let prev_mode = self.vim.mode;
            let action = self.vim.handle_key(&token);
            if prev_mode != self.vim.mode || action != VimAction::None {
                self.execute_vim_action(action, &token, cx);
                return true;
            }
            if token == "p" {
                let text = self.vim.register.clone();
                if !text.is_empty() {
                    self.apply_insert(&text, cx);
                    return true;
                }
            } else if token == "x" {
                self.apply_delete_forward(false, cx);
                return true;
            } else if token == "u" {
                self.apply_undo(cx);
                return true;
            }
        }
        false
    }

    fn execute_vim_action(&mut self, action: VimAction, token: &str, cx: &mut Context<Self>) {
        let extend = self.vim.mode == VimMode::Visual;
        match action {
            VimAction::None => {
                if token == "a" {
                    self.move_cursors(MoveDir::Right, false, false, cx);
                } else if token == "o" {
                    self.move_cursors(MoveDir::LineEnd, false, false, cx);
                    self.apply_auto_indent_newline(cx);
                } else if token == "O" {
                    self.move_cursors(MoveDir::LineStart, false, false, cx);
                    self.apply_insert("\n", cx);
                    self.move_cursors(MoveDir::Up, false, false, cx);
                }
                self.push_sync_to_workspace(cx);
                cx.notify();
            }
            VimAction::Move(motion, count) => {
                for _ in 0..count {
                    apply_vim_motion(self, motion, extend, cx);
                }
            }
            VimAction::ApplyOperator(op, motion, count) => {
                let start = self.primary_cursor().head;
                for _ in 0..count {
                    apply_vim_motion(self, motion, true, cx);
                }
                let end = self.primary_cursor().head;
                let (lo, hi) = if start <= end {
                    (start, end)
                } else {
                    (end, start)
                };
                self.apply_vim_range_operator(op, lo, hi, cx);
            }
            VimAction::LineOperator(op, count) => {
                let (line_idx, _) = self.char_to_line_col(self.primary_cursor().head);
                let start = self.line_starts.get(line_idx).copied().unwrap_or(0);
                let end_line = (line_idx + count).min(self.line_starts.len());
                let end = if end_line < self.line_starts.len() {
                    self.line_starts[end_line]
                } else {
                    self.rope.len_chars()
                };
                self.apply_vim_range_operator(op, start, end, cx);
            }
            VimAction::ExecuteCommand(cmd) => {
                let trimmed = cmd.trim();
                if let Some(weak_ws) = self.weak_workspace.clone() {
                    let _ = weak_ws.update(cx, |ws, cx| {
                        if trimmed == "w" {
                            ws.sql_editor_ui.save_popover_open = true;
                        } else if matches!(trimmed, "run" | "%" | "r") {
                            trigger_run_query(ws, false, cx);
                        } else if trimmed == "q!" {
                            ws.sql_editor_ui.doc.update(cx, |doc, cx| {
                                doc.rope = Rope::new();
                                doc.cursors = vec![CursorPosition::caret(0)];
                                doc.after_text_mutation(false, cx);
                            });
                        }
                        cx.notify();
                    });
                }
                self.push_sync_to_workspace(cx);
                cx.notify();
            }
        }
    }

    fn apply_vim_range_operator(
        &mut self,
        op: VimOperator,
        lo: usize,
        hi: usize,
        cx: &mut Context<Self>,
    ) {
        if hi <= lo {
            self.push_sync_to_workspace(cx);
            cx.notify();
            return;
        }
        self.vim.register = self.rope.slice(lo, hi);
        match op {
            VimOperator::Yank => {
                cx.write_to_clipboard(ClipboardItem::new_string(self.vim.register.clone()));
                self.cursors = vec![CursorPosition::caret(lo)];
                self.push_sync_to_workspace(cx);
                cx.notify();
            }
            VimOperator::Delete | VimOperator::Change => {
                self.cursors = vec![CursorPosition {
                    anchor: lo,
                    head: hi,
                }];
                self.history
                    .apply_multi_caret_insert(&mut self.rope, &mut self.cursors, "");
                self.after_text_mutation(false, cx);
            }
        }
    }

    fn handle_standard_nav_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        let key = ks.key.as_str();
        let ctrl = ks.modifiers.control || ks.modifiers.platform;
        let shift = ks.modifiers.shift;
        if self.vim.enabled && self.vim.mode == VimMode::Insert && key == "escape" {
            let _ = self.vim.handle_key("Esc");
            self.autocomplete.dismiss();
            self.push_sync_to_workspace(cx);
            cx.notify();
            return true;
        }
        match key {
            "left" => self.move_cursors(MoveDir::Left, shift, ctrl, cx),
            "right" => self.move_cursors(MoveDir::Right, shift, ctrl, cx),
            "up" => self.move_cursors(MoveDir::Up, shift, false, cx),
            "down" => self.move_cursors(MoveDir::Down, shift, false, cx),
            "home" => {
                self.move_cursors(
                    if ctrl {
                        MoveDir::DocStart
                    } else {
                        MoveDir::LineStart
                    },
                    shift,
                    false,
                    cx,
                );
            }
            "end" => {
                self.move_cursors(
                    if ctrl {
                        MoveDir::DocEnd
                    } else {
                        MoveDir::LineEnd
                    },
                    shift,
                    false,
                    cx,
                );
            }
            "pageup" => self.move_cursors(MoveDir::PageUp, shift, false, cx),
            "pagedown" => self.move_cursors(MoveDir::PageDown, shift, false, cx),
            "backspace" => self.apply_backspace(ctrl, cx),
            "delete" => self.apply_delete_forward(ctrl, cx),
            "enter" if !ctrl && !shift && !ks.modifiers.alt => self.apply_auto_indent_newline(cx),
            "tab" if !ctrl && !ks.modifiers.alt => self.apply_insert("    ", cx),
            _ => return false,
        }
        true
    }

    fn char_index_for_local_point(&self, pos: Point<Pixels>) -> usize {
        let rel_y = f32::from(pos.y) - f32::from(self.last_bounds.origin.y) - TOP_PAD_PX;
        let line = ((rel_y + self.scroll_y) / self.line_height_px)
            .floor()
            .max(0.0) as usize;
        let line = line.min(self.line_starts.len().saturating_sub(1));
        let rel_x = f32::from(pos.x)
            - f32::from(self.last_bounds.origin.x)
            - self.gutter_width_px
            - CODE_LEFT_PAD_PX
            + self.scroll_x;
        let col = if let Some(row) = self.last_shaped_rows.iter().find(|r| r.line_idx == line) {
            let byte_idx = row.shaped.closest_index_for_x(px(rel_x.max(0.0)));
            row.text[..byte_idx.min(row.text.len())].chars().count()
        } else {
            (rel_x.max(0.0) / self.char_advance_px).round() as usize
        };
        self.line_col_to_char(line, col)
    }

    fn handle_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Left {
            return;
        }
        window.focus(&self.focus_handle, cx);
        let rel_x = f32::from(event.position.x) - f32::from(self.last_bounds.origin.x);
        if rel_x < self.gutter_width_px {
            let rel_y =
                f32::from(event.position.y) - f32::from(self.last_bounds.origin.y) - TOP_PAD_PX;
            let line = ((rel_y + self.scroll_y) / self.line_height_px)
                .floor()
                .max(0.0) as usize;
            let (start, end) = self.line_range_chars(line);
            self.cursors = vec![CursorPosition {
                anchor: start,
                head: end,
            }];
            self.dragging = true;
            self.autocomplete.dismiss();
            self.push_sync_to_workspace(cx);
            cx.notify();
            return;
        }
        let char_idx = self.char_index_for_local_point(event.position);
        if event.modifiers.alt {
            if !self.cursors.iter().any(|c| c.head == char_idx) {
                self.cursors.push(CursorPosition::caret(char_idx));
            }
        } else if event.click_count >= 2 {
            let (ws, we) = word_range_at(&self.rope, char_idx);
            self.cursors = vec![CursorPosition {
                anchor: ws,
                head: we,
            }];
        } else if event.modifiers.shift {
            if let Some(primary) = self.cursors.first_mut() {
                primary.head = char_idx;
            }
        } else {
            self.cursors = vec![CursorPosition::caret(char_idx)];
            self.dragging = true;
        }
        self.autocomplete.dismiss();
        self.push_sync_to_workspace(cx);
        cx.notify();
    }

    fn handle_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.dragging || !event.pressed_button.is_some_and(|b| b == MouseButton::Left) {
            self.dragging = false;
            return;
        }
        let char_idx = self.char_index_for_local_point(event.position);
        if let Some(primary) = self.cursors.first_mut()
            && primary.head != char_idx
        {
            primary.head = char_idx;
            self.push_sync_to_workspace(cx);
            cx.notify();
        }
    }

    fn handle_mouse_up(&mut self, _event: &MouseUpEvent, cx: &mut Context<Self>) {
        if self.dragging {
            self.dragging = false;
            cx.notify();
        }
    }

    fn handle_scroll_wheel(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(px(self.line_height_px * 3.0));
        let line_count = self.line_starts.len().max(1);
        let content_h = TOP_PAD_PX + line_count as f32 * self.line_height_px + BOTTOM_PAD_PX;
        let view_h = f32::from(self.last_bounds.size.height).max(self.line_height_px);
        let max_scroll_y = (content_h - view_h).max(0.0);
        if event.modifiers.shift {
            self.scroll_x = (self.scroll_x - f32::from(delta.y)).max(0.0);
        } else {
            self.scroll_y = (self.scroll_y - f32::from(delta.y)).clamp(0.0, max_scroll_y);
            self.scroll_x = (self.scroll_x - f32::from(delta.x)).max(0.0);
        }
        cx.notify();
    }

    fn caret_overlay_position(&self) -> (f32, f32) {
        let (line, col) = self.char_to_line_col(self.primary_cursor().head);
        let x_offset = self
            .last_shaped_rows
            .iter()
            .find(|r| r.line_idx == line)
            .map(|row| {
                let byte_idx = char_to_byte_idx(&row.text, col);
                f32::from(row.shaped.x_for_index(byte_idx))
            })
            .unwrap_or_else(|| col as f32 * self.char_advance_px);
        let raw_x = self.gutter_width_px + CODE_LEFT_PAD_PX + x_offset - self.scroll_x;
        let raw_y = TOP_PAD_PX + (line as f32 + 1.0) * self.line_height_px - self.scroll_y + 4.0;
        let bounds_w = f32::from(self.last_bounds.size.width).max(AUTOCOMPLETE_WIDTH_PX + 16.0);
        let x = raw_x.clamp(8.0, (bounds_w - AUTOCOMPLETE_WIDTH_PX - 8.0).max(8.0));
        let y = raw_y.max(self.line_height_px);
        (x, y)
    }
}

impl EntityInputHandler for SqlEditorDocEntity {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let total = self.rope.len_utf16();
        let start_u16 = range.start.min(range.end).min(total);
        let end_u16 = range.start.max(range.end).min(total);
        *adjusted_range = Some(start_u16..end_u16);
        let start_c = self.rope.utf16_to_char(start_u16);
        let end_c = self.rope.utf16_to_char(end_u16);
        Some(self.rope.slice(start_c, end_c))
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let primary = self.primary_cursor();
        let (s, e) = primary.range();
        Some(UTF16Selection {
            range: self.rope.char_to_utf16(s)..self.rope.char_to_utf16(e),
            reversed: primary.head < primary.anchor,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range_utf16.clone()
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.marked_range_utf16.take().is_some() {
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.vim.enabled && self.vim.mode != VimMode::Insert {
            return;
        }
        let target_u16 = range.or_else(|| self.marked_range_utf16.take());
        if let Some(r_u16) = target_u16 {
            let total = self.rope.len_utf16();
            let s_c = self.rope.utf16_to_char(r_u16.start.min(total));
            let e_c = self.rope.utf16_to_char(r_u16.end.min(total));
            self.cursors = vec![CursorPosition {
                anchor: s_c.min(e_c),
                head: s_c.max(e_c),
            }];
        }
        self.marked_range_utf16 = None;
        self.apply_insert(text, cx);
        window.notify_text_input_changed(&self.focus_handle);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.vim.enabled && self.vim.mode != VimMode::Insert {
            return;
        }
        let replace_u16 = range
            .or_else(|| self.marked_range_utf16.clone())
            .unwrap_or_else(|| {
                let (s, e) = self.primary_cursor().range();
                self.rope.char_to_utf16(s)..self.rope.char_to_utf16(e)
            });
        let total = self.rope.len_utf16();
        let start_u16 = replace_u16.start.min(total);
        let start_c = self.rope.utf16_to_char(start_u16);
        let end_c = self.rope.utf16_to_char(replace_u16.end.min(total));
        self.rope.delete(start_c.min(end_c), start_c.max(end_c));
        self.rope.insert(start_c.min(end_c), new_text);
        self.rebuild_line_starts();

        let new_u16_len = new_text.encode_utf16().count();
        self.marked_range_utf16 = if new_u16_len > 0 {
            Some(start_u16..(start_u16 + new_u16_len))
        } else {
            None
        };
        let sel_end_u16 = match new_selected_range {
            Some(ref rel) => start_u16 + rel.end,
            None => start_u16 + new_u16_len,
        };
        let sel_start_u16 = match new_selected_range {
            Some(ref rel) => start_u16 + rel.start,
            None => sel_end_u16,
        };
        self.cursors = vec![CursorPosition {
            anchor: self.rope.utf16_to_char(sel_start_u16),
            head: self.rope.utf16_to_char(sel_end_u16),
        }];
        self.after_text_mutation(false, cx);
        window.notify_text_selection_changed(&self.focus_handle);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let c = self.rope.utf16_to_char(range_utf16.start);
        let (line, col) = self.char_to_line_col(c);
        let x_off = self
            .last_shaped_rows
            .iter()
            .find(|r| r.line_idx == line)
            .map(|row| {
                let b = char_to_byte_idx(&row.text, col);
                f32::from(row.shaped.x_for_index(b))
            })
            .unwrap_or_else(|| col as f32 * self.char_advance_px);
        let x = element_bounds.origin.x
            + px(self.gutter_width_px + CODE_LEFT_PAD_PX + x_off - self.scroll_x);
        let y = element_bounds.origin.y
            + px(TOP_PAD_PX + line as f32 * self.line_height_px - self.scroll_y);
        Some(Bounds::new(
            point(x, y),
            size(px(self.char_advance_px.max(2.0)), px(self.line_height_px)),
        ))
    }

    fn character_index_for_point(
        &mut self,
        pos: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let c = self.char_index_for_local_point(pos);
        Some(self.rope.char_to_utf16(c))
    }

    fn text_length_utf16(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.rope.len_utf16())
    }

    fn accepts_text_input(&self, _window: &mut Window, _cx: &mut Context<Self>) -> bool {
        !self.vim.enabled || self.vim.mode == VimMode::Insert
    }
}

struct SqlEditorSurfaceElement {
    doc: Entity<SqlEditorDocEntity>,
}

impl IntoElement for SqlEditorSurfaceElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SqlEditorSurfaceElement {
    type RequestLayoutState = Style;
    type PrepaintState = Option<EditorPrepaintState>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        let layout_id = window.request_layout(style.clone(), [], cx);
        (layout_id, style)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Style,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        Some(prepaint_editor_surface(&self.doc, bounds, window, cx))
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _style: &mut Style,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(state) = prepaint.take() {
            paint_editor_surface(&self.doc, bounds, state, window, cx);
        }
    }
}

fn prepaint_editor_surface(
    doc_entity: &Entity<SqlEditorDocEntity>,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> EditorPrepaintState {
    doc_entity.update(cx, |doc, _cx| {
        doc.last_bounds = bounds;
        let line_count = doc.line_starts.len().max(1);
        let digits = line_count.to_string().len().max(3);
        let font_size = px(doc.font_size_px);
        doc.gutter_width_px = (digits as f32 * 8.4 + GUTTER_LEFT_PAD_PX + GUTTER_RIGHT_PAD_PX)
            .max(MIN_GUTTER_WIDTH_PX);

        let view_h = f32::from(bounds.size.height).max(doc.line_height_px);
        let max_scroll_y =
            (TOP_PAD_PX + line_count as f32 * doc.line_height_px + BOTTOM_PAD_PX - view_h).max(0.0);
        doc.scroll_y = doc.scroll_y.clamp(0.0, max_scroll_y);

        let first_line =
            ((doc.scroll_y - TOP_PAD_PX).max(0.0) / doc.line_height_px).floor() as usize;
        let visible_lines = (view_h / doc.line_height_px).ceil() as usize + 2;
        let last_line = (first_line + visible_lines).min(line_count);
        let rows = shape_visible_editor_rows(doc, first_line..last_line, font_size, window);

        let (selection_quads, caret_quads, bracket_quads, active_line_band, marked_underline) =
            compute_editor_geometry(doc, bounds, &rows);
        let empty_hint = build_empty_editor_hint(doc, bounds, font_size, window);

        doc.last_shaped_rows = rows.clone();
        EditorPrepaintState {
            first_line,
            gutter_width_px: doc.gutter_width_px,
            line_height_px: doc.line_height_px,
            rows,
            selection_quads,
            caret_quads,
            bracket_quads,
            active_line_band,
            empty_hint,
            marked_underline,
        }
    })
}

fn shape_visible_editor_rows(
    doc: &mut SqlEditorDocEntity,
    line_range: Range<usize>,
    font_size: Pixels,
    window: &Window,
) -> Vec<ShapedEditorRow> {
    let gutter_font = tabular_gutter_font(&doc.font);
    let primary_line = doc.char_to_line_col(doc.primary_cursor().head).0;
    let mut rows = Vec::with_capacity(line_range.len());

    for line_idx in line_range {
        let text = doc.rope.line(line_idx).unwrap_or_default();
        let highlights =
            highlight_sql_line(&mut doc.highlighter, &doc.rope, line_idx, &doc.palette);
        let runs = build_line_text_runs(&text, &doc.font, &highlights, doc.palette.editor_text);
        let shaped = window.text_system().shape_line(
            SharedString::from(text.clone()),
            font_size,
            &runs,
            None,
        );
        let num_text = format!("{}", line_idx + 1);
        let is_primary = line_idx == primary_line;
        let mut g_font = gutter_font.clone();
        if is_primary {
            g_font.weight = FontWeight::SEMIBOLD;
        }
        let gutter_run = TextRun {
            len: num_text.len(),
            font: g_font,
            color: if is_primary {
                doc.palette.editor_text
            } else {
                doc.palette.editor_gutter_text
            },
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let gutter_shaped = window.text_system().shape_line(
            SharedString::from(num_text),
            font_size,
            &[gutter_run],
            None,
        );
        rows.push(ShapedEditorRow {
            line_idx,
            text,
            shaped,
            gutter_shaped,
        });
    }
    rows
}

fn build_empty_editor_hint(
    doc: &SqlEditorDocEntity,
    bounds: Bounds<Pixels>,
    font_size: Pixels,
    window: &Window,
) -> Option<(ShapedLine, Point<Pixels>)> {
    (doc.rope.len_chars() == 0 && doc.marked_range_utf16.is_none()).then(|| {
        let hint_text = doc.empty_hint_text;
        let hint_color = with_alpha(doc.palette.editor_text, 0.36);
        let run = TextRun {
            len: hint_text.len(),
            font: doc.font.clone(),
            color: hint_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped =
            window
                .text_system()
                .shape_line(SharedString::from(hint_text), font_size, &[run], None);
        let origin = point(
            bounds.origin.x + px(doc.gutter_width_px + CODE_LEFT_PAD_PX - doc.scroll_x),
            bounds.origin.y + px(TOP_PAD_PX - doc.scroll_y),
        );
        (shaped, origin)
    })
}

type HighlightQuad = (Bounds<Pixels>, Hsla);
type HighlightQuads = Vec<HighlightQuad>;
type UnderlineQuad = (Point<Pixels>, Pixels, Hsla);
type CursorAndSelectionQuads = (HighlightQuads, HighlightQuads);
type EditorRenderQuads = (
    HighlightQuads,
    HighlightQuads,
    HighlightQuads,
    Option<Bounds<Pixels>>,
    Option<UnderlineQuad>,
);

fn compute_editor_geometry(
    doc: &SqlEditorDocEntity,
    bounds: Bounds<Pixels>,
    rows: &[ShapedEditorRow],
) -> EditorRenderQuads {
    let code_origin_x =
        f32::from(bounds.origin.x) + doc.gutter_width_px + CODE_LEFT_PAD_PX - doc.scroll_x;
    let code_origin_y = f32::from(bounds.origin.y) + TOP_PAD_PX - doc.scroll_y;
    let lh = doc.line_height_px;
    let primary_line = doc.char_to_line_col(doc.primary_cursor().head).0;
    let active_band = rows.iter().any(|r| r.line_idx == primary_line).then(|| {
        Bounds::new(
            point(
                bounds.origin.x + px(doc.gutter_width_px),
                px(code_origin_y + primary_line as f32 * lh),
            ),
            size(bounds.size.width - px(doc.gutter_width_px), px(lh)),
        )
    });

    let (selection_quads, caret_quads) =
        compute_cursor_and_selection_quads(doc, rows, code_origin_x, code_origin_y, lh);
    let bracket_quads = compute_bracket_quads(doc, rows, code_origin_x, code_origin_y, lh);
    let marked_underline = compute_marked_underline(doc, rows, code_origin_x, code_origin_y, lh);

    (
        selection_quads,
        caret_quads,
        bracket_quads,
        active_band,
        marked_underline,
    )
}

fn compute_cursor_and_selection_quads(
    doc: &SqlEditorDocEntity,
    rows: &[ShapedEditorRow],
    code_origin_x: f32,
    code_origin_y: f32,
    lh: f32,
) -> CursorAndSelectionQuads {
    let mut selection_quads = Vec::new();
    let mut caret_quads = Vec::new();
    for cursor in &doc.cursors {
        let (s, e) = cursor.range();
        if e > s {
            let (sl, sc) = doc.char_to_line_col(s);
            let (el, ec) = doc.char_to_line_col(e);
            for row in rows {
                if row.line_idx < sl || row.line_idx > el {
                    continue;
                }
                let start_col = if row.line_idx == sl { sc } else { 0 };
                let x1 = x_for_char_col(row, start_col, doc.char_advance_px);
                let x2 = if row.line_idx == el {
                    x_for_char_col(row, ec, doc.char_advance_px)
                } else {
                    x_for_char_col(row, row.text.chars().count(), doc.char_advance_px)
                        + doc.char_advance_px * 0.65
                };
                selection_quads.push((
                    Bounds::new(
                        point(
                            px(code_origin_x + x1),
                            px(code_origin_y + row.line_idx as f32 * lh),
                        ),
                        size(px((x2 - x1).max(3.0)), px(lh)),
                    ),
                    doc.palette.editor_selection,
                ));
            }
        }
        let (hl, hc) = doc.char_to_line_col(cursor.head);
        if let Some(row) = rows.iter().find(|r| r.line_idx == hl) {
            let x = code_origin_x + x_for_char_col(row, hc, doc.char_advance_px);
            let y = code_origin_y + hl as f32 * lh;
            let is_block = doc.vim.enabled && doc.vim.mode != VimMode::Insert;
            let (w, color) = if is_block {
                (
                    doc.char_advance_px.max(7.0),
                    with_alpha(doc.palette.editor_caret, 0.42),
                )
            } else {
                (1.75, doc.palette.editor_caret)
            };
            caret_quads.push((
                Bounds::new(point(px(x), px(y + 1.0)), size(px(w), px(lh - 2.0))),
                color,
            ));
        }
    }
    (selection_quads, caret_quads)
}

fn compute_bracket_quads(
    doc: &SqlEditorDocEntity,
    rows: &[ShapedEditorRow],
    code_origin_x: f32,
    code_origin_y: f32,
    lh: f32,
) -> Vec<(Bounds<Pixels>, Hsla)> {
    let mut bracket_quads = Vec::new();
    let Some((b1, b2)) = matching_bracket_pair(&doc.rope, doc.primary_cursor().head) else {
        return bracket_quads;
    };
    let match_color = with_alpha(doc.palette.editor_syntax_keyword, 0.24);
    for b_idx in [b1, b2] {
        let (bl, bc) = doc.char_to_line_col(b_idx);
        if let Some(row) = rows.iter().find(|r| r.line_idx == bl) {
            let x1 = x_for_char_col(row, bc, doc.char_advance_px);
            let x2 = x_for_char_col(row, bc + 1, doc.char_advance_px);
            bracket_quads.push((
                Bounds::new(
                    point(
                        px(code_origin_x + x1),
                        px(code_origin_y + bl as f32 * lh + 1.0),
                    ),
                    size(px((x2 - x1).max(doc.char_advance_px)), px(lh - 2.0)),
                ),
                match_color,
            ));
        }
    }
    bracket_quads
}

fn compute_marked_underline(
    doc: &SqlEditorDocEntity,
    rows: &[ShapedEditorRow],
    code_origin_x: f32,
    code_origin_y: f32,
    lh: f32,
) -> Option<(Point<Pixels>, Pixels, Hsla)> {
    let mr = doc.marked_range_utf16.as_ref()?;
    let s_c = doc.rope.utf16_to_char(mr.start);
    let e_c = doc.rope.utf16_to_char(mr.end);
    let (sl, sc) = doc.char_to_line_col(s_c);
    let (el, ec) = doc.char_to_line_col(e_c);
    if sl != el {
        return None;
    }
    let row = rows.iter().find(|r| r.line_idx == sl)?;
    let x1 = x_for_char_col(row, sc, doc.char_advance_px);
    let x2 = x_for_char_col(row, ec, doc.char_advance_px);
    Some((
        point(
            px(code_origin_x + x1),
            px(code_origin_y + (sl as f32 + 1.0) * lh - 2.0),
        ),
        px((x2 - x1).max(4.0)),
        doc.palette.editor_caret,
    ))
}

fn paint_editor_surface(
    doc_entity: &Entity<SqlEditorDocEntity>,
    bounds: Bounds<Pixels>,
    state: EditorPrepaintState,
    window: &mut Window,
    cx: &mut App,
) {
    let doc = doc_entity.read(cx);
    let palette = doc.palette.clone();
    let focus_handle = doc.focus_handle.clone();
    let scroll_x = doc.scroll_x;
    let scroll_y = doc.scroll_y;
    let is_focused = focus_handle.is_focused(window);
    let border_color = cx.colors().border;
    let _ = doc;

    let gutter_bounds = Bounds::new(
        bounds.origin,
        size(px(state.gutter_width_px), bounds.size.height),
    );
    let code_bounds = Bounds::new(
        point(bounds.origin.x + px(state.gutter_width_px), bounds.origin.y),
        size(
            bounds.size.width - px(state.gutter_width_px),
            bounds.size.height,
        ),
    );
    window.paint_quad(fill(code_bounds, palette.editor_background));
    window.paint_quad(fill(gutter_bounds, palette.editor_gutter_background));
    window.paint_quad(fill(
        Bounds::new(
            point(
                bounds.origin.x + px(state.gutter_width_px - 1.0),
                bounds.origin.y,
            ),
            size(px(1.0), bounds.size.height),
        ),
        border_color,
    ));

    window.with_content_mask(
        Some(ContentMask {
            bounds: code_bounds,
        }),
        |window| {
            paint_editor_code_layer(
                bounds, &state, &palette, scroll_x, scroll_y, is_focused, window, cx,
            );
        },
    );
    window.with_content_mask(
        Some(ContentMask {
            bounds: gutter_bounds,
        }),
        |window| {
            paint_editor_gutter_layer(bounds, &state, scroll_y, window, cx);
        },
    );

    window.handle_input(
        &focus_handle,
        ElementInputHandler::new(bounds, doc_entity.clone()),
        cx,
    );
    let _ = state.first_line;
}

#[expect(
    clippy::too_many_arguments,
    reason = "Code layer paint pass consumes prepainted quads, rows, and scroll metrics"
)]
fn paint_editor_code_layer(
    bounds: Bounds<Pixels>,
    state: &EditorPrepaintState,
    palette: &ContentThemePalette,
    scroll_x: f32,
    scroll_y: f32,
    is_focused: bool,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(band) = state.active_line_band {
        window.paint_quad(fill(band, palette.editor_current_line));
    }
    for (q_bounds, color) in &state.bracket_quads {
        window.paint_quad(fill(*q_bounds, *color));
        window.paint_quad(outline(
            *q_bounds,
            palette.editor_syntax_keyword,
            BorderStyle::Solid,
        ));
    }
    for (s_bounds, color) in &state.selection_quads {
        window.paint_quad(fill(*s_bounds, *color));
    }
    for row in &state.rows {
        let origin = point(
            bounds.origin.x + px(state.gutter_width_px + CODE_LEFT_PAD_PX - scroll_x),
            bounds.origin.y
                + px(TOP_PAD_PX + row.line_idx as f32 * state.line_height_px - scroll_y),
        );
        let _ = row.shaped.paint(
            origin,
            px(state.line_height_px),
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }
    if let Some((ref hint_line, hint_origin)) = state.empty_hint {
        let _ = hint_line.paint(
            hint_origin,
            px(state.line_height_px),
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }
    if let Some((ul_origin, ul_width, ul_color)) = state.marked_underline {
        window.paint_quad(fill(
            Bounds::new(ul_origin, size(ul_width, px(1.5))),
            ul_color,
        ));
    }
    if is_focused {
        for (c_bounds, color) in &state.caret_quads {
            window.paint_quad(fill(*c_bounds, *color));
        }
    }
}

fn paint_editor_gutter_layer(
    bounds: Bounds<Pixels>,
    state: &EditorPrepaintState,
    scroll_y: f32,
    window: &mut Window,
    cx: &mut App,
) {
    for row in &state.rows {
        let num_w = f32::from(row.gutter_shaped.width);
        let gx =
            bounds.origin.x + px((state.gutter_width_px - GUTTER_RIGHT_PAD_PX - num_w).max(4.0));
        let gy = bounds.origin.y
            + px(TOP_PAD_PX + row.line_idx as f32 * state.line_height_px - scroll_y);
        let _ = row.gutter_shaped.paint(
            point(gx, gy),
            px(state.line_height_px),
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }
}

/// Renders the SQL editor surface with syntax highlighting, autocomplete, saved/history drawer,
/// and vertical split pairing with [`results_pane::render_results_pane_embedded`].
#[must_use]
pub fn render_sql_editor(
    ws: &mut TableMinusWorkspace,
    window: &mut Window,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    sync_doc_with_workspace(ws, cx);
    let editor_pane = render_editor_panel_body(ws, cx);
    let results = results_pane::render_results_pane_embedded(ws, window, cx);
    ResizablePanelGroup::new("sql-editor-results-split")
        .orientation(HeroOrientation::Vertical)
        .panel(
            ResizablePanel::new()
                .default_size(64.0)
                .min_size_px(px(140.0))
                .child(editor_pane),
        )
        .panel(
            ResizablePanel::new()
                .default_size(36.0)
                .min_size_px(px(110.0))
                .child(results),
        )
        .into_any_element()
}

fn render_editor_panel_body(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let doc_entity = ws.sql_editor_ui.doc.clone();
    let doc = doc_entity.read(cx);
    let focus_handle = doc.focus_handle.clone();
    let palette = doc.palette.clone();
    let primary = doc.primary_cursor();
    let primary_pos = doc.char_to_line_col(primary.head);
    let cursor_count = doc.cursors.len();
    let ac_open = doc.autocomplete.is_open;
    let grammar = doc.grammar;
    let vim_enabled = doc.vim.enabled;
    let vim_mode = doc.vim.mode;
    let vim_cmd_buf = doc.vim.command_buffer.clone();
    let _ = doc;

    let toolbar = render_editor_toolbar(
        ws,
        primary_pos,
        cursor_count,
        grammar,
        vim_enabled,
        vim_mode,
        &vim_cmd_buf,
        cx,
    );

    let editor_surface =
        build_interactive_editor_canvas(&doc_entity, &focus_handle, &palette, ac_open, cx);
    let main_row = if ws.sql_editor_ui.show_drawer {
        let drawer = render_editor_drawer(ws, cx);
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_row()
            .child(editor_surface)
            .child(drawer)
    } else {
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_row()
            .child(editor_surface)
    };

    let find_replace_bar = render_find_replace_bar(ws, cx);

    let root = div()
        .id("workspace-sql-editor")
        .flex_1()
        .min_h_0()
        .size_full()
        .flex()
        .flex_col()
        .child(toolbar)
        .children(find_replace_bar)
        .child(main_row);
    attach_editor_actions(root, cx).into_any_element()
}

fn attach_editor_actions(
    root: gpui::Stateful<gpui::Div>,
    cx: &mut Context<TableMinusWorkspace>,
) -> gpui::Stateful<gpui::Div> {
    root.on_action(cx.listener(|ws, _: &actions::RunQuery, _w, cx| {
        trigger_run_query(ws, false, cx);
    }))
    .on_action(cx.listener(|ws, _: &actions::RunSelectedQuery, _w, cx| {
        trigger_run_query(ws, true, cx);
    }))
    .on_action(cx.listener(|ws, _: &actions::CancelQuery, _w, cx| {
        cancel_active_query(ws, cx);
    }))
    .on_action(cx.listener(|ws, _: &actions::SaveQuery, _w, cx| {
        ws.sql_editor_ui.save_popover_open = !ws.sql_editor_ui.save_popover_open;
        cx.notify();
    }))
    .on_action(cx.listener(|ws, _: &actions::EscapeDismiss, w, cx| {
        if ws.sql_editor_ui.doc.read(cx).autocomplete.is_open {
            ws.sql_editor_ui.doc.update(cx, |doc, cx| {
                doc.autocomplete.dismiss();
                doc.suppress_autocomplete = true;
                cx.notify();
            });
        } else if ws.sql_editor_ui.find_replace_open {
            ws.sql_editor_ui.find_replace_open = false;
            let focus = ws.sql_editor_ui.doc.read(cx).focus_handle.clone();
            w.focus(&focus, cx);
            cx.notify();
        } else if ws.sql_editor_ui.save_popover_open {
            ws.sql_editor_ui.save_popover_open = false;
            cx.notify();
        } else {
            ws.dismiss_overlays(cx);
        }
    }))
}

fn build_interactive_editor_canvas(
    doc_entity: &Entity<SqlEditorDocEntity>,
    focus_handle: &FocusHandle,
    palette: &ContentThemePalette,
    ac_open: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let key_doc = doc_entity.clone();
    let md_doc = doc_entity.clone();
    let mm_doc = doc_entity.clone();
    let mu_doc = doc_entity.clone();
    let sw_doc = doc_entity.clone();

    let mut container = div()
        .id("sql-editor-surface")
        .relative()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .h_full()
        .cursor_text()
        .track_focus(focus_handle)
        .on_key_down(move |event, window, cx| {
            key_doc.update(cx, |doc, cx| doc.handle_key_down(event, window, cx));
        })
        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
            md_doc.update(cx, |doc, cx| doc.handle_mouse_down(event, window, cx));
        })
        .on_mouse_move(move |event, _window, cx| {
            mm_doc.update(cx, |doc, cx| doc.handle_mouse_move(event, cx));
        })
        .on_mouse_up(MouseButton::Left, move |event, _window, cx| {
            mu_doc.update(cx, |doc, cx| doc.handle_mouse_up(event, cx));
        })
        .on_scroll(move |event, _window, cx| {
            sw_doc.update(cx, |doc, cx| doc.handle_scroll_wheel(event, cx));
        })
        .child(SqlEditorSurfaceElement {
            doc: doc_entity.clone(),
        });

    if ac_open {
        container = container.child(render_autocomplete_overlay(doc_entity, palette, cx));
    }
    container.into_any_element()
}

#[expect(
    clippy::too_many_arguments,
    reason = "Editor toolbar reflects complete live doc/Vim/query state"
)]
fn render_editor_toolbar(
    ws: &mut TableMinusWorkspace,
    primary_pos: (usize, usize),
    cursor_count: usize,
    grammar: SqlGrammar,
    vim_enabled: bool,
    vim_mode: VimMode,
    vim_cmd_buf: &str,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let left = render_toolbar_left_actions(ws, vim_enabled, vim_mode, vim_cmd_buf, cx);
    let right = render_toolbar_right_status(ws, primary_pos, cursor_count, grammar, cx);

    div()
        .id("sql-editor-toolbar")
        .w_full()
        .h(px(34.0))
        .flex_shrink_0()
        .px(px(10.0))
        .flex()
        .items_center()
        .justify_between()
        .bg(theme::muted_surface_bg(cx))
        .border_b_1()
        .border_color(theme::hairline_border_color(cx))
        .child(left)
        .child(right)
        .into_any_element()
}

fn render_toolbar_left_actions(
    ws: &mut TableMinusWorkspace,
    vim_enabled: bool,
    vim_mode: VimMode,
    vim_cmd_buf: &str,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let save_popover = render_save_query_popover(ws, cx);
    let s = &ws.controller.strings;
    let find_btn = render_toolbar_find_button(ws.sql_editor_ui.find_replace_open, s, cx);
    let format_btn = render_toolbar_format_button(s, cx);
    let explain_btn = render_toolbar_explain_button(false, s, cx);
    let explain_analyze_btn = render_toolbar_explain_button(true, s, cx);
    let drawer_btn = render_toolbar_drawer_button(ws, cx);
    let vim_btn = render_toolbar_vim_button(vim_enabled, s, cx);

    let mut left = div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(save_popover)
        .child(find_btn)
        .child(format_btn)
        .child(explain_btn)
        .child(explain_analyze_btn)
        .child(drawer_btn)
        .child(vim_btn);

    if vim_enabled {
        let (mode_label, color) = match vim_mode {
            VimMode::Normal => ("NORMAL".to_owned(), HeroColor::Accent),
            VimMode::Insert => ("INSERT".to_owned(), HeroColor::Success),
            VimMode::Visual => ("VISUAL".to_owned(), HeroColor::Warning),
            VimMode::Command => (format!(":{vim_cmd_buf}"), HeroColor::Default),
        };
        left = left.child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .size(HeroSize::Sm)
                .color(color)
                .child(ChipLabel::new().child(mode_label)),
        );
    }
    left.into_any_element()
}

fn render_toolbar_find_button(
    is_open: bool,
    s: &crate::i18n::Strings,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let btn = Button::new("sql-find-btn")
        .variant(if is_open {
            Variant::Secondary
        } else {
            Variant::Ghost
        })
        .size(HeroSize::Sm)
        .child(Icon::new(IconName::Search).size(IconSize::Xs))
        .on_press(cx.listener(|ws, _e, w, cx| {
            ws.sql_editor_ui.find_replace_open = !ws.sql_editor_ui.find_replace_open;
            let focus = if ws.sql_editor_ui.find_replace_open {
                ws.sql_editor_ui.find_input.read(cx).focus_handle(cx)
            } else {
                ws.sql_editor_ui.doc.read(cx).focus_handle.clone()
            };
            w.focus(&focus, cx);
            cx.notify();
        }));
    Tooltip::new(format!("{} (Ctrl+F)", s.editor_find))
        .child(btn)
        .into_any_element()
}

fn render_toolbar_format_button(
    s: &crate::i18n::Strings,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    Button::new("sql-format-btn")
        .variant(Variant::Ghost)
        .size(HeroSize::Sm)
        .child(Icon::new(IconName::WandSparkles).size(IconSize::Xs))
        .label(s.editor_format_sql)
        .on_press(cx.listener(|ws, _e, _w, cx| {
            trigger_format_sql(ws, cx);
        }))
        .into_any_element()
}

fn render_toolbar_explain_button(
    analyze: bool,
    s: &crate::i18n::Strings,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let (id, label) = if analyze {
        ("sql-explain-analyze-btn", s.explain_analyze)
    } else {
        ("sql-explain-btn", s.explain_plan)
    };
    let mut btn = Button::new(id).variant(Variant::Ghost).size(HeroSize::Sm);
    if !analyze {
        btn = btn.child(Icon::new(IconName::Activity).size(IconSize::Xs));
    }
    btn.label(label)
        .on_press(cx.listener(move |ws, _e, _w, cx| {
            trigger_explain_query(ws, analyze, cx);
        }))
        .into_any_element()
}

fn render_toolbar_drawer_button(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let drawer_count = ws.controller.saved_queries.len() + ws.controller.query_history.len();
    let drawer_label = if drawer_count > 0 {
        format!("{} ({drawer_count})", ws.controller.strings.library_label)
    } else {
        ws.controller.strings.library_label.to_owned()
    };
    Button::new("sql-drawer-btn")
        .variant(if ws.sql_editor_ui.show_drawer {
            Variant::Secondary
        } else {
            Variant::Ghost
        })
        .size(HeroSize::Sm)
        .child(Icon::new(IconName::History).size(IconSize::Xs))
        .label(drawer_label)
        .on_press(cx.listener(|ws, _e, _w, cx| {
            ws.sql_editor_ui.show_drawer = !ws.sql_editor_ui.show_drawer;
            cx.notify();
        }))
        .into_any_element()
}

fn render_toolbar_vim_button(
    vim_enabled: bool,
    s: &crate::i18n::Strings,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let vim_label = if vim_enabled {
        s.vim_on_label
    } else {
        s.vim_off_label
    };
    Button::new("sql-vim-toggle-btn")
        .variant(if vim_enabled {
            Variant::Tertiary
        } else {
            Variant::Ghost
        })
        .size(HeroSize::Sm)
        .child(Icon::new(IconName::Terminal).size(IconSize::Xs))
        .label(vim_label)
        .on_press(cx.listener(|ws, _e, _w, cx| {
            ws.controller.toggle_vim_mode();
            let enabled = ws.controller.vim_mode.is_some();
            ws.sql_editor_ui.doc.update(cx, |doc, cx| {
                doc.vim = VimState::new(enabled);
                cx.notify();
            });
            cx.notify();
        }))
        .into_any_element()
}

fn render_find_replace_bar(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> Option<AnyElement> {
    if !ws.sql_editor_ui.find_replace_open {
        return None;
    }
    let doc_text = ws.sql_editor_ui.current_text(cx);
    refresh_find_matches_state(ws, &doc_text, cx);

    let s = &ws.controller.strings;
    let find_input = ws.sql_editor_ui.find_input.clone();
    let replace_input = ws.sql_editor_ui.replace_input.clone();
    let count_label = if ws.sql_editor_ui.match_count > 0 {
        format!(
            "{} / {}",
            ws.sql_editor_ui.active_match_idx + 1,
            ws.sql_editor_ui.match_count
        )
    } else {
        "0".to_owned()
    };

    let find_group = render_find_input_group(ws, &find_input, &count_label, s, cx);
    let replace_group = render_replace_input_group(&replace_input, s, cx);

    Some(
        div()
            .id("sql-find-replace-bar")
            .w_full()
            .px(px(12.0))
            .py(px(6.0))
            .flex_shrink_0()
            .border_b_1()
            .border_color(theme::hairline_border_color(cx))
            .bg(theme::panel_surface_bg(cx))
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .child(find_group)
            .child(replace_group)
            .into_any_element(),
    )
}

fn render_find_input_group(
    ws: &TableMinusWorkspace,
    find_input: &Entity<InputState>,
    count_label: &str,
    s: &crate::i18n::Strings,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let has_matches = ws.sql_editor_ui.match_count > 0;
    let case_on = ws.sql_editor_ui.case_sensitive;
    let word_on = ws.sql_editor_ui.whole_word;
    div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(
            div().w(px(210.0)).child(
                Input::new(find_input)
                    .placeholder(s.editor_find)
                    .height(px(26.0))
                    .text_size(px(12.0))
                    .variant(FieldVariant::Secondary)
                    .on_submit(cx.listener(|ws, _val: &str, _w, cx| {
                        step_find_match(ws, true, cx);
                    })),
            ),
        )
        .child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .size(HeroSize::Sm)
                .color(if has_matches {
                    HeroColor::Accent
                } else {
                    HeroColor::Default
                })
                .child(ChipLabel::new().child(count_label.to_owned())),
        )
        .child(
            Button::new("sql-find-prev-btn")
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .is_disabled(!has_matches)
                .child(Icon::new(IconName::ChevronUp).size(IconSize::Xs))
                .on_press(cx.listener(|ws, _e, _w, cx| step_find_match(ws, false, cx))),
        )
        .child(
            Button::new("sql-find-next-btn")
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .is_disabled(!has_matches)
                .child(Icon::new(IconName::ChevronDown).size(IconSize::Xs))
                .on_press(cx.listener(|ws, _e, _w, cx| step_find_match(ws, true, cx))),
        )
        .child(
            Button::new("sql-find-case-btn")
                .variant(if case_on {
                    Variant::Secondary
                } else {
                    Variant::Ghost
                })
                .size(HeroSize::Sm)
                .label("Aa")
                .on_press(cx.listener(|ws, _e, _w, cx| {
                    ws.sql_editor_ui.case_sensitive = !ws.sql_editor_ui.case_sensitive;
                    cx.notify();
                })),
        )
        .child(
            Button::new("sql-find-word-btn")
                .variant(if word_on {
                    Variant::Secondary
                } else {
                    Variant::Ghost
                })
                .size(HeroSize::Sm)
                .label("W")
                .on_press(cx.listener(|ws, _e, _w, cx| {
                    ws.sql_editor_ui.whole_word = !ws.sql_editor_ui.whole_word;
                    cx.notify();
                })),
        )
        .into_any_element()
}

fn render_replace_input_group(
    replace_input: &Entity<InputState>,
    s: &crate::i18n::Strings,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(
            div().w(px(190.0)).child(
                Input::new(replace_input)
                    .placeholder(s.editor_replace)
                    .height(px(26.0))
                    .text_size(px(12.0))
                    .variant(FieldVariant::Secondary)
                    .on_submit(cx.listener(|ws, _val: &str, _w, cx| {
                        apply_find_replace(ws, false, cx);
                    })),
            ),
        )
        .child(
            Button::new("sql-replace-one-btn")
                .variant(Variant::Tertiary)
                .size(HeroSize::Sm)
                .label(s.editor_replace)
                .on_press(cx.listener(|ws, _e, _w, cx| apply_find_replace(ws, false, cx))),
        )
        .child(
            Button::new("sql-replace-all-btn")
                .variant(Variant::Tertiary)
                .size(HeroSize::Sm)
                .label(s.editor_replace_all)
                .on_press(cx.listener(|ws, _e, _w, cx| apply_find_replace(ws, true, cx))),
        )
        .child(
            Button::new("sql-find-close-btn")
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .child(Icon::new(IconName::X).size(IconSize::Xs))
                .on_press(cx.listener(|ws, _e, w, cx| {
                    ws.sql_editor_ui.find_replace_open = false;
                    let focus = ws.sql_editor_ui.doc.read(cx).focus_handle.clone();
                    w.focus(&focus, cx);
                    cx.notify();
                })),
        )
        .into_any_element()
}

fn open_find_replace_with_text(
    ws: &mut TableMinusWorkspace,
    window: &mut Window,
    doc_text: &str,
    selected: Option<String>,
    cx: &mut Context<TableMinusWorkspace>,
) {
    ws.sql_editor_ui.find_replace_open = true;
    if let Some(sel) = selected
        && !sel.contains('\n')
        && sel.len() <= 200
    {
        ws.sql_editor_ui.find_input.update(cx, |state, cx| {
            state.set_value(sel);
            cx.notify();
        });
    }
    refresh_find_matches_state(ws, doc_text, cx);
    let focus = ws.sql_editor_ui.find_input.read(cx).focus_handle(cx);
    window.focus(&focus, cx);
    cx.notify();
}

fn refresh_find_matches_state(ws: &mut TableMinusWorkspace, doc_text: &str, cx: &App) {
    let query = ws.sql_editor_ui.find_input.read(cx).value().to_string();
    let matches = if query.is_empty() {
        Vec::new()
    } else {
        find_matches(
            doc_text,
            &query,
            FindReplaceOptions {
                case_sensitive: ws.sql_editor_ui.case_sensitive,
                whole_word: ws.sql_editor_ui.whole_word,
            },
        )
    };
    ws.sql_editor_ui.match_count = matches.len();
    if matches.is_empty() {
        ws.sql_editor_ui.active_match_idx = 0;
    } else if ws.sql_editor_ui.active_match_idx >= matches.len() {
        ws.sql_editor_ui.active_match_idx = matches.len() - 1;
    }
    ws.sql_editor_ui.find_matches_positions = matches;
}

fn step_find_match(
    ws: &mut TableMinusWorkspace,
    forward: bool,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let doc_text = ws.sql_editor_ui.current_text(cx);
    refresh_find_matches_state(ws, &doc_text, cx);
    let count = ws.sql_editor_ui.find_matches_positions.len();
    if count == 0 {
        return;
    }
    let idx = if forward {
        (ws.sql_editor_ui.active_match_idx + 1) % count
    } else if ws.sql_editor_ui.active_match_idx == 0 {
        count - 1
    } else {
        ws.sql_editor_ui.active_match_idx - 1
    };
    ws.sql_editor_ui.active_match_idx = idx;
    if let Some(range) = ws.sql_editor_ui.find_matches_positions.get(idx).cloned() {
        select_byte_range_in_doc(ws, &doc_text, range, cx);
    }
    cx.notify();
}

fn select_byte_range_in_doc(
    ws: &mut TableMinusWorkspace,
    doc_text: &str,
    range: Range<usize>,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let start_chars = doc_text[..range.start.min(doc_text.len())].chars().count();
    let end_chars = start_chars
        + doc_text[range.start.min(doc_text.len())..range.end.min(doc_text.len())]
            .chars()
            .count();
    ws.sql_editor_ui.doc.update(cx, |doc, cx| {
        doc.cursors = vec![CursorPosition {
            anchor: start_chars,
            head: end_chars,
        }];
        doc.ensure_primary_caret_visible();
        cx.notify();
    });
}

fn apply_find_replace(
    ws: &mut TableMinusWorkspace,
    replace_all: bool,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let find = ws.sql_editor_ui.find_input.read(cx).value().to_string();
    if find.is_empty() {
        return;
    }
    let replace = ws.sql_editor_ui.replace_input.read(cx).value().to_string();
    let current = ws.sql_editor_ui.current_text(cx);
    ws.controller.set_editor_sql(&current);
    let replaced = ws.controller.find_replace_in_editor(
        &find,
        &replace,
        ws.sql_editor_ui.case_sensitive,
        ws.sql_editor_ui.whole_word,
        replace_all,
    );
    if replaced > 0 {
        sync_doc_sql_from_controller(ws, cx);
        let label = if replace_all {
            ws.controller.strings.editor_replace_all
        } else {
            ws.controller.strings.editor_replace
        };
        ws.notify_toast(
            ToastSeverity::Info,
            format!("{label}: {replaced}"),
            None,
            cx,
        );
    }
    cx.notify();
}

fn trigger_format_sql(ws: &mut TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) {
    let current = ws.sql_editor_ui.current_text(cx);
    ws.controller.set_editor_sql(&current);
    ws.controller.format_editor_sql();
    sync_doc_sql_from_controller(ws, cx);
    let msg = ws.controller.strings.editor_format_sql;
    ws.notify_toast(ToastSeverity::Info, msg, None, cx);
    cx.notify();
}

fn trigger_explain_query(
    ws: &mut TableMinusWorkspace,
    analyze: bool,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let sql = {
        let doc = ws.sql_editor_ui.doc.read(cx);
        let (s, e) = doc.primary_cursor().range();
        if e > s {
            doc.rope.slice(s, e)
        } else {
            doc.rope.text()
        }
    };
    let label = if analyze {
        ws.controller.strings.explain_analyze
    } else {
        ws.controller.strings.explain_plan
    };
    let suffix = ws.controller.strings.explain_nodes_suffix;
    match ws.controller.run_visual_explain_blocking(&sql, analyze) {
        Ok(nodes) => {
            ws.explain_ui.last_error = None;
            ws.explain_ui.selected_node_id = nodes.first().map(|n| n.id.to_string());
            ws.controller.screen_mode = crate::app::ScreenMode::Workspace;
            ws.workspace_pane.close_tool();
            ws.workspace_pane.sub_view = super::WorkspaceSubView::SqlEditor;
            if let Some(tab_id) = ws.controller.tabs.active_tab() {
                ws.ensure_tab_result(tab_id).sub_view = super::ResultPaneSubView::Explain;
            }
            ws.notify_toast(
                ToastSeverity::Success,
                format!("{label}: {} {suffix}", nodes.len()),
                None,
                cx,
            );
            cx.notify();
        }
        Err(err) => {
            let msg = err.to_string();
            ws.explain_ui.last_error = Some(msg.clone());
            ws.notify_toast(ToastSeverity::Error, msg, None, cx);
            cx.notify();
        }
    }
}

fn sync_doc_sql_from_controller(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let updated = ws.controller.editor_sql_text();
    ws.sql_editor_ui.doc.update(cx, |doc, cx| {
        let prev_head = doc.primary_cursor().head;
        doc.rope = Rope::from_text(&updated);
        let clamped = prev_head.min(doc.rope.len_chars());
        doc.cursors = vec![CursorPosition::caret(clamped)];
        doc.rebuild_line_starts();
        doc.highlighter
            .invalidate_lines(0, doc.line_starts.len().saturating_sub(1));
        doc.last_controller_sql = updated.clone();
        doc.ensure_primary_caret_visible();
        cx.notify();
    });
    refresh_find_matches_state(ws, &updated, cx);
}

fn render_toolbar_right_status(
    ws: &TableMinusWorkspace,
    primary_pos: (usize, usize),
    cursor_count: usize,
    grammar: SqlGrammar,
    cx: &App,
) -> AnyElement {
    let tnum_font = tabular_gutter_font(&editor_gpui_font(&ws.controller.settings));
    let s = &ws.controller.strings;
    let line_str = (primary_pos.0 + 1).to_string();
    let col_str = (primary_pos.1 + 1).to_string();
    let pos_text = if cursor_count > 1 {
        s.cursor_pos_multi_tpl
            .replace("{line}", &line_str)
            .replace("{col}", &col_str)
            .replace("{count}", &cursor_count.to_string())
    } else {
        s.cursor_pos_tpl
            .replace("{line}", &line_str)
            .replace("{col}", &col_str)
    };
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(
            div()
                .font(tnum_font)
                .text_size(px(theme::TEXT_META_PX))
                .text_color(cx.colors().muted)
                .child(pos_text),
        )
        .child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .size(HeroSize::Sm)
                .color(HeroColor::Default)
                .child(ChipLabel::new().child(grammar_label(grammar))),
        )
        .into_any_element()
}

fn render_save_query_popover(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let trigger = Button::new("sql-save-trigger-btn")
        .variant(Variant::Ghost)
        .size(HeroSize::Sm)
        .child(Icon::new(IconName::Save).size(IconSize::Xs))
        .label(s.save);

    let name_input = ws.sql_editor_ui.save_name_input.clone();
    let submit_input = name_input.clone();
    let save_panel = div()
        .w(px(264.0))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            Input::new(&name_input)
                .placeholder(s.query_name_placeholder)
                .height(px(28.0))
                .text_size(px(12.0))
                .variant(FieldVariant::Secondary)
                .on_submit(cx.listener(move |ws, val: &str, _w, cx| {
                    commit_save_query(ws, val, cx);
                })),
        )
        .child(
            div()
                .flex()
                .justify_end()
                .gap(px(6.0))
                .child(
                    Button::new("sql-save-cancel-btn")
                        .variant(Variant::Ghost)
                        .size(HeroSize::Sm)
                        .label(s.cancel_label)
                        .on_press(cx.listener(|ws, _e, _w, cx| {
                            ws.sql_editor_ui.save_popover_open = false;
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("sql-save-confirm-btn")
                        .variant(Variant::Primary)
                        .size(HeroSize::Sm)
                        .label(s.save_query)
                        .on_press(cx.listener(move |ws, _e, _w, cx| {
                            let val = submit_input.read(cx).value().to_string();
                            commit_save_query(ws, &val, cx);
                        })),
                ),
        );

    Popover::new(trigger)
        .id("sql-save-popover")
        .title(s.save_query_title)
        .placement(PopoverPlacement::BottomStart)
        .is_open(ws.sql_editor_ui.save_popover_open)
        .on_open_change(cx.listener(|ws, open: &bool, _w, cx| {
            ws.sql_editor_ui.save_popover_open = *open;
            cx.notify();
        }))
        .child(save_panel)
        .into_any_element()
}

fn commit_save_query(
    ws: &mut TableMinusWorkspace,
    raw_name: &str,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let trimmed = raw_name.trim();
    if trimmed.is_empty() {
        let title = ws.controller.strings.enter_query_name_toast;
        let desc = ws.controller.strings.enter_query_name_desc;
        ws.notify_toast(ToastSeverity::Warning, title, Some(desc.into()), cx);
        return;
    }
    let sql = ws.sql_editor_ui.doc.read(cx).rope.text();
    ws.controller.set_editor_sql(&sql);
    ws.controller.save_current_query(trimmed, None);
    ws.sql_editor_ui.save_popover_open = false;
    ws.sql_editor_ui.show_drawer = true;
    ws.sql_editor_ui.drawer_tab = EditorDrawerTab::Saved;
    let saved_msg = ws
        .controller
        .strings
        .saved_query_named_tpl
        .replace("{name}", trimmed);
    ws.notify_toast(ToastSeverity::Success, saved_msg, None, cx);
    cx.notify();
}

fn render_editor_drawer(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let filter_input = ws.sql_editor_ui.drawer_search_input.clone();
    let needle = filter_input.read(cx).value().trim().to_ascii_lowercase();
    let active_tab = ws.sql_editor_ui.drawer_tab;
    let selected_key = match active_tab {
        EditorDrawerTab::Saved => "saved",
        EditorDrawerTab::History => "history",
    };

    let s = &ws.controller.strings;
    let tabs = Tabs::new(
        "sql-drawer-tabs",
        vec![
            TabItem::new("saved", s.saved_tab),
            TabItem::new("history", s.history_tab),
        ],
        "saved",
    )
    .variant(TabsVariant::Secondary)
    .size(TabsSize::Sm)
    .selected_key(selected_key)
    .on_selection_change(cx.listener(|ws, key: &SharedString, _w, cx| {
        ws.sql_editor_ui.drawer_tab = if key.as_ref() == "history" {
            EditorDrawerTab::History
        } else {
            EditorDrawerTab::Saved
        };
        cx.notify();
    }));

    let body = match active_tab {
        EditorDrawerTab::Saved => render_saved_queries_list(ws, &needle, cx),
        EditorDrawerTab::History => render_query_history_list(ws, &needle, cx),
    };

    div()
        .id("sql-editor-drawer")
        .w(px(DRAWER_WIDTH_PX))
        .h_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .bg(theme::panel_surface_bg(cx))
        .border_l_1()
        .border_color(theme::hairline_border_color(cx))
        .child(
            div()
                .p(px(8.0))
                .flex()
                .flex_col()
                .gap(px(6.0))
                .border_b_1()
                .border_color(theme::hairline_border_color(cx))
                .child(tabs)
                .child(
                    Input::new(&filter_input)
                        .placeholder(s.filter_saved_history_placeholder)
                        .height(px(28.0))
                        .text_size(px(12.0))
                        .variant(FieldVariant::Secondary)
                        .on_change(cx.listener(|_ws, _v, _w, cx| cx.notify())),
                ),
        )
        .child(div().flex_1().min_h_0().p(px(6.0)).child(body))
        .into_any_element()
}

fn render_saved_queries_list(
    ws: &TableMinusWorkspace,
    needle: &str,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let filtered: Vec<(String, String, Option<String>)> = ws
        .controller
        .saved_queries
        .iter()
        .filter(|q| {
            needle.is_empty()
                || q.name.to_ascii_lowercase().contains(needle)
                || q.sql.to_ascii_lowercase().contains(needle)
        })
        .map(|q| (q.name.clone(), q.sql.clone(), q.folder.clone()))
        .collect();

    if filtered.is_empty() {
        let s = &ws.controller.strings;
        return render_drawer_empty_state(
            IconName::Bookmark,
            if needle.is_empty() {
                s.no_saved_queries_title
            } else {
                s.no_matching_saved_queries
            },
            if needle.is_empty() {
                s.no_saved_queries_desc
            } else {
                s.try_clearing_search_desc
            },
            cx,
        );
    }

    let items: Vec<ListBoxItem> = filtered
        .iter()
        .enumerate()
        .map(|(i, (name, sql, folder))| {
            let preview = single_line_preview(sql, 44);
            let desc = match folder {
                Some(f) if !f.is_empty() => format!("{f} · {preview}"),
                _ => preview,
            };
            ListBoxItem::new(format!("saved:{i}"), name.clone())
                .description(desc)
                .icon(IconName::Bookmark)
        })
        .collect();
    let entity = cx.entity();
    ListBox::new("sql-saved-listbox", items)
        .selection_mode(SelectionMode::Single)
        .max_h(px(460.0))
        .on_action(move |key, window, cx| {
            if let Some(idx) = key
                .strip_prefix("saved:")
                .and_then(|s| s.parse::<usize>().ok())
                && let Some((_, sql, _)) = filtered.get(idx)
            {
                let sql = sql.clone();
                entity.update(cx, |ws, cx| {
                    load_sql_into_active_editor(ws, &sql, window, cx)
                });
            }
        })
        .into_any_element()
}

fn render_query_history_list(
    ws: &TableMinusWorkspace,
    needle: &str,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let filtered: Vec<(String, String, u64, bool)> = ws
        .controller
        .query_history
        .iter()
        .rev()
        .filter(|h| needle.is_empty() || h.sql.to_ascii_lowercase().contains(needle))
        .map(|h| (h.sql.clone(), h.database.clone(), h.elapsed_ms, h.succeeded))
        .collect();

    let s = &ws.controller.strings;
    if filtered.is_empty() {
        return render_drawer_empty_state(
            IconName::History,
            if needle.is_empty() {
                s.no_query_history_title
            } else {
                s.no_matching_history
            },
            if needle.is_empty() {
                s.no_query_history_desc
            } else {
                s.try_clearing_search_desc
            },
            cx,
        );
    }

    let items: Vec<ListBoxItem> = filtered
        .iter()
        .enumerate()
        .map(|(i, (sql, db, ms, ok))| {
            let label = single_line_preview(sql, 42);
            let status = if *ok { s.status_ok } else { s.status_error };
            let desc = if db.is_empty() {
                format!("{ms} ms · {status}")
            } else {
                format!("{db} · {ms} ms · {status}")
            };
            ListBoxItem::new(format!("hist:{i}"), label)
                .description(desc)
                .icon(if *ok {
                    IconName::CircleCheck
                } else {
                    IconName::CircleX
                })
        })
        .collect();
    let entity = cx.entity();
    ListBox::new("sql-history-listbox", items)
        .selection_mode(SelectionMode::Single)
        .max_h(px(460.0))
        .on_action(move |key, window, cx| {
            if let Some(idx) = key
                .strip_prefix("hist:")
                .and_then(|s| s.parse::<usize>().ok())
                && let Some((sql, _, _, _)) = filtered.get(idx)
            {
                let sql = sql.clone();
                entity.update(cx, |ws, cx| {
                    load_sql_into_active_editor(ws, &sql, window, cx)
                });
            }
        })
        .into_any_element()
}

fn render_drawer_empty_state(
    icon: IconName,
    title: &'static str,
    subtitle: &'static str,
    cx: &App,
) -> AnyElement {
    div()
        .w_full()
        .py(px(28.0))
        .px(px(12.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .child(Icon::new(icon).size(IconSize::Lg).color(cx.colors().muted))
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.colors().foreground)
                .child(title),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.colors().muted)
                .text_center()
                .child(subtitle),
        )
        .into_any_element()
}

fn render_autocomplete_overlay(
    doc_entity: &Entity<SqlEditorDocEntity>,
    palette: &ContentThemePalette,
    cx: &App,
) -> AnyElement {
    let doc = doc_entity.read(cx);
    let items: Vec<SchemaCompletionItem> = doc
        .autocomplete
        .items()
        .iter()
        .take(AUTOCOMPLETE_MAX_VISIBLE)
        .cloned()
        .collect();
    if items.is_empty() {
        return div().into_any_element();
    }
    let selected_idx = doc
        .autocomplete
        .selected_index
        .min(items.len().saturating_sub(1));
    let (anchor_x, anchor_y) = doc.caret_overlay_position();
    let insert_hint = doc.autocomplete_insert_hint;
    let dismiss_hint = doc.autocomplete_dismiss_hint;
    let _ = doc;

    let max_h = (items.len() as f32 * AUTOCOMPLETE_ROW_HEIGHT_PX + 12.0).min(248.0);
    let mut list_col = div()
        .id("sql-autocomplete-list")
        .max_h(px(max_h))
        .overflow_y_scroll()
        .p(px(4.0))
        .flex()
        .flex_col()
        .gap(px(2.0));

    for (i, item) in items.iter().enumerate() {
        list_col = list_col.child(render_autocomplete_row(
            i,
            item,
            i == selected_idx,
            doc_entity,
            cx,
        ));
    }

    div()
        .id("sql-autocomplete-popup")
        .absolute()
        .left(px(anchor_x))
        .top(px(anchor_y))
        .w(px(AUTOCOMPLETE_WIDTH_PX))
        .rounded(px(theme::PANEL_RADIUS_PX))
        .bg(theme::elevated_overlay_bg(cx))
        .border_1()
        .border_color(theme::hairline_border_color(cx))
        .shadow_lg()
        .on_mouse_down(MouseButton::Left, |_e, _w, cx| cx.stop_propagation())
        .child(list_col)
        .child(
            div()
                .px(px(8.0))
                .py(px(4.0))
                .border_t_1()
                .border_color(theme::hairline_border_color(cx))
                .flex()
                .justify_between()
                .text_xs()
                .text_color(with_alpha(palette.editor_text, 0.55))
                .child(insert_hint)
                .child(dismiss_hint),
        )
        .into_any_element()
}

fn render_autocomplete_row(
    idx: usize,
    item: &SchemaCompletionItem,
    is_selected: bool,
    doc_entity: &Entity<SqlEditorDocEntity>,
    cx: &App,
) -> AnyElement {
    let colors = cx.colors().clone();
    let icon = match item.kind {
        SchemaCompletionKind::Snippet => IconName::Sparkles,
        SchemaCompletionKind::Schema => IconName::Database,
        SchemaCompletionKind::Table => IconName::Table,
        SchemaCompletionKind::Column => IconName::Columns2,
        SchemaCompletionKind::Function => IconName::Terminal,
        SchemaCompletionKind::Keyword => IconName::Code,
    };
    let row_bg = if is_selected {
        with_alpha(colors.accent.color, 0.14)
    } else {
        herogpui::gpui::transparent_black()
    };
    let hover_bg = with_alpha(colors.foreground, 0.06);
    let click_doc = doc_entity.clone();
    let mut text_col = div().min_w_0().flex_1().flex().flex_col().child(
        div()
            .text_sm()
            .font_weight(if is_selected {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::MEDIUM
            })
            .text_color(colors.foreground)
            .truncate()
            .child(item.label.clone()),
    );
    if let Some(detail) = item.detail.clone() {
        text_col = text_col.child(
            div()
                .text_xs()
                .text_color(colors.muted)
                .truncate()
                .child(detail),
        );
    }
    div()
        .id(SharedString::from(format!("ac-item-{idx}")))
        .w_full()
        .px(px(8.0))
        .py(px(5.0))
        .rounded(px(6.0))
        .bg(row_bg)
        .hover(move |s| s.bg(hover_bg))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .on_mouse_down(MouseButton::Left, move |_e, window, cx| {
            cx.stop_propagation();
            click_doc.update(cx, |doc, cx| {
                let _ = doc.accept_autocomplete_at(idx, cx);
                window.focus(&doc.focus_handle, cx);
            });
        })
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(Icon::new(icon).size(IconSize::Sm).color(if is_selected {
                    colors.accent.color
                } else {
                    colors.muted
                }))
                .child(text_col),
        )
        .child(
            div()
                .px(px(5.0))
                .py(px(1.0))
                .rounded(px(4.0))
                .bg(with_alpha(colors.foreground, 0.06))
                .text_xs()
                .text_color(colors.muted)
                .child(completion_kind_tag(item.kind)),
        )
        .into_any_element()
}

fn sync_doc_with_workspace(ws: &mut TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) {
    let active_tab = ws.controller.tabs.active_tab();
    let initial_tab_sql = active_tab.and_then(|id| {
        ws.controller
            .tabs
            .tabs()
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| match &t.kind {
                TabKind::QueryEditor { sql } => Some(sql.clone()),
                _ => None,
            })
    });
    let ctrl_sql = ws.controller.editor_sql_text();
    let grammar = ws.active_grammar();
    let palette = ws.content_palette.clone();
    let font = editor_gpui_font(&ws.controller.settings);
    let font_size_px = ws.controller.settings.editor_font.size.max(10.0);
    let line_height_px = (font_size_px * ws.controller.settings.editor_font.line_height.max(1.1))
        .max(font_size_px + 2.0);
    let vim_mode = ws.controller.vim_mode;
    let empty_hint_text = ws.controller.strings.sql_editor_placeholder;
    let autocomplete_insert_hint = ws.controller.strings.autocomplete_insert_hint;
    let autocomplete_dismiss_hint = ws.controller.strings.autocomplete_dismiss_hint;
    let fingerprint = compute_catalog_fingerprint(ws, grammar);
    let catalog = (ws.sql_editor_ui.doc.read(cx).catalog_fingerprint != fingerprint)
        .then(|| build_schema_catalog(ws));

    let mut sync_back_text: Option<String> = None;
    ws.sql_editor_ui.doc.update(cx, |doc, _cx| {
        doc.palette = palette;
        doc.font = font;
        doc.font_size_px = font_size_px;
        doc.line_height_px = line_height_px;
        doc.char_advance_px = (font_size_px * 0.6).max(6.0);
        doc.empty_hint_text = empty_hint_text;
        doc.autocomplete_insert_hint = autocomplete_insert_hint;
        doc.autocomplete_dismiss_hint = autocomplete_dismiss_hint;
        if doc.grammar != grammar {
            doc.grammar = grammar;
            doc.highlighter = IncrementalLineHighlighter::new(grammar);
        }
        if let Some(items) = catalog {
            doc.autocomplete.set_catalog(items);
            doc.catalog_fingerprint = fingerprint;
        }
        if let Some(vm) = vim_mode {
            doc.vim.enabled = true;
            doc.vim.mode = from_ctrl_vim_mode(vm);
        } else if doc.vim.enabled {
            doc.vim.enabled = false;
        }
        sync_back_text = sync_doc_tab_buffer(doc, active_tab, initial_tab_sql, ctrl_sql);
    });
    if let Some(text) = sync_back_text {
        ws.controller.set_editor_sql(&text);
    }
}

fn sync_doc_tab_buffer(
    doc: &mut SqlEditorDocEntity,
    active_tab: Option<TabId>,
    initial_tab_sql: Option<String>,
    ctrl_sql: String,
) -> Option<String> {
    if doc.active_tab_id == active_tab {
        if ctrl_sql != doc.last_controller_sql {
            doc.rope = Rope::from_text(&ctrl_sql);
            doc.cursors = vec![CursorPosition::caret(doc.rope.len_chars())];
            doc.rebuild_line_starts();
            doc.highlighter
                .invalidate_lines(0, doc.line_starts.len().saturating_sub(1));
            doc.last_controller_sql = ctrl_sql;
        }
        return None;
    }

    if let Some(prev) = doc.active_tab_id {
        doc.tab_buffers.insert(
            prev,
            TabDocSnapshot {
                text: doc.rope.text(),
                cursors: doc.cursors.clone(),
                scroll_y: doc.scroll_y,
                scroll_x: doc.scroll_x,
            },
        );
    }
    doc.active_tab_id = active_tab;
    if let Some(next_id) = active_tab
        && let Some(snap) = doc.tab_buffers.get(&next_id).cloned()
    {
        doc.rope = Rope::from_text(&snap.text);
        doc.cursors = snap.cursors;
        doc.scroll_y = snap.scroll_y;
        doc.scroll_x = snap.scroll_x;
        doc.rebuild_line_starts();
        doc.last_controller_sql = snap.text.clone();
        return Some(snap.text);
    }

    let seed = initial_tab_sql
        .filter(|s| !s.is_empty())
        .unwrap_or(ctrl_sql);
    doc.rope = Rope::from_text(&seed);
    doc.cursors = vec![CursorPosition::caret(doc.rope.len_chars())];
    doc.scroll_y = 0.0;
    doc.scroll_x = 0.0;
    doc.rebuild_line_starts();
    doc.last_controller_sql = seed.clone();
    Some(seed)
}

fn load_sql_into_active_editor(
    ws: &mut TableMinusWorkspace,
    sql: &str,
    window: &mut Window,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let sql_owned = sql.to_owned();
    ws.controller.set_editor_sql(&sql_owned);
    ws.sql_editor_ui.doc.update(cx, |doc, cx| {
        doc.rope = Rope::from_text(&sql_owned);
        doc.cursors = vec![CursorPosition::caret(doc.rope.len_chars())];
        doc.rebuild_line_starts();
        doc.highlighter
            .invalidate_lines(0, doc.line_starts.len().saturating_sub(1));
        doc.autocomplete.dismiss();
        doc.last_controller_sql = sql_owned;
        doc.scroll_y = 0.0;
        doc.scroll_x = 0.0;
        window.focus(&doc.focus_handle, cx);
        cx.notify();
    });
    cx.notify();
}

pub(crate) fn cancel_active_query(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) {
    if let Some(token) = ws.results_ui.active_cancel.take() {
        token.cancel();
        let cancelled_msg = ws.controller.strings.query_cancelled_msg;
        let cancelled_toast = ws.controller.strings.query_cancelled_toast;
        if let Some(tab_id) = ws.controller.tabs.active_tab() {
            ws.ensure_tab_result(tab_id).fail(cancelled_msg);
        }
        ws.notify_toast(ToastSeverity::Info, cancelled_toast, None, cx);
        cx.notify();
    }
}

enum QueryStreamMsg {
    Event(ResultEvent),
    Done(Result<RunSummary, String>, u64),
}

pub(crate) fn trigger_run_query(
    ws: &mut TableMinusWorkspace,
    selected_only: bool,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let sql = {
        let doc = ws.sql_editor_ui.doc.read(cx);
        let primary = doc.primary_cursor();
        let (s, e) = primary.range();
        if e > s && (selected_only || has_non_empty_selection(&doc.cursors)) {
            doc.rope.slice(s, e)
        } else {
            doc.rope.text()
        }
    };
    if sql.trim().is_empty() {
        let title = ws.controller.strings.nothing_to_execute_toast;
        let desc = ws.controller.strings.enter_sql_before_running;
        ws.notify_toast(ToastSeverity::Info, title, Some(desc.into()), cx);
        return;
    }
    let Some(tab_id) = ws.controller.tabs.active_tab() else {
        let msg = ws.controller.strings.no_active_tab_toast;
        ws.notify_toast(ToastSeverity::Warning, msg, None, cx);
        return;
    };
    if let Some(prev) = ws.results_ui.active_cancel.take() {
        prev.cancel();
    }
    let Some(services) = ws.controller.services.clone() else {
        let msg = ws.controller.strings.no_db_service_active;
        ws.ensure_tab_result(tab_id).fail(msg);
        ws.controller.record_query_history(&sql, 0, None, false);
        cx.notify();
        return;
    };

    let cancel = CancellationToken::new();
    ws.results_ui.active_cancel = Some(cancel.clone());
    ws.ensure_tab_result(tab_id).reset_for_run();
    ws.controller
        .set_editor_sql(&ws.sql_editor_ui.doc.read(cx).rope.text());
    ws.console_log
        .push(sql.clone(), super::console_log::ConsoleLogKind::Query);

    let (ui_tx, ui_rx) = async_channel::unbounded::<QueryStreamMsg>();
    spawn_query_stream_worker(
        services,
        ws.controller.active_workspace,
        sql.clone(),
        cancel,
        ui_tx,
    );

    cx.spawn(async move |this, cx| {
        while let Ok(msg) = ui_rx.recv().await {
            let mut should_stop = false;
            let update_ok = this.update(cx, |ws, cx| match msg {
                QueryStreamMsg::Event(ev) => {
                    ws.ensure_tab_result(tab_id).push_event(ev.clone());
                    ws.controller.apply_query_stream_event(ev);
                    cx.notify();
                }
                QueryStreamMsg::Done(res, elapsed_ms) => {
                    finalize_stream_query(ws, tab_id, &sql, res, elapsed_ms);
                    should_stop = true;
                    cx.notify();
                }
            });
            if update_ok.is_err() || should_stop {
                break;
            }
        }
    })
    .detach();
    cx.notify();
}

fn spawn_query_stream_worker(
    services: std::sync::Arc<crate::services::ServiceHub>,
    workspace_id: tableminus_state::connection_window::WorkspaceId,
    sql_run: String,
    cancel: CancellationToken,
    ui_tx: async_channel::Sender<QueryStreamMsg>,
) {
    let runtime = services.runtime.clone();
    runtime.spawn(async move {
        let started = Instant::now();
        let (tx, mut rx) = mpsc::channel::<ResultEvent>(64);
        let fwd_tx = ui_tx.clone();
        let forwarder = tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                if fwd_tx.send(QueryStreamMsg::Event(ev)).await.is_err() {
                    break;
                }
            }
        });
        let guard = services.sessions.read().await;
        let run_res = guard
            .run_query(
                workspace_id,
                &sql_run,
                RunOptions::new(cancel.clone()),
                EditorSessionOptions::default(),
                tx,
                &cancel,
            )
            .await
            .map_err(|e| e.to_string());
        drop(guard);
        let _ = forwarder.await;
        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let _ = ui_tx.send(QueryStreamMsg::Done(run_res, elapsed_ms)).await;
    });
}

fn finalize_stream_query(
    ws: &mut TableMinusWorkspace,
    tab_id: TabId,
    sql: &str,
    res: Result<RunSummary, String>,
    elapsed_ms: u64,
) {
    ws.results_ui.active_cancel = None;
    let succeeded = res.is_ok();
    let tab_res = ws.ensure_tab_result(tab_id);
    match &res {
        Ok(summary) => tab_res.finish(summary.clone(), elapsed_ms),
        Err(err) => tab_res.fail(err.clone()),
    }
    let rows_affected = tab_res
        .accumulator
        .commands_completed
        .iter()
        .flatten()
        .copied()
        .sum::<u64>();
    let affected = (!tab_res.accumulator.commands_completed.is_empty()).then_some(rows_affected);
    ws.controller.apply_query_finished(res, elapsed_ms);
    ws.controller
        .record_query_history(sql, elapsed_ms, affected, succeeded);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MoveDir {
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    LineStart,
    LineEnd,
    DocStart,
    DocEnd,
}

fn has_non_empty_selection(cursors: &[CursorPosition]) -> bool {
    cursors.iter().any(|c| c.anchor != c.head)
}

fn compute_line_starts(rope: &Rope) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (idx, ch) in rope.text().chars().enumerate() {
        if ch == '\n' {
            starts.push(idx + 1);
        }
    }
    starts
}

fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map_or(s.len(), |(byte_idx, _)| byte_idx)
}

fn x_for_char_col(row: &ShapedEditorRow, col: usize, char_advance_px: f32) -> f32 {
    let char_len = row.text.chars().count();
    if col <= char_len {
        let byte_idx = char_to_byte_idx(&row.text, col);
        f32::from(row.shaped.x_for_index(byte_idx))
    } else {
        f32::from(row.shaped.width) + (col - char_len) as f32 * char_advance_px
    }
}

fn tabular_gutter_font(base: &Font) -> Font {
    let mut out = base.clone();
    out.features = FontFeatures(std::sync::Arc::new(vec![("tnum".to_owned(), 1)]));
    out
}

fn dedup_collapsed_cursors(cursors: &mut Vec<CursorPosition>) {
    if cursors.len() <= 1 {
        return;
    }
    let mut out: Vec<CursorPosition> = Vec::with_capacity(cursors.len());
    for c in cursors.drain(..) {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    if out.is_empty() {
        out.push(CursorPosition::caret(0));
    }
    *cursors = out;
}

fn move_vertical(char_idx: usize, delta_lines: isize, rope: &Rope, line_starts: &[usize]) -> usize {
    let cur_line = line_starts
        .partition_point(|&s| s <= char_idx)
        .saturating_sub(1);
    let col = char_idx.saturating_sub(line_starts[cur_line]);
    let target_line = if delta_lines < 0 {
        cur_line.saturating_sub(delta_lines.unsigned_abs())
    } else {
        (cur_line + delta_lines as usize).min(line_starts.len().saturating_sub(1))
    };
    let start = line_starts[target_line];
    let end = if target_line + 1 < line_starts.len() {
        line_starts[target_line + 1].saturating_sub(1)
    } else {
        rope.len_chars()
    };
    (start + col).min(end.max(start))
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'
}

fn word_range_at(rope: &Rope, char_idx: usize) -> (usize, usize) {
    let text = rope.text();
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    let idx = char_idx.min(chars.len().saturating_sub(1));
    if !is_ident_char(chars[idx]) {
        return (idx, (idx + 1).min(chars.len()));
    }
    let mut s = idx;
    while s > 0 && is_ident_char(chars[s - 1]) {
        s -= 1;
    }
    let mut e = idx + 1;
    while e < chars.len() && is_ident_char(chars[e]) {
        e += 1;
    }
    (s, e)
}

fn prev_word_boundary(rope: &Rope, char_idx: usize) -> usize {
    let text = rope.text();
    let chars: Vec<char> = text.chars().collect();
    let mut pos = char_idx.min(chars.len());
    while pos > 0 && !is_ident_char(chars[pos - 1]) {
        pos -= 1;
    }
    while pos > 0 && is_ident_char(chars[pos - 1]) {
        pos -= 1;
    }
    pos
}

fn next_word_boundary(rope: &Rope, char_idx: usize) -> usize {
    let text = rope.text();
    let chars: Vec<char> = text.chars().collect();
    let mut pos = char_idx.min(chars.len());
    while pos < chars.len() && is_ident_char(chars[pos]) {
        pos += 1;
    }
    while pos < chars.len() && !is_ident_char(chars[pos]) {
        pos += 1;
    }
    pos
}

fn identifier_prefix_before(rope: &Rope, char_idx: usize) -> Option<String> {
    let window_start = char_idx.saturating_sub(96);
    let slice = rope.slice(window_start, char_idx);
    let prefix: String = slice
        .chars()
        .rev()
        .take_while(|c| is_ident_char(*c))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if prefix.is_empty() {
        if slice.ends_with('.') {
            Some(String::new())
        } else {
            None
        }
    } else {
        Some(prefix)
    }
}

fn matching_bracket_pair(rope: &Rope, char_idx: usize) -> Option<(usize, usize)> {
    let len = rope.len_chars();
    if len == 0 {
        return None;
    }
    let window_start = char_idx.saturating_sub(BRACKET_SEARCH_LIMIT);
    let window_end = (char_idx + BRACKET_SEARCH_LIMIT).min(len);
    let slice: Vec<char> = rope.slice(window_start, window_end).chars().collect();
    let rel_cursor = char_idx - window_start;
    let candidate_idx = [rel_cursor, rel_cursor.saturating_sub(1)]
        .into_iter()
        .find(|&i| {
            slice
                .get(i)
                .is_some_and(|c| matches!(c, '(' | ')' | '[' | ']' | '{' | '}'))
        })?;
    let ch = slice[candidate_idx];
    let (partner, forward) = match ch {
        '(' => (')', true),
        '[' => (']', true),
        '{' => ('}', true),
        ')' => ('(', false),
        ']' => ('[', false),
        '}' => ('{', false),
        _ => return None,
    };
    let mut depth = 0i32;
    if forward {
        for (i, &c) in slice.iter().enumerate().skip(candidate_idx) {
            if c == ch {
                depth += 1;
            } else if c == partner {
                depth -= 1;
                if depth == 0 {
                    return Some((window_start + candidate_idx, window_start + i));
                }
            }
        }
    } else {
        for i in (0..=candidate_idx).rev() {
            let c = slice[i];
            if c == ch {
                depth += 1;
            } else if c == partner {
                depth -= 1;
                if depth == 0 {
                    return Some((window_start + i, window_start + candidate_idx));
                }
            }
        }
    }
    None
}

fn keystroke_to_vim_token(ks: &Keystroke) -> Option<String> {
    if ks.modifiers.control || ks.modifiers.platform || ks.modifiers.alt {
        return None;
    }
    let mapped = match ks.key.as_str() {
        "escape" => "Esc",
        "enter" => "Enter",
        "backspace" => "Backspace",
        "left" => "Left",
        "right" => "Right",
        "up" => "Up",
        "down" => "Down",
        _ => {
            return ks
                .key_char
                .clone()
                .or_else(|| (ks.key.len() == 1).then(|| ks.key.clone()));
        }
    };
    Some(mapped.to_owned())
}

fn apply_vim_motion(
    doc: &mut SqlEditorDocEntity,
    motion: VimMotion,
    extend: bool,
    cx: &mut Context<SqlEditorDocEntity>,
) {
    match motion {
        VimMotion::Left => doc.move_cursors(MoveDir::Left, extend, false, cx),
        VimMotion::Right => doc.move_cursors(MoveDir::Right, extend, false, cx),
        VimMotion::Up => doc.move_cursors(MoveDir::Up, extend, false, cx),
        VimMotion::Down => doc.move_cursors(MoveDir::Down, extend, false, cx),
        VimMotion::WordForward | VimMotion::WordEnd => {
            doc.move_cursors(MoveDir::Right, extend, true, cx);
        }
        VimMotion::WordBack => doc.move_cursors(MoveDir::Left, extend, true, cx),
        VimMotion::LineStart => doc.move_cursors(MoveDir::LineStart, extend, false, cx),
        VimMotion::LineEnd => doc.move_cursors(MoveDir::LineEnd, extend, false, cx),
        VimMotion::DocStart => doc.move_cursors(MoveDir::DocStart, extend, false, cx),
        VimMotion::DocEnd => doc.move_cursors(MoveDir::DocEnd, extend, false, cx),
    }
}

fn to_ctrl_vim_mode(mode: VimMode) -> crate::controller_types::VimMode {
    match mode {
        VimMode::Normal => crate::controller_types::VimMode::Normal,
        VimMode::Insert => crate::controller_types::VimMode::Insert,
        VimMode::Visual => crate::controller_types::VimMode::Visual,
        VimMode::Command => crate::controller_types::VimMode::Command,
    }
}

fn from_ctrl_vim_mode(mode: crate::controller_types::VimMode) -> VimMode {
    match mode {
        crate::controller_types::VimMode::Normal => VimMode::Normal,
        crate::controller_types::VimMode::Insert => VimMode::Insert,
        crate::controller_types::VimMode::Visual => VimMode::Visual,
        crate::controller_types::VimMode::Command => VimMode::Command,
    }
}

fn compute_catalog_fingerprint(ws: &TableMinusWorkspace, grammar: SqlGrammar) -> u64 {
    let s = ws.controller.cached_schemas.len() as u64;
    let o = ws.controller.cached_objects.len() as u64;
    let c = ws.controller.structure_columns.len() as u64;
    ((ws.controller.locale as u64) << 56) ^ (s << 40) ^ (o << 20) ^ (c << 4) ^ (grammar as u64)
}

fn sql_functions_catalog(s: &crate::i18n::Strings) -> [(&'static str, &'static str); 12] {
    [
        ("COUNT", s.sql_fn_count),
        ("SUM", s.sql_fn_sum),
        ("AVG", s.sql_fn_avg),
        ("MIN", s.sql_fn_min),
        ("MAX", s.sql_fn_max),
        ("COALESCE", s.sql_fn_coalesce),
        ("NULLIF", s.sql_fn_nullif),
        ("CAST", s.sql_fn_cast),
        ("LOWER", s.sql_fn_lower),
        ("UPPER", s.sql_fn_upper),
        ("LENGTH", s.sql_fn_length),
        ("NOW", s.sql_fn_now),
    ]
}

fn sql_snippets_catalog(
    s: &crate::i18n::Strings,
) -> [(&'static str, &'static str, &'static str); 11] {
    [
        (
            "sel · SELECT * FROM <table>",
            s.sql_snip_sel,
            "SELECT *\nFROM ",
        ),
        (
            "sell · SELECT * FROM <table> LIMIT 100",
            s.sql_snip_sell,
            "SELECT *\nFROM \nLIMIT 100;",
        ),
        (
            "selw · SELECT * FROM <table> WHERE ...",
            s.sql_snip_selw,
            "SELECT *\nFROM \nWHERE ",
        ),
        (
            "cnt · SELECT COUNT(*) FROM <table>",
            s.sql_snip_cnt,
            "SELECT COUNT(*) AS total\nFROM ",
        ),
        (
            "ins · INSERT INTO <table> (...) VALUES (...)",
            s.sql_snip_ins,
            "INSERT INTO  ()\nVALUES ();",
        ),
        (
            "upd · UPDATE <table> SET ... WHERE ...",
            s.sql_snip_upd,
            "UPDATE \nSET \nWHERE ;",
        ),
        (
            "del · DELETE FROM <table> WHERE ...",
            s.sql_snip_del,
            "DELETE FROM \nWHERE ;",
        ),
        (
            "join · INNER JOIN <table> ON ...",
            s.sql_snip_join,
            "INNER JOIN  ON ",
        ),
        (
            "ljoin · LEFT JOIN <table> ON ...",
            s.sql_snip_ljoin,
            "LEFT JOIN  ON ",
        ),
        (
            "cte · WITH cte AS (...)",
            s.sql_snip_cte,
            "WITH cte AS (\n    SELECT *\n    FROM \n)\nSELECT * FROM cte;",
        ),
        (
            "ct · CREATE TABLE <table> (...)",
            s.sql_snip_ct,
            "CREATE TABLE  (\n    id INTEGER PRIMARY KEY\n);",
        ),
    ]
}

fn detect_sql_autocomplete_context(
    rope: &Rope,
    head_char: usize,
    active_prefix: Option<&str>,
) -> AutocompleteContext {
    let prefix_chars = active_prefix.map_or(0, |p| p.chars().count());
    let before_end = head_char.saturating_sub(prefix_chars);
    let start_char = before_end.saturating_sub(96);
    let slice: String = rope
        .text()
        .chars()
        .skip(start_char)
        .take(before_end - start_char)
        .collect();
    if active_prefix.is_none() && !slice.chars().last().is_some_and(char::is_whitespace) {
        return AutocompleteContext::General;
    }
    let prev_word = slice
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .rfind(|w| !w.is_empty())
        .map(str::to_ascii_uppercase);
    match prev_word.as_deref() {
        Some("FROM" | "JOIN" | "INTO" | "UPDATE" | "TABLE" | "TRUNCATE" | "DESCRIBE") => {
            AutocompleteContext::TableExpecting
        }
        Some("SELECT" | "WHERE" | "ON" | "SET" | "BY" | "HAVING" | "AND" | "OR") => {
            AutocompleteContext::ColumnExpecting
        }
        _ => AutocompleteContext::General,
    }
}

fn build_schema_catalog(ws: &TableMinusWorkspace) -> Vec<SchemaCompletionItem> {
    let strings = &ws.controller.strings;
    let mut items = Vec::new();
    for (label, detail, insert_text) in sql_snippets_catalog(strings) {
        items.push(SchemaCompletionItem {
            label: label.to_owned(),
            kind: SchemaCompletionKind::Snippet,
            detail: Some(detail.to_owned()),
            insert_text: insert_text.to_owned(),
        });
    }
    for schema in &ws.controller.cached_schemas {
        items.push(SchemaCompletionItem {
            label: schema.name.clone(),
            kind: SchemaCompletionKind::Schema,
            detail: Some(strings.completion_schema_detail.to_owned()),
            insert_text: schema.name.clone(),
        });
    }
    for obj in &ws.controller.cached_objects {
        let kind_label = match obj.kind {
            ObjectKind::Table | ObjectKind::PartitionedTable | ObjectKind::ForeignTable => {
                strings.obj_kind_table
            }
            ObjectKind::View | ObjectKind::MaterializedView => strings.obj_kind_view,
            _ => strings.obj_kind_object,
        };
        let detail = match &obj.schema {
            Some(s) if !s.is_empty() => format!("{kind_label} · {s}"),
            _ => kind_label.to_owned(),
        };
        items.push(SchemaCompletionItem {
            label: obj.name.clone(),
            kind: SchemaCompletionKind::Table,
            detail: Some(detail),
            insert_text: obj.name.clone(),
        });
        items.push(SchemaCompletionItem {
            label: format!("SELECT * FROM {} LIMIT 100", obj.name),
            kind: SchemaCompletionKind::Snippet,
            detail: Some(strings.quick_query_tpl.replace("{table}", &obj.name)),
            insert_text: format!("SELECT * FROM {} LIMIT 100;", obj.name),
        });
    }
    for col in &ws.controller.structure_columns {
        items.push(SchemaCompletionItem {
            label: col.name.clone(),
            kind: SchemaCompletionKind::Column,
            detail: Some(col.data_type.clone()),
            insert_text: col.name.clone(),
        });
    }
    for (name, detail) in sql_functions_catalog(strings) {
        items.push(SchemaCompletionItem {
            label: name.to_owned(),
            kind: SchemaCompletionKind::Function,
            detail: Some(detail.to_owned()),
            insert_text: format!("{name}()"),
        });
    }
    for &kw in SQL_KEYWORDS {
        items.push(SchemaCompletionItem {
            label: kw.to_owned(),
            kind: SchemaCompletionKind::Keyword,
            detail: Some(strings.sql_keyword_detail.to_owned()),
            insert_text: kw.to_owned(),
        });
    }
    items
}

const fn completion_kind_tag(kind: SchemaCompletionKind) -> &'static str {
    match kind {
        SchemaCompletionKind::Snippet => "SNIP",
        SchemaCompletionKind::Schema => "SCHEMA",
        SchemaCompletionKind::Table => "TABLE",
        SchemaCompletionKind::Column => "COL",
        SchemaCompletionKind::Function => "FN",
        SchemaCompletionKind::Keyword => "KW",
    }
}

pub(crate) const fn grammar_label(grammar: SqlGrammar) -> &'static str {
    match grammar {
        SqlGrammar::PostgreSql => "PostgreSQL",
        SqlGrammar::MySql => "MySQL",
        SqlGrammar::Sqlite => "SQLite",
        SqlGrammar::DuckDb => "DuckDB",
        SqlGrammar::MsSql => "SQL Server",
        SqlGrammar::ClickHouse => "ClickHouse",
        SqlGrammar::Cql => "CQL",
        SqlGrammar::Mongo => "MongoDB",
        SqlGrammar::Redis => "Redis",
    }
}

fn single_line_preview(sql: &str, max_chars: usize) -> String {
    let collapsed: String = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = collapsed.chars().take(max_chars).collect();
    if collapsed.chars().count() > max_chars {
        out.push('…');
    }
    out
}
