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
