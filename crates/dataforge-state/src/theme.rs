//! Theme slots, schema versioning, pure resolution, built-in palettes, and VS Code import.
//!
//! Strictly enforces the Content vs Chrome split: themes color content surfaces
//! (`editor.*`, `dataGrid.*`, `window.background`) and never chrome controls.

use std::collections::BTreeMap;
use std::fmt;

/// Minimum schema version readable by this build.
pub const MIN_READABLE_SCHEMA: u32 = 3;

/// Current schema version for theme files.
pub const CURRENT_SCHEMA: u32 = 5;

/// Maximum byte size accepted for a theme file or synced theme payload (64 KiB).
pub const MAX_THEME_FILE_BYTES: usize = 64 * 1024;

/// Maximum JSON nesting depth accepted for untrusted theme files.
pub const MAX_THEME_JSON_DEPTH: usize = 16;

/// All themeable content slots in DataForge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ThemeSlot {
    EditorBackground,
    EditorText,
    EditorSelection,
    EditorCaret,
    EditorCurrentLine,
    EditorGutterBackground,
    EditorGutterText,
    EditorSyntaxKeyword,
    EditorSyntaxString,
    EditorSyntaxComment,
    EditorSyntaxNumber,
    EditorSyntaxOperator,
    EditorSyntaxPunctuation,
    EditorSyntaxIdentifier,
    EditorSyntaxQuotedIdentifier,
    EditorSyntaxNull,
    DataGridHeaderBackground,
    DataGridHeaderText,
    DataGridSelection,
    DataGridInactiveSelection,
    DataGridSelectionText,
    DataGridFocusBorder,
    DataGridInserted,
    DataGridModified,
    DataGridDeleted,
    WindowBackground,
}

impl ThemeSlot {
    /// All defined slots in canonical order.
    pub const ALL: [Self; 26] = [
        Self::EditorBackground,
        Self::EditorText,
        Self::EditorSelection,
        Self::EditorCaret,
        Self::EditorCurrentLine,
        Self::EditorGutterBackground,
        Self::EditorGutterText,
        Self::EditorSyntaxKeyword,
        Self::EditorSyntaxString,
        Self::EditorSyntaxComment,
        Self::EditorSyntaxNumber,
        Self::EditorSyntaxOperator,
        Self::EditorSyntaxPunctuation,
        Self::EditorSyntaxIdentifier,
        Self::EditorSyntaxQuotedIdentifier,
        Self::EditorSyntaxNull,
        Self::DataGridHeaderBackground,
        Self::DataGridHeaderText,
        Self::DataGridSelection,
        Self::DataGridInactiveSelection,
        Self::DataGridSelectionText,
        Self::DataGridFocusBorder,
        Self::DataGridInserted,
        Self::DataGridModified,
        Self::DataGridDeleted,
        Self::WindowBackground,
    ];

    /// Canonical dot-separated key name used in theme JSON files.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::EditorBackground => "editor.background",
            Self::EditorText => "editor.text",
            Self::EditorSelection => "editor.selection",
            Self::EditorCaret => "editor.caret",
            Self::EditorCurrentLine => "editor.currentLine",
            Self::EditorGutterBackground => "editor.gutterBackground",
            Self::EditorGutterText => "editor.gutterText",
            Self::EditorSyntaxKeyword => "editor.syntax.keyword",
            Self::EditorSyntaxString => "editor.syntax.string",
            Self::EditorSyntaxComment => "editor.syntax.comment",
            Self::EditorSyntaxNumber => "editor.syntax.number",
            Self::EditorSyntaxOperator => "editor.syntax.operator",
            Self::EditorSyntaxPunctuation => "editor.syntax.punctuation",
            Self::EditorSyntaxIdentifier => "editor.syntax.identifier",
            Self::EditorSyntaxQuotedIdentifier => "editor.syntax.quotedIdentifier",
            Self::EditorSyntaxNull => "editor.syntax.null",
            Self::DataGridHeaderBackground => "dataGrid.headerBackground",
            Self::DataGridHeaderText => "dataGrid.headerText",
            Self::DataGridSelection => "dataGrid.selection",
            Self::DataGridInactiveSelection => "dataGrid.inactiveSelection",
            Self::DataGridSelectionText => "dataGrid.selectionText",
            Self::DataGridFocusBorder => "dataGrid.focusBorder",
            Self::DataGridInserted => "dataGrid.inserted",
            Self::DataGridModified => "dataGrid.modified",
            Self::DataGridDeleted => "dataGrid.deleted",
            Self::WindowBackground => "window.background",
        }
    }

    /// Parses a canonical slot key.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|slot| slot.key() == key)
    }

    /// The schema version that introduced this slot.
    #[must_use]
    pub const fn schema_introduced_in(self) -> u32 {
        match self {
            Self::EditorBackground
            | Self::EditorText
            | Self::EditorSelection
            | Self::EditorCaret
            | Self::EditorCurrentLine
            | Self::EditorGutterBackground
            | Self::EditorGutterText
            | Self::EditorSyntaxKeyword
            | Self::EditorSyntaxString
            | Self::EditorSyntaxComment
            | Self::EditorSyntaxNumber
            | Self::EditorSyntaxOperator
            | Self::EditorSyntaxPunctuation
            | Self::EditorSyntaxIdentifier
            | Self::EditorSyntaxQuotedIdentifier
            | Self::EditorSyntaxNull
            | Self::WindowBackground => 3,
            Self::DataGridHeaderBackground
            | Self::DataGridHeaderText
            | Self::DataGridSelection
            | Self::DataGridInactiveSelection
            | Self::DataGridSelectionText
            | Self::DataGridFocusBorder => 4,
            Self::DataGridInserted | Self::DataGridModified | Self::DataGridDeleted => 5,
        }
    }
}

impl fmt::Display for ThemeSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// Schema version metadata and compatibility helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeSchema;

impl ThemeSchema {
    /// Returns the oldest schema version that can encode all slots in `slots`.
    #[must_use]
    pub fn required_schema<'a>(slots: impl IntoIterator<Item = &'a ThemeSlot>) -> u32 {
        slots
            .into_iter()
            .map(|slot| slot.schema_introduced_in())
            .max()
            .unwrap_or(MIN_READABLE_SCHEMA)
            .max(MIN_READABLE_SCHEMA)
    }

    /// Validates whether `file_schema` is readable by this build.
    #[must_use]
    pub const fn is_readable(file_schema: u32) -> bool {
        file_schema >= MIN_READABLE_SCHEMA && file_schema <= CURRENT_SCHEMA
    }

    /// Sync content kind identifier tied to `CURRENT_SCHEMA`.
    #[must_use]
    pub fn sync_content_kind() -> String {
        format!("theme.v{CURRENT_SCHEMA}")
    }
}

/// Tracks active readers per `ThemeSlot`.
///
/// Per `theme.md`: a slot exists in the active registry only while at least one
/// view/reader reads it; removing its last reader removes the slot.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ThemeSlotRegistry {
    readers: BTreeMap<ThemeSlot, usize>,
}

impl ThemeSlotRegistry {
    /// Creates an empty slot registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers one reader for `slot` and returns the new reader count.
    pub fn acquire(&mut self, slot: ThemeSlot) -> usize {
        let count = self.readers.entry(slot).or_insert(0);
        *count = count.saturating_add(1);
        *count
    }

    /// Unregisters one reader for `slot`, removing the slot when the count hits 0.
    pub fn release(&mut self, slot: ThemeSlot) {
        if let Some(count) = self.readers.get_mut(&slot) {
            if *count <= 1 {
                self.readers.remove(&slot);
            } else {
                *count -= 1;
            }
        }
    }

    /// Returns whether `slot` currently has at least one active reader.
    #[must_use]
    pub fn contains(&self, slot: ThemeSlot) -> bool {
        self.readers.contains_key(&slot)
    }

    /// Returns the number of active readers for `slot`.
    #[must_use]
    pub fn reader_count(&self, slot: ThemeSlot) -> usize {
        self.readers.get(&slot).copied().unwrap_or(0)
    }

    /// Returns all currently active slots.
    #[must_use]
    pub fn active_slots(&self) -> Vec<ThemeSlot> {
        self.readers.keys().copied().collect()
    }
}
