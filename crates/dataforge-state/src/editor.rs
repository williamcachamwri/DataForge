//! SQL/text editor state: height-balanced Rope, UndoHistory with multi-caret
//! bottom-up edits, incremental UTF-16 syntax highlighting, schema-aware
//! autocomplete, Vim mode, split panes, saved queries/history, and RunEvent accumulator.

use std::sync::Arc;

/// Maximum number of Unicode scalar values (`char`s) per Rope leaf.
pub const MAX_LEAF_CHARS: usize = 2048;

/// Normalizes `\r\n` and `\r` line endings to `\n`.
#[must_use]
pub fn normalize_line_endings(input: &str) -> String {
    if !input.contains('\r') {
        return input.to_owned();
    }
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(ch);
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LeafMetrics {
    chars: usize,
    utf16: usize,
    newlines: usize,
}

impl LeafMetrics {
    fn of(s: &str) -> Self {
        let mut chars = 0usize;
        let mut utf16 = 0usize;
        let mut newlines = 0usize;
        for ch in s.chars() {
            chars += 1;
            utf16 += ch.len_utf16();
            if ch == '\n' {
                newlines += 1;
            }
        }
        Self {
            chars,
            utf16,
            newlines,
        }
    }
}

#[derive(Debug, Clone)]
enum RopeNode {
    Leaf {
        text: String,
        metrics: LeafMetrics,
    },
    Branch {
        left: Arc<RopeNode>,
        right: Arc<RopeNode>,
        height: u8,
        chars: usize,
        utf16: usize,
        newlines: usize,
    },
}

impl RopeNode {
    fn leaf(text: String) -> Arc<Self> {
        let metrics = LeafMetrics::of(&text);
        Arc::new(Self::Leaf { text, metrics })
    }

    fn branch(left: Arc<Self>, right: Arc<Self>) -> Arc<Self> {
        let height = 1 + left.height().max(right.height());
        let chars = left.chars() + right.chars();
        let utf16 = left.utf16() + right.utf16();
        let newlines = left.newlines() + right.newlines();
        Arc::new(Self::Branch {
            left,
            right,
            height,
            chars,
            utf16,
            newlines,
        })
    }

    fn height(&self) -> u8 {
        match self {
            Self::Leaf { .. } => 1,
            Self::Branch { height, .. } => *height,
        }
    }

    fn chars(&self) -> usize {
        match self {
            Self::Leaf { metrics, .. } => metrics.chars,
            Self::Branch { chars, .. } => *chars,
        }
    }

    fn utf16(&self) -> usize {
        match self {
            Self::Leaf { metrics, .. } => metrics.utf16,
            Self::Branch { utf16, .. } => *utf16,
        }
    }

    fn newlines(&self) -> usize {
        match self {
            Self::Leaf { metrics, .. } => metrics.newlines,
            Self::Branch { newlines, .. } => *newlines,
        }
    }

    fn balance_factor(&self) -> i32 {
        match self {
            Self::Leaf { .. } => 0,
            Self::Branch { left, right, .. } => {
                i32::from(left.height()) - i32::from(right.height())
            }
        }
    }

    fn balance(node: Arc<Self>) -> Arc<Self> {
        let bf = node.balance_factor();
        if bf > 1
            && let Self::Branch { left, right, .. } = node.as_ref()
        {
            if left.balance_factor() < 0
                && let Self::Branch {
                    left: ll,
                    right: lr,
                    ..
                } = left.as_ref()
            {
                let new_left = Self::rotate_left(ll, lr);
                return Self::rotate_right(&new_left, right);
            }
            return Self::rotate_right(left, right);
        }
        if bf < -1
            && let Self::Branch { left, right, .. } = node.as_ref()
        {
            if right.balance_factor() > 0
                && let Self::Branch {
                    left: rl,
                    right: rr,
                    ..
                } = right.as_ref()
            {
                let new_right = Self::rotate_right(rl, rr);
                return Self::rotate_left(left, &new_right);
            }
            return Self::rotate_left(left, right);
        }
        node
    }

    fn rotate_right(left: &Arc<Self>, right: &Arc<Self>) -> Arc<Self> {
        if let Self::Branch {
            left: ll,
            right: lr,
            ..
        } = left.as_ref()
        {
            let new_right = Self::branch(Arc::clone(lr), Arc::clone(right));
            Self::branch(Arc::clone(ll), new_right)
        } else {
            Self::branch(Arc::clone(left), Arc::clone(right))
        }
    }

    fn rotate_left(left: &Arc<Self>, right: &Arc<Self>) -> Arc<Self> {
        if let Self::Branch {
            left: rl,
            right: rr,
            ..
        } = right.as_ref()
        {
            let new_left = Self::branch(Arc::clone(left), Arc::clone(rl));
            Self::branch(new_left, Arc::clone(rr))
        } else {
            Self::branch(Arc::clone(left), Arc::clone(right))
        }
    }

    fn from_normalized_str(s: &str) -> Arc<Self> {
        let total_chars = s.chars().count();
        if total_chars <= MAX_LEAF_CHARS {
            return Self::leaf(s.to_owned());
        }
        let mid_char = total_chars / 2;
        let split_byte = s.char_indices().nth(mid_char).map_or(s.len(), |(b, _)| b);
        let left = Self::from_normalized_str(&s[..split_byte]);
        let right = Self::from_normalized_str(&s[split_byte..]);
        Self::balance(Self::branch(left, right))
    }

    fn split_at(node: &Arc<Self>, char_idx: usize) -> (Arc<Self>, Arc<Self>) {
        if char_idx == 0 {
            return (Self::leaf(String::new()), Arc::clone(node));
        }
        if char_idx >= node.chars() {
            return (Arc::clone(node), Self::leaf(String::new()));
        }
        match node.as_ref() {
            Self::Leaf { text, .. } => {
                let byte_idx = text
                    .char_indices()
                    .nth(char_idx)
                    .map_or(text.len(), |(b, _)| b);
                (
                    Self::leaf(text[..byte_idx].to_owned()),
                    Self::leaf(text[byte_idx..].to_owned()),
                )
            }
            Self::Branch { left, right, .. } => {
                let left_chars = left.chars();
                if char_idx < left_chars {
                    let (ll, lr) = Self::split_at(left, char_idx);
                    (ll, Self::balance(Self::branch(lr, Arc::clone(right))))
                } else if char_idx > left_chars {
                    let (rl, rr) = Self::split_at(right, char_idx - left_chars);
                    (Self::balance(Self::branch(Arc::clone(left), rl)), rr)
                } else {
                    (Arc::clone(left), Arc::clone(right))
                }
            }
        }
    }

    fn concat(left: Arc<Self>, right: Arc<Self>) -> Arc<Self> {
        if left.chars() == 0 {
            return right;
        }
        if right.chars() == 0 {
            return left;
        }
        if let (
            Self::Leaf {
                text: lt,
                metrics: lm,
            },
            Self::Leaf {
                text: rt,
                metrics: rm,
            },
        ) = (left.as_ref(), right.as_ref())
            && lm.chars + rm.chars <= MAX_LEAF_CHARS
        {
            let mut combined = String::with_capacity(lt.len() + rt.len());
            combined.push_str(lt);
            combined.push_str(rt);
            return Self::leaf(combined);
        }
        Self::balance(Self::branch(left, right))
    }

    fn collect_str(&self, out: &mut String) {
        match self {
            Self::Leaf { text, .. } => out.push_str(text),
            Self::Branch { left, right, .. } => {
                left.collect_str(out);
                right.collect_str(out);
            }
        }
    }

    fn char_to_utf16(&self, char_idx: usize) -> usize {
        match self {
            Self::Leaf { text, .. } => text.chars().take(char_idx).map(char::len_utf16).sum(),
            Self::Branch { left, right, .. } => {
                let lc = left.chars();
                if char_idx <= lc {
                    left.char_to_utf16(char_idx)
                } else {
                    left.utf16() + right.char_to_utf16(char_idx - lc)
                }
            }
        }
    }

    fn utf16_to_char(&self, utf16_idx: usize) -> usize {
        match self {
            Self::Leaf { text, .. } => {
                let mut acc = 0usize;
                let mut chars = 0usize;
                for ch in text.chars() {
                    let w = ch.len_utf16();
                    if acc + w > utf16_idx {
                        break;
                    }
                    acc += w;
                    chars += 1;
                }
                chars
            }
            Self::Branch { left, right, .. } => {
                let lu = left.utf16();
                if utf16_idx <= lu {
                    left.utf16_to_char(utf16_idx)
                } else {
                    left.chars() + right.utf16_to_char(utf16_idx - lu)
                }
            }
        }
    }
}

/// Height-balanced Rope with 2,048-char leaves and `\n`-normalized line endings.
#[derive(Debug, Clone)]
pub struct Rope {
    root: Arc<RopeNode>,
}

impl Default for Rope {
    fn default() -> Self {
        Self::new()
    }
}

impl Rope {
    /// Creates an empty [`Rope`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            root: RopeNode::leaf(String::new()),
        }
    }

    /// Creates a [`Rope`] from `text`, normalizing `\r\n` and `\r` to `\n`.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        let normalized = normalize_line_endings(text);
        Self {
            root: RopeNode::from_normalized_str(&normalized),
        }
    }

    /// Total number of Unicode scalar values (`char`s).
    #[must_use]
    pub fn len_chars(&self) -> usize {
        self.root.chars()
    }

    /// Total number of UTF-16 code units.
    #[must_use]
    pub fn len_utf16(&self) -> usize {
        self.root.utf16()
    }

    /// Tree height (for balance verification).
    #[must_use]
    pub fn height(&self) -> u8 {
        self.root.height()
    }

    /// Number of lines (`newlines + 1`).
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.root.newlines() + 1
    }

    /// Inserts `text` (normalized to `\n`) at `char_idx` (clamped to `len_chars()`).
    pub fn insert(&mut self, char_idx: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        let idx = char_idx.min(self.len_chars());
        let normalized = normalize_line_endings(text);
        let mid = RopeNode::from_normalized_str(&normalized);
        let (left, right) = RopeNode::split_at(&self.root, idx);
        let merged = RopeNode::concat(left, mid);
        self.root = RopeNode::concat(merged, right);
    }

    /// Deletes `[start..end)` in `char` offsets (clamped).
    pub fn delete(&mut self, start: usize, end: usize) {
        let len = self.len_chars();
        let s = start.min(len);
        let e = end.min(len);
        if s >= e {
            return;
        }
        let (left, rest) = RopeNode::split_at(&self.root, s);
        let (_, right) = RopeNode::split_at(&rest, e - s);
        self.root = RopeNode::concat(left, right);
    }

    /// Extracts `[start..end)` in `char` offsets as a `String`.
    #[must_use]
    pub fn slice(&self, start: usize, end: usize) -> String {
        let len = self.len_chars();
        let s = start.min(len);
        let e = end.min(len);
        if s >= e {
            return String::new();
        }
        let (_, rest) = RopeNode::split_at(&self.root, s);
        let (mid, _) = RopeNode::split_at(&rest, e - s);
        let mut out = String::new();
        mid.collect_str(&mut out);
        out
    }

    /// Converts a `char` offset to a UTF-16 code unit offset.
    #[must_use]
    pub fn char_to_utf16(&self, char_idx: usize) -> usize {
        self.root.char_to_utf16(char_idx.min(self.len_chars()))
    }

    /// Converts a UTF-16 code unit offset to a `char` offset.
    #[must_use]
    pub fn utf16_to_char(&self, utf16_idx: usize) -> usize {
        self.root.utf16_to_char(utf16_idx.min(self.len_utf16()))
    }

    /// Returns the full text of the rope.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        self.root.collect_str(&mut out);
        out
    }

    /// Returns the 0-based line `line_idx` (without trailing `\n`), if in range.
    #[must_use]
    pub fn line(&self, line_idx: usize) -> Option<String> {
        let full = self.text();
        full.split('\n').nth(line_idx).map(str::to_owned)
    }
}

/// A single cursor/selection in `cursorPositions` (in `char` offsets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CursorPosition {
    pub anchor: usize,
    pub head: usize,
}

impl CursorPosition {
    /// Creates a collapsed caret at `offset`.
    #[must_use]
    pub const fn caret(offset: usize) -> Self {
        Self {
            anchor: offset,
            head: offset,
        }
    }

    /// Returns `(start, end)` ordered ascending.
    #[must_use]
    pub const fn range(self) -> (usize, usize) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
}

/// Atomic text replacement in `char` offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub start: usize,
    pub end: usize,
    pub deleted: String,
    pub inserted: String,
}

/// One undoable step containing one or more edits (applied bottom-up) and cursor snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoGroup {
    pub edits: Vec<TextEdit>,
    pub before_cursors: Vec<CursorPosition>,
    pub after_cursors: Vec<CursorPosition>,
}

/// Undo/redo history for [`Rope`] with multi-caret bottom-up edit support.
#[derive(Debug, Default, Clone)]
pub struct UndoHistory {
    undo_stack: Vec<UndoGroup>,
    redo_stack: Vec<UndoGroup>,
}

impl UndoHistory {
    /// Creates an empty undo history.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies `insert_text` at every cursor in `cursor_positions` bottom-up
    /// (descending offset) so earlier offsets remain valid, and pushes one [`UndoGroup`].
    pub fn apply_multi_caret_insert(
        &mut self,
        rope: &mut Rope,
        cursor_positions: &mut Vec<CursorPosition>,
        insert_text: &str,
    ) {
        let before_cursors = cursor_positions.clone();
        let normalized = normalize_line_endings(insert_text);
        let insert_chars = normalized.chars().count();

        // Sort indices descending by range start so bottom-up application never shifts earlier offsets.
        let mut ordered: Vec<(usize, CursorPosition)> =
            cursor_positions.iter().copied().enumerate().collect();
        ordered.sort_by(|a, b| {
            b.1.range()
                .0
                .cmp(&a.1.range().0)
                .then_with(|| b.0.cmp(&a.0))
        });

        let mut edits = Vec::with_capacity(ordered.len());
        for (_, cursor) in &ordered {
            let (start, end) = cursor.range();
            let deleted = rope.slice(start, end);
            rope.delete(start, end);
            rope.insert(start, &normalized);
            edits.push(TextEdit {
                start,
                end,
                deleted,
                inserted: normalized.clone(),
            });
        }

        // Recompute resulting cursor positions in ascending order.
        let mut asc: Vec<(usize, CursorPosition)> =
            cursor_positions.iter().copied().enumerate().collect();
        asc.sort_by(|a, b| {
            a.1.range()
                .0
                .cmp(&b.1.range().0)
                .then_with(|| a.0.cmp(&b.0))
        });
        let mut delta: isize = 0;
        let mut updated = vec![CursorPosition::caret(0); cursor_positions.len()];
        for (orig_idx, cursor) in asc {
            let (start, end) = cursor.range();
            let new_pos = usize::try_from(isize::try_from(start).unwrap_or(0) + delta).unwrap_or(0)
                + insert_chars;
            updated[orig_idx] = CursorPosition::caret(new_pos);
            let removed = isize::try_from(end - start).unwrap_or(0);
            let added = isize::try_from(insert_chars).unwrap_or(0);
            delta += added - removed;
        }
        *cursor_positions = updated.clone();

        self.undo_stack.push(UndoGroup {
            edits,
            before_cursors,
            after_cursors: updated,
        });
        self.redo_stack.clear();
    }

    /// Undoes the most recent [`UndoGroup`], restoring `rope` and `cursor_positions`.
    pub fn undo(&mut self, rope: &mut Rope, cursor_positions: &mut Vec<CursorPosition>) -> bool {
        let Some(group) = self.undo_stack.pop() else {
            return false;
        };
        // `group.edits` were applied bottom-up (descending `start`). Undoing in reverse
        // order (top-down, ascending `start`) restores each earlier region so all
        // subsequent `edit.start` offsets match the document again.
        for edit in group.edits.iter().rev() {
            let ins_chars = edit.inserted.chars().count();
            rope.delete(edit.start, edit.start + ins_chars);
            rope.insert(edit.start, &edit.deleted);
        }
        *cursor_positions = group.before_cursors.clone();
        self.redo_stack.push(group);
        true
    }

    /// Redoes the most recently undone [`UndoGroup`].
    pub fn redo(&mut self, rope: &mut Rope, cursor_positions: &mut Vec<CursorPosition>) -> bool {
        let Some(group) = self.redo_stack.pop() else {
            return false;
        };
        for edit in &group.edits {
            rope.delete(edit.start, edit.end);
            rope.insert(edit.start, &edit.inserted);
        }
        *cursor_positions = group.after_cursors.clone();
        self.undo_stack.push(group);
        true
    }

    /// Returns whether undo is available.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Returns whether redo is available.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

/// A syntax-highlighted token span in UTF-16 code units relative to a line's start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utf16SyntaxSpan {
    pub start_utf16: usize,
    pub end_utf16: usize,
    pub kind: tablepro_core::sql_syntax::SyntaxKind,
}

/// Incremental per-line syntax highlighter backed by `tablepro_core::sql_syntax::syntax_tokens`.
///
/// Converts UTF-8 byte ranges from `syntax_tokens` into UTF-16 code unit offsets
/// for Win32/DirectWrite text layout and caches unchanged lines.
#[derive(Debug, Clone)]
pub struct IncrementalLineHighlighter {
    grammar: tablepro_core::sql_syntax::SqlGrammar,
    cached_lines: Vec<Option<(String, Vec<Utf16SyntaxSpan>)>>,
}

impl IncrementalLineHighlighter {
    /// Creates a highlighter for `grammar`.
    #[must_use]
    pub fn new(grammar: tablepro_core::sql_syntax::SqlGrammar) -> Self {
        Self {
            grammar,
            cached_lines: Vec::new(),
        }
    }

    /// Invalidates cached tokens for `[start_line..=end_line]`.
    pub fn invalidate_lines(&mut self, start_line: usize, end_line: usize) {
        let end = end_line.min(self.cached_lines.len().saturating_sub(1));
        for idx in start_line..=end {
            if let Some(slot) = self.cached_lines.get_mut(idx) {
                *slot = None;
            }
        }
    }

    /// Returns UTF-16 syntax spans for `line_idx` in `rope`, lexing only on cache miss.
    pub fn highlight_line(&mut self, rope: &Rope, line_idx: usize) -> Vec<Utf16SyntaxSpan> {
        let Some(line_text) = rope.line(line_idx) else {
            return Vec::new();
        };
        if self.cached_lines.len() <= line_idx {
            self.cached_lines.resize(line_idx + 1, None);
        }
        if let Some((cached_text, spans)) = &self.cached_lines[line_idx]
            && cached_text == &line_text
        {
            return spans.clone();
        }
        let spans = lex_line_utf16(&line_text, self.grammar);
        self.cached_lines[line_idx] = Some((line_text, spans.clone()));
        spans
    }
}

/// Lexes a single line using `tablepro_core::sql_syntax::syntax_tokens` into UTF-16 spans.
#[must_use]
pub fn lex_line_utf16(
    line: &str,
    grammar: tablepro_core::sql_syntax::SqlGrammar,
) -> Vec<Utf16SyntaxSpan> {
    let settings = tablepro_core::sql_syntax::script::LexicalSettings::default_for(grammar);
    let tokens = tablepro_core::sql_syntax::syntax_tokens(line, grammar, settings);
    let mut byte_to_utf16 = vec![0usize; line.len() + 1];
    let mut utf16_acc = 0usize;
    for (byte_idx, ch) in line.char_indices() {
        byte_to_utf16[byte_idx] = utf16_acc;
        utf16_acc += ch.len_utf16();
    }
    byte_to_utf16[line.len()] = utf16_acc;

    tokens
        .into_iter()
        .filter_map(|tok| {
            let start = *byte_to_utf16.get(tok.range.start)?;
            let end = *byte_to_utf16.get(tok.range.end)?;
            Some(Utf16SyntaxSpan {
                start_utf16: start,
                end_utf16: end,
                kind: tok.kind,
            })
        })
        .collect()
}

/// Kind of completion item offered by schema-aware autocomplete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SchemaCompletionKind {
    Snippet,
    Table,
    Schema,
    Column,
    Function,
    Keyword,
}

/// SQL clause context around the cursor to prioritize relevant completions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AutocompleteContext {
    #[default]
    General,
    /// Cursor follows `FROM`, `JOIN`, `INTO`, `UPDATE`, `TABLE`, etc.
    TableExpecting,
    /// Cursor follows `SELECT`, `WHERE`, `ON`, `SET`, `ORDER BY`, `GROUP BY`, etc.
    ColumnExpecting,
}

/// A single schema-aware autocomplete candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaCompletionItem {
    pub label: String,
    pub kind: SchemaCompletionKind,
    pub detail: Option<String>,
    pub insert_text: String,
}

/// Schema-aware autocomplete popup state.
#[derive(Debug, Default, Clone)]
pub struct AutocompleteState {
    pub is_open: bool,
    pub prefix: String,
    catalog: Vec<SchemaCompletionItem>,
    filtered: Vec<SchemaCompletionItem>,
    pub selected_index: usize,
}

impl AutocompleteState {
    /// Creates an autocomplete state with an initial schema catalog.
    #[must_use]
    pub fn with_catalog(catalog: Vec<SchemaCompletionItem>) -> Self {
        Self {
            is_open: false,
            prefix: String::new(),
            catalog,
            filtered: Vec::new(),
            selected_index: 0,
        }
    }

    /// Replaces the schema catalog and refreshes active matches.
    pub fn set_catalog(&mut self, catalog: Vec<SchemaCompletionItem>) {
        self.catalog = catalog;
        if self.is_open {
            let prefix = self.prefix.clone();
            self.update_prefix(&prefix);
        }
    }

    /// Opens or updates autocomplete for `prefix`.
    pub fn update_prefix(&mut self, prefix: &str) {
        self.update_prefix_with_context(prefix, AutocompleteContext::General);
    }

    /// Opens or updates autocomplete for `prefix` using the surrounding SQL clause `context`.
    pub fn update_prefix_with_context(&mut self, prefix: &str, context: AutocompleteContext) {
        self.prefix = prefix.to_owned();
        let needle = prefix.to_lowercase();
        let mut items: Vec<SchemaCompletionItem> = self
            .catalog
            .iter()
            .filter(|item| {
                if needle.is_empty() {
                    return match context {
                        AutocompleteContext::TableExpecting => matches!(
                            item.kind,
                            SchemaCompletionKind::Table | SchemaCompletionKind::Schema
                        ),
                        AutocompleteContext::ColumnExpecting => matches!(
                            item.kind,
                            SchemaCompletionKind::Column | SchemaCompletionKind::Function
                        ),
                        AutocompleteContext::General => true,
                    };
                }
                item.label.to_lowercase().contains(&needle)
                    || (item.kind == SchemaCompletionKind::Snippet
                        && item.insert_text.to_lowercase().contains(&needle))
            })
            .cloned()
            .collect();
        items.sort_by(|a, b| {
            let a_starts = !needle.is_empty() && a.label.to_lowercase().starts_with(&needle);
            let b_starts = !needle.is_empty() && b.label.to_lowercase().starts_with(&needle);
            let a_ctx = context_rank(a.kind, context);
            let b_ctx = context_rank(b.kind, context);
            b_starts
                .cmp(&a_starts)
                .then_with(|| a_ctx.cmp(&b_ctx))
                .then_with(|| a.kind.cmp(&b.kind))
                .then_with(|| a.label.cmp(&b.label))
        });
        self.filtered = items;
        self.selected_index = 0;
        self.is_open = !self.filtered.is_empty();
    }

    /// Returns currently matched items.
    #[must_use]
    pub fn items(&self) -> &[SchemaCompletionItem] {
        &self.filtered
    }

    /// Moves selection down.
    pub fn move_next(&mut self) {
        if !self.filtered.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.filtered.len();
        }
    }

    /// Moves selection up.
    pub fn move_prev(&mut self) {
        if !self.filtered.is_empty() {
            self.selected_index = if self.selected_index == 0 {
                self.filtered.len() - 1
            } else {
                self.selected_index - 1
            };
        }
    }

    /// Accepts the currently selected item and closes the popup.
    pub fn accept(&mut self) -> Option<SchemaCompletionItem> {
        let item = self.filtered.get(self.selected_index).cloned()?;
        self.dismiss();
        Some(item)
    }

    /// Closes the autocomplete popup.
    pub fn dismiss(&mut self) {
        self.is_open = false;
        self.filtered.clear();
        self.selected_index = 0;
    }
}

const fn context_rank(kind: SchemaCompletionKind, context: AutocompleteContext) -> u8 {
    match context {
        AutocompleteContext::TableExpecting => match kind {
            SchemaCompletionKind::Table => 0,
            SchemaCompletionKind::Schema => 1,
            SchemaCompletionKind::Snippet => 2,
            SchemaCompletionKind::Keyword => 3,
            SchemaCompletionKind::Column => 4,
            SchemaCompletionKind::Function => 5,
        },
        AutocompleteContext::ColumnExpecting => match kind {
            SchemaCompletionKind::Column => 0,
            SchemaCompletionKind::Function => 1,
            SchemaCompletionKind::Table => 2,
            SchemaCompletionKind::Snippet => 3,
            SchemaCompletionKind::Keyword => 4,
            SchemaCompletionKind::Schema => 5,
        },
        AutocompleteContext::General => match kind {
            SchemaCompletionKind::Snippet => 0,
            SchemaCompletionKind::Table => 1,
            SchemaCompletionKind::Schema => 2,
            SchemaCompletionKind::Column => 3,
            SchemaCompletionKind::Function => 4,
            SchemaCompletionKind::Keyword => 5,
        },
    }
}

/// Vim modal editing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VimMode {
    Normal,
    Insert,
    Visual,
    Command,
}

/// Pending Vim operator in Normal mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimOperator {
    Delete,
    Change,
    Yank,
}

/// Vim motion primitives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimMotion {
    Left,
    Right,
    Up,
    Down,
    WordForward,
    WordBack,
    WordEnd,
    LineStart,
    LineEnd,
    DocStart,
    DocEnd,
}

/// Output action produced by [`VimState::handle_key`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VimAction {
    None,
    Move(VimMotion, usize),
    ApplyOperator(VimOperator, VimMotion, usize),
    LineOperator(VimOperator, usize),
    ExecuteCommand(String),
}

/// Optional Vim mode state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VimState {
    pub enabled: bool,
    pub mode: VimMode,
    pub pending_operator: Option<VimOperator>,
    pub count_prefix: String,
    pub command_buffer: String,
    pub pending_g: bool,
    pub register: String,
}

impl Default for VimState {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: VimMode::Normal,
            pending_operator: None,
            count_prefix: String::new(),
            command_buffer: String::new(),
            pending_g: false,
            register: String::new(),
        }
    }
}

impl VimState {
    /// Creates an enabled or disabled Vim state machine.
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    fn take_count(&mut self) -> usize {
        let n = self.count_prefix.parse::<usize>().unwrap_or(1).max(1);
        self.count_prefix.clear();
        n
    }

    /// Processes a single key token (e.g. `"Esc"`, `"i"`, `"d"`, `"w"`, `":"`, `"Enter"`).
    pub fn handle_key(&mut self, key: &str) -> VimAction {
        if !self.enabled {
            return VimAction::None;
        }
        if key == "Esc" {
            self.mode = VimMode::Normal;
            self.pending_operator = None;
            self.count_prefix.clear();
            self.command_buffer.clear();
            self.pending_g = false;
            return VimAction::None;
        }
        match self.mode {
            VimMode::Insert => VimAction::None,
            VimMode::Command => self.handle_command_key(key),
            VimMode::Normal | VimMode::Visual => self.handle_normal_or_visual(key),
        }
    }

    fn handle_command_key(&mut self, key: &str) -> VimAction {
        if key == "Enter" {
            let cmd = std::mem::take(&mut self.command_buffer);
            self.mode = VimMode::Normal;
            return VimAction::ExecuteCommand(cmd);
        }
        if key == "Backspace" {
            self.command_buffer.pop();
            if self.command_buffer.is_empty() {
                self.mode = VimMode::Normal;
            }
            return VimAction::None;
        }
        if key.chars().count() == 1 {
            self.command_buffer.push_str(key);
        }
        VimAction::None
    }

    fn handle_normal_or_visual(&mut self, key: &str) -> VimAction {
        if key.len() == 1
            && let Some(ch) = key.chars().next()
            && ch.is_ascii_digit()
            && !(ch == '0' && self.count_prefix.is_empty())
        {
            self.count_prefix.push(ch);
            return VimAction::None;
        }

        if self.pending_g {
            self.pending_g = false;
            if key == "g" {
                return self.emit_motion(VimMotion::DocStart);
            }
        }

        match key {
            "i" | "a" | "o" | "O" if self.pending_operator.is_none() => {
                self.mode = VimMode::Insert;
                VimAction::None
            }
            "v" if self.pending_operator.is_none() => {
                self.mode = if self.mode == VimMode::Visual {
                    VimMode::Normal
                } else {
                    VimMode::Visual
                };
                VimAction::None
            }
            ":" if self.pending_operator.is_none() => {
                self.mode = VimMode::Command;
                self.command_buffer.clear();
                VimAction::None
            }
            "g" => {
                self.pending_g = true;
                VimAction::None
            }
            "d" => self.handle_operator(VimOperator::Delete),
            "c" => self.handle_operator(VimOperator::Change),
            "y" => self.handle_operator(VimOperator::Yank),
            _ => match Self::parse_motion(key) {
                Some(motion) => self.emit_motion(motion),
                None => VimAction::None,
            },
        }
    }

    fn handle_operator(&mut self, op: VimOperator) -> VimAction {
        if self.pending_operator == Some(op) {
            self.pending_operator = None;
            let count = self.take_count();
            if op == VimOperator::Change {
                self.mode = VimMode::Insert;
            }
            return VimAction::LineOperator(op, count);
        }
        self.pending_operator = Some(op);
        VimAction::None
    }

    fn emit_motion(&mut self, motion: VimMotion) -> VimAction {
        let count = self.take_count();
        if let Some(op) = self.pending_operator.take() {
            if op == VimOperator::Change {
                self.mode = VimMode::Insert;
            }
            VimAction::ApplyOperator(op, motion, count)
        } else {
            VimAction::Move(motion, count)
        }
    }

    fn parse_motion(key: &str) -> Option<VimMotion> {
        Some(match key {
            "h" | "Left" => VimMotion::Left,
            "j" | "Down" => VimMotion::Down,
            "k" | "Up" => VimMotion::Up,
            "l" | "Right" => VimMotion::Right,
            "w" => VimMotion::WordForward,
            "b" => VimMotion::WordBack,
            "e" => VimMotion::WordEnd,
            "0" => VimMotion::LineStart,
            "$" => VimMotion::LineEnd,
            "G" => VimMotion::DocEnd,
            _ => return None,
        })
    }
}

/// Default split item holding priority (< 490 so divider drags always win, per `split-views.md`).
pub const SPLIT_PANE_HOLDING_PRIORITY: u16 = 260;

/// Orientation of a split pane container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

/// Identifier for an editor leaf pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EditorPaneId(pub u64);

/// Layout properties for a hosted split pane item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitPaneLayoutPolicy {
    /// Must remain `< 490` (`dragThatCannotResizeWindow`).
    pub holding_priority: u16,
    /// Must be `true` (`sizingOptions = []`) so minimum width does not pin dividers.
    pub sizing_options_empty: bool,
    pub detail_minimum_thickness: Option<u32>,
}

impl Default for SplitPaneLayoutPolicy {
    fn default() -> Self {
        Self {
            holding_priority: SPLIT_PANE_HOLDING_PRIORITY,
            sizing_options_empty: true,
            detail_minimum_thickness: None,
        }
    }
}

/// Recursive split pane tree for editor panes.
#[derive(Debug, Clone, PartialEq)]
pub enum EditorSplitNode {
    Leaf {
        pane_id: EditorPaneId,
        policy: SplitPaneLayoutPolicy,
    },
    Split {
        axis: SplitAxis,
        ratio: f64,
        first: Box<EditorSplitNode>,
        second: Box<EditorSplitNode>,
    },
}

impl EditorSplitNode {
    /// Creates a single-pane root.
    #[must_use]
    pub fn single(pane_id: EditorPaneId) -> Self {
        Self::Leaf {
            pane_id,
            policy: SplitPaneLayoutPolicy::default(),
        }
    }

    /// Splits `target_pane` into `target_pane` and `new_pane` along `axis`.
    pub fn split_pane(
        &mut self,
        target_pane: EditorPaneId,
        new_pane: EditorPaneId,
        axis: SplitAxis,
    ) -> bool {
        match self {
            Self::Leaf { pane_id, policy } if *pane_id == target_pane => {
                let first = Box::new(Self::Leaf {
                    pane_id: *pane_id,
                    policy: *policy,
                });
                let second = Box::new(Self::single(new_pane));
                *self = Self::Split {
                    axis,
                    ratio: 0.5,
                    first,
                    second,
                };
                true
            }
            Self::Leaf { .. } => false,
            Self::Split { first, second, .. } => {
                first.split_pane(target_pane, new_pane, axis)
                    || second.split_pane(target_pane, new_pane, axis)
            }
        }
    }

    /// Returns all leaf pane ids in traversal order.
    #[must_use]
    pub fn leaf_ids(&self) -> Vec<EditorPaneId> {
        let mut out = Vec::new();
        self.collect_leaf_ids(&mut out);
        out
    }

    fn collect_leaf_ids(&self, out: &mut Vec<EditorPaneId>) {
        match self {
            Self::Leaf { pane_id, .. } => out.push(*pane_id),
            Self::Split { first, second, .. } => {
                first.collect_leaf_ids(out);
                second.collect_leaf_ids(out);
            }
        }
    }
}

/// A saved query in the editor library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedQueryEntry {
    pub id: String,
    pub name: String,
    pub folder: Option<String>,
    pub sql: String,
}

/// An executed query recorded in query history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryHistoryEntry {
    pub sql: String,
    pub connection_id: String,
    pub elapsed_ms: u64,
    pub succeeded: bool,
}

/// Bounded query history model with substring search.
#[derive(Debug, Clone)]
pub struct QueryHistoryModel {
    entries: Vec<QueryHistoryEntry>,
    max_entries: usize,
}

impl QueryHistoryModel {
    /// Creates a history buffer retaining at most `max_entries` items.
    #[must_use]
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries: max_entries.max(1),
        }
    }

    /// Records a new execution at the front (most recent first).
    pub fn record(&mut self, entry: QueryHistoryEntry) {
        self.entries.insert(0, entry);
        if self.entries.len() > self.max_entries {
            self.entries.truncate(self.max_entries);
        }
    }

    /// Searches history entries by case-insensitive SQL substring.
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<&QueryHistoryEntry> {
        let q = query.trim().to_lowercase();
        self.entries
            .iter()
            .filter(|e| q.is_empty() || e.sql.to_lowercase().contains(&q))
            .collect()
    }

    /// Returns all entries most-recent first.
    #[must_use]
    pub fn entries(&self) -> &[QueryHistoryEntry] {
        &self.entries
    }
}

/// Accumulated result set from a streaming query run.
#[derive(Debug, Clone, PartialEq)]
pub struct AccumulatedResultSet {
    pub columns: Arc<[tablepro_core::column::ResultColumn]>,
    pub rows: Vec<Vec<tablepro_core::value::Value>>,
    pub retained: u64,
    pub total_rows: Option<u64>,
    pub truncation: Option<tablepro_core::Truncation>,
    pub row_limit_reached: bool,
    pub byte_limit_reached: bool,
}

/// Streaming accumulator consuming `tablepro_core::ResultEvent` and `RunSummary`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct RunEventAccumulator {
    pub result_sets: Vec<AccumulatedResultSet>,
    pub notices: Vec<tablepro_core::ServerNotice>,
    pub notice_limit_reached: Option<u64>,
    pub commands_completed: Vec<Option<u64>>,
    pub summary: Option<tablepro_core::RunSummary>,
}

impl RunEventAccumulator {
    /// Creates an empty accumulator for a new query run.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies one streaming [`tablepro_core::ResultEvent`].
    pub fn push_event(&mut self, event: tablepro_core::ResultEvent) {
        use tablepro_core::ResultEvent;
        match event {
            ResultEvent::ResultSetStarted { columns } => {
                self.result_sets.push(AccumulatedResultSet {
                    columns,
                    rows: Vec::new(),
                    retained: 0,
                    total_rows: None,
                    truncation: None,
                    row_limit_reached: false,
                    byte_limit_reached: false,
                });
            }
            ResultEvent::ColumnsResolved { set_index, columns } => {
                if let Some(set) = self.result_sets.get_mut(set_index) {
                    set.columns = columns;
                }
            }
            ResultEvent::Rows(batch) => {
                if let Some(set) = self.result_sets.last_mut() {
                    set.retained = set.retained.saturating_add(batch.len() as u64);
                    set.rows.extend(batch);
                }
            }
            ResultEvent::RowLimitReached { retained } => {
                if let Some(set) = self.result_sets.last_mut() {
                    set.row_limit_reached = true;
                    set.retained = retained;
                }
            }
            ResultEvent::ByteLimitReached { retained } => {
                if let Some(set) = self.result_sets.last_mut() {
                    set.byte_limit_reached = true;
                    set.retained = retained;
                }
            }
            ResultEvent::ResultSetFinished {
                retained,
                total_rows,
                truncation,
            } => {
                if let Some(set) = self.result_sets.last_mut() {
                    set.retained = retained;
                    set.total_rows = total_rows;
                    set.truncation = Some(truncation);
                }
            }
            ResultEvent::CommandCompleted { rows_affected } => {
                self.commands_completed.push(rows_affected);
            }
            ResultEvent::Notice(notice) => {
                self.notices.push(notice);
            }
            ResultEvent::NoticeLimitReached { retained } => {
                self.notice_limit_reached = Some(retained);
            }
        }
    }

    /// Finalizes the accumulator with [`tablepro_core::RunSummary`].
    pub fn finish(&mut self, summary: tablepro_core::RunSummary) {
        self.summary = Some(summary);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use proptest::prelude::*;
    use tablepro_core::column::{ColumnType, ResultColumn};
    use tablepro_core::sql_syntax::{SqlGrammar, SyntaxKind};
    use tablepro_core::value::Value;
    use tablepro_core::{ResultEvent, RunEnd, RunSummary, TransactionState, Truncation};

    use super::*;

    #[test]
    fn rope_normalizes_crlf_and_handles_large_text_and_utf16() {
        let big = "a".repeat(5000);
        let mut rope = Rope::from_text(&format!("hello\r\nworld\r{big}"));
        assert_eq!(rope.line_count(), 3);
        assert_eq!(rope.line(0).as_deref(), Some("hello"));
        assert_eq!(rope.line(1).as_deref(), Some("world"));
        assert!(rope.height() >= 2);

        // Surrogate-pair emoji takes 1 char, 2 UTF-16 code units.
        rope.insert(0, "🚀");
        assert_eq!(rope.char_to_utf16(1), 2);
        assert_eq!(rope.utf16_to_char(2), 1);
    }

    #[test]
    fn multi_caret_bottom_up_edits_and_undo_redo_roundtrip() {
        let mut rope = Rope::from_text("aa\nbb\ncc");
        let mut cursors = vec![
            CursorPosition::caret(0),
            CursorPosition::caret(3),
            CursorPosition::caret(6),
        ];
        let mut history = UndoHistory::new();

        history.apply_multi_caret_insert(&mut rope, &mut cursors, "-- ");
        assert_eq!(rope.text(), "-- aa\n-- bb\n-- cc");
        assert_eq!(
            cursors,
            vec![
                CursorPosition::caret(3),
                CursorPosition::caret(9),
                CursorPosition::caret(15)
            ]
        );

        assert!(history.undo(&mut rope, &mut cursors));
        assert_eq!(rope.text(), "aa\nbb\ncc");
        assert_eq!(
            cursors,
            vec![
                CursorPosition::caret(0),
                CursorPosition::caret(3),
                CursorPosition::caret(6)
            ]
        );

        assert!(history.redo(&mut rope, &mut cursors));
        assert_eq!(rope.text(), "-- aa\n-- bb\n-- cc");
    }

    #[test]
    fn incremental_highlighter_emits_utf16_spans() {
        let rope = Rope::from_text("SELECT '🚀' FROM users");
        let mut hl = IncrementalLineHighlighter::new(SqlGrammar::PostgreSql);
        let spans = hl.highlight_line(&rope, 0);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == SyntaxKind::Keyword && s.start_utf16 == 0 && s.end_utf16 == 6)
        );
        // '🚀' is 4 UTF-16 units (' + 2 + ')
        assert!(
            spans
                .iter()
                .any(|s| s.kind == SyntaxKind::String && (s.end_utf16 - s.start_utf16) == 4)
        );
    }

    #[test]
    fn autocomplete_vim_splits_and_run_event_accumulator() {
        let mut ac = AutocompleteState::with_catalog(vec![
            SchemaCompletionItem {
                label: "users".to_owned(),
                kind: SchemaCompletionKind::Table,
                detail: Some("public.users".to_owned()),
                insert_text: "users".to_owned(),
            },
            SchemaCompletionItem {
                label: "user_id".to_owned(),
                kind: SchemaCompletionKind::Column,
                detail: None,
                insert_text: "user_id".to_owned(),
            },
        ]);
        ac.update_prefix("us");
        assert!(ac.is_open);
        assert_eq!(ac.items().len(), 2);
        assert_eq!(ac.accept().map(|i| i.label), Some("users".to_owned()));

        let mut vim = VimState::new(true);
        assert_eq!(vim.handle_key("2"), VimAction::None);
        assert_eq!(
            vim.handle_key("w"),
            VimAction::Move(VimMotion::WordForward, 2)
        );
        assert_eq!(vim.handle_key("d"), VimAction::None);
        assert_eq!(
            vim.handle_key("d"),
            VimAction::LineOperator(VimOperator::Delete, 1)
        );
        assert_eq!(vim.handle_key(":"), VimAction::None);
        assert_eq!(vim.mode, VimMode::Command);
        vim.handle_key("w");
        assert_eq!(
            vim.handle_key("Enter"),
            VimAction::ExecuteCommand("w".to_owned())
        );
        assert_eq!(vim.mode, VimMode::Normal);

        let mut split = EditorSplitNode::single(EditorPaneId(1));
        assert!(split.split_pane(EditorPaneId(1), EditorPaneId(2), SplitAxis::Horizontal));
        assert_eq!(split.leaf_ids(), vec![EditorPaneId(1), EditorPaneId(2)]);
        assert!(SplitPaneLayoutPolicy::default().holding_priority < 490);
        assert!(SplitPaneLayoutPolicy::default().sizing_options_empty);

        let mut acc = RunEventAccumulator::new();
        acc.push_event(ResultEvent::ResultSetStarted {
            columns: Arc::from([ResultColumn::new("id", ColumnType::unknown())]),
        });
        acc.push_event(ResultEvent::Rows(vec![vec![Value::Int(1)]]));
        acc.push_event(ResultEvent::ResultSetFinished {
            retained: 1,
            total_rows: Some(1),
            truncation: Truncation::Complete,
        });
        acc.finish(RunSummary {
            elapsed: Duration::from_millis(12),
            end: RunEnd::Completed,
            transaction_state: TransactionState::Idle,
        });
        assert_eq!(acc.result_sets.len(), 1);
        assert_eq!(acc.result_sets[0].rows.len(), 1);
    }

    proptest! {
        #[test]
        fn prop_rope_matches_string_oracle(
            initial in "[a-z\n]{0,120}",
            ops in prop::collection::vec((0usize..200, "[a-z]{0,20}", 0usize..20), 0..30),
        ) {
            let mut rope = Rope::from_text(&initial);
            let mut oracle = normalize_line_endings(&initial);

            for (pos, ins, del_len) in ops {
                let char_len = oracle.chars().count();
                let idx = pos % (char_len + 1);
                rope.insert(idx, &ins);
                let byte_idx = oracle.char_indices().nth(idx).map_or(oracle.len(), |(b, _)| b);
                oracle.insert_str(byte_idx, &ins);

                let new_char_len = oracle.chars().count();
                if new_char_len > 0 && del_len > 0 {
                    let s = pos % new_char_len;
                    let e = (s + del_len).min(new_char_len);
                    rope.delete(s, e);
                    let bs = oracle.char_indices().nth(s).map_or(oracle.len(), |(b, _)| b);
                    let be = oracle.char_indices().nth(e).map_or(oracle.len(), |(b, _)| b);
                    oracle.replace_range(bs..be, "");
                }

                prop_assert_eq!(rope.text(), oracle.clone());
                prop_assert_eq!(rope.len_chars(), oracle.chars().count());
            }
        }
    }
}
