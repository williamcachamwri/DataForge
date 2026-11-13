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
        Self { chars, utf16, newlines }
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
            Self::Branch { left, right, .. } => i32::from(left.height()) - i32::from(right.height()),
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
                let byte_idx = text.char_indices().nth(char_idx).map_or(text.len(), |(b, _)| b);
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
            Self::Leaf { text: lt, metrics: lm },
            Self::Leaf { text: rt, metrics: rm },
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
        let mut ordered: Vec<(usize, CursorPosition)> = cursor_positions.iter().copied().enumerate().collect();
        ordered.sort_by(|a, b| b.1.range().0.cmp(&a.1.range().0).then_with(|| b.0.cmp(&a.0)));

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
        let mut asc: Vec<(usize, CursorPosition)> = cursor_positions.iter().copied().enumerate().collect();
        asc.sort_by(|a, b| a.1.range().0.cmp(&b.1.range().0).then_with(|| a.0.cmp(&b.0)));
        let mut delta: isize = 0;
        let mut updated = vec![CursorPosition::caret(0); cursor_positions.len()];
        for (orig_idx, cursor) in asc {
            let (start, end) = cursor.range();
            let new_pos = usize::try_from(isize::try_from(start).unwrap_or(0) + delta).unwrap_or(0) + insert_chars;
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
        // Edits were recorded bottom-up; undo them in reverse (top-down) or re-index.
        // Since each edit in `group.edits` was applied bottom-up, reversing the order
        // (top-down) undoes at the exact `edit.start` offsets? Wait: undoing top-down
        // shifts later offsets, whereas undoing in reverse of application?
        // Actually, when `group.edits` was applied bottom-up, `edit[0]` had largest `start`.
        // Undoing `edit` in reverse order (`rev()`, smallest `start` first) would shift larger `start`s!
        // Undoing in the SAME order (`group.edits.iter()`, largest `start` first) restores
        // each region from bottom to top without shifting any earlier offset!
        for edit in &group.edits {
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
pub fn lex_line_utf16(line: &str, grammar: tablepro_core::sql_syntax::SqlGrammar) -> Vec<Utf16SyntaxSpan> {
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
