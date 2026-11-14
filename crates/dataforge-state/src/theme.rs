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

/// RGBA color with 8-bit channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    /// Creates an opaque RGBA color.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Creates an RGBA color with explicit alpha.
    #[must_use]
    pub const fn with_alpha(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Parses `#RGB`, `#RGBA`, `#RRGGBB`, or `#RRGGBBAA` hex strings.
    #[must_use]
    pub fn from_hex(input: &str) -> Option<Self> {
        let hex = input.strip_prefix('#').unwrap_or(input);
        match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
                let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
                let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
                Some(Self::rgb(r, g, b))
            }
            4 => {
                let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
                let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
                let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
                let a = u8::from_str_radix(&hex[3..4], 16).ok()? * 17;
                Some(Self::with_alpha(r, g, b, a))
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Self::rgb(r, g, b))
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                Some(Self::with_alpha(r, g, b, a))
            }
            _ => None,
        }
    }

    /// Formats as `#RRGGBB` when opaque, or `#RRGGBBAA` when translucent.
    #[must_use]
    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
        }
    }
}

/// Appearance category for a theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeAppearance {
    Light,
    Dark,
    HighContrast,
}

/// Stable identifier for a theme.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct ThemeId(pub String);

impl ThemeId {
    /// Built-in Light theme id.
    #[must_use]
    pub fn builtin_light() -> Self {
        Self("builtin.light".to_owned())
    }

    /// Built-in Dark theme id.
    #[must_use]
    pub fn builtin_dark() -> Self {
        Self("builtin.dark".to_owned())
    }

    /// Built-in High-Contrast theme id.
    #[must_use]
    pub fn builtin_high_contrast() -> Self {
        Self("builtin.high_contrast".to_owned())
    }

    /// Returns whether this id refers to a built-in theme.
    #[must_use]
    pub fn is_builtin(&self) -> bool {
        self.0.starts_with("builtin.")
    }
}

/// Complete theme definition (built-in or user-imported/edited).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeDefinition {
    pub id: ThemeId,
    pub name: String,
    pub appearance: ThemeAppearance,
    pub is_builtin: bool,
    pub colors: BTreeMap<ThemeSlot, Rgba>,
}

impl ThemeDefinition {
    /// Built-in Light content palette.
    #[must_use]
    pub fn builtin_light() -> Self {
        let mut colors = BTreeMap::new();
        colors.insert(ThemeSlot::EditorBackground, Rgba::rgb(0xFF, 0xFF, 0xFF));
        colors.insert(ThemeSlot::EditorText, Rgba::rgb(0x1E, 0x1E, 0x1E));
        colors.insert(
            ThemeSlot::EditorSelection,
            Rgba::with_alpha(0xAD, 0xD6, 0xFF, 0xC0),
        );
        colors.insert(ThemeSlot::EditorCaret, Rgba::rgb(0x00, 0x00, 0x00));
        colors.insert(ThemeSlot::EditorCurrentLine, Rgba::rgb(0xF5, 0xF7, 0xFA));
        colors.insert(
            ThemeSlot::EditorGutterBackground,
            Rgba::rgb(0xFF, 0xFF, 0xFF),
        );
        colors.insert(ThemeSlot::EditorGutterText, Rgba::rgb(0x6E, 0x76, 0x81));
        colors.insert(ThemeSlot::EditorSyntaxKeyword, Rgba::rgb(0x00, 0x33, 0xB3));
        colors.insert(ThemeSlot::EditorSyntaxString, Rgba::rgb(0x06, 0x7D, 0x17));
        colors.insert(ThemeSlot::EditorSyntaxComment, Rgba::rgb(0x6A, 0x73, 0x7D));
        colors.insert(ThemeSlot::EditorSyntaxNumber, Rgba::rgb(0x17, 0x50, 0xEB));
        colors.insert(ThemeSlot::EditorSyntaxOperator, Rgba::rgb(0x24, 0x29, 0x2E));
        colors.insert(
            ThemeSlot::EditorSyntaxPunctuation,
            Rgba::rgb(0x4E, 0x56, 0x60),
        );
        colors.insert(
            ThemeSlot::EditorSyntaxIdentifier,
            Rgba::rgb(0x1E, 0x1E, 0x1E),
        );
        colors.insert(
            ThemeSlot::EditorSyntaxQuotedIdentifier,
            Rgba::rgb(0x04, 0x51, 0xA5),
        );
        colors.insert(ThemeSlot::EditorSyntaxNull, Rgba::rgb(0x80, 0x80, 0x80));
        colors.insert(
            ThemeSlot::DataGridHeaderBackground,
            Rgba::rgb(0xF8, 0xF9, 0xFB),
        );
        colors.insert(ThemeSlot::DataGridHeaderText, Rgba::rgb(0x1F, 0x23, 0x28));
        colors.insert(ThemeSlot::DataGridSelection, Rgba::rgb(0x09, 0x69, 0xDA));
        colors.insert(
            ThemeSlot::DataGridInactiveSelection,
            Rgba::rgb(0xD0, 0xD7, 0xDE),
        );
        colors.insert(
            ThemeSlot::DataGridSelectionText,
            Rgba::rgb(0xFF, 0xFF, 0xFF),
        );
        colors.insert(ThemeSlot::DataGridFocusBorder, Rgba::rgb(0x09, 0x69, 0xDA));
        colors.insert(
            ThemeSlot::DataGridInserted,
            Rgba::with_alpha(0x2D, 0xA4, 0x4E, 0x38),
        );
        colors.insert(
            ThemeSlot::DataGridModified,
            Rgba::with_alpha(0xBF, 0x87, 0x00, 0x38),
        );
        colors.insert(
            ThemeSlot::DataGridDeleted,
            Rgba::with_alpha(0xCF, 0x22, 0x2E, 0x38),
        );
        colors.insert(ThemeSlot::WindowBackground, Rgba::rgb(0xFF, 0xFF, 0xFF));
        Self {
            id: ThemeId::builtin_light(),
            name: "DataForge Light".to_owned(),
            appearance: ThemeAppearance::Light,
            is_builtin: true,
            colors,
        }
    }

    /// Built-in Dark content palette.
    #[must_use]
    pub fn builtin_dark() -> Self {
        let mut colors = BTreeMap::new();
        colors.insert(ThemeSlot::EditorBackground, Rgba::rgb(0x1B, 0x1C, 0x20));
        colors.insert(ThemeSlot::EditorText, Rgba::rgb(0xD4, 0xD4, 0xD4));
        colors.insert(
            ThemeSlot::EditorSelection,
            Rgba::with_alpha(0x26, 0x4F, 0x78, 0xCC),
        );
        colors.insert(ThemeSlot::EditorCaret, Rgba::rgb(0xAE, 0xAF, 0xAD));
        colors.insert(ThemeSlot::EditorCurrentLine, Rgba::rgb(0x21, 0x23, 0x29));
        colors.insert(
            ThemeSlot::EditorGutterBackground,
            Rgba::rgb(0x1B, 0x1C, 0x20),
        );
        colors.insert(ThemeSlot::EditorGutterText, Rgba::rgb(0x85, 0x85, 0x85));
        colors.insert(ThemeSlot::EditorSyntaxKeyword, Rgba::rgb(0x56, 0x9C, 0xD6));
        colors.insert(ThemeSlot::EditorSyntaxString, Rgba::rgb(0xCE, 0x91, 0x78));
        colors.insert(ThemeSlot::EditorSyntaxComment, Rgba::rgb(0x6A, 0x99, 0x55));
        colors.insert(ThemeSlot::EditorSyntaxNumber, Rgba::rgb(0xB5, 0xCE, 0xA8));
        colors.insert(ThemeSlot::EditorSyntaxOperator, Rgba::rgb(0xD4, 0xD4, 0xD4));
        colors.insert(
            ThemeSlot::EditorSyntaxPunctuation,
            Rgba::rgb(0xA0, 0xA0, 0xA0),
        );
        colors.insert(
            ThemeSlot::EditorSyntaxIdentifier,
            Rgba::rgb(0xD4, 0xD4, 0xD4),
        );
        colors.insert(
            ThemeSlot::EditorSyntaxQuotedIdentifier,
            Rgba::rgb(0x9C, 0xDC, 0xFE),
        );
        colors.insert(ThemeSlot::EditorSyntaxNull, Rgba::rgb(0x80, 0x80, 0x80));
        colors.insert(
            ThemeSlot::DataGridHeaderBackground,
            Rgba::rgb(0x1F, 0x20, 0x25),
        );
        colors.insert(ThemeSlot::DataGridHeaderText, Rgba::rgb(0xCC, 0xCC, 0xCC));
        colors.insert(ThemeSlot::DataGridSelection, Rgba::rgb(0x09, 0x47, 0x71));
        colors.insert(
            ThemeSlot::DataGridInactiveSelection,
            Rgba::rgb(0x3A, 0x3D, 0x41),
        );
        colors.insert(
            ThemeSlot::DataGridSelectionText,
            Rgba::rgb(0xFF, 0xFF, 0xFF),
        );
        colors.insert(ThemeSlot::DataGridFocusBorder, Rgba::rgb(0x00, 0x7F, 0xD4));
        colors.insert(
            ThemeSlot::DataGridInserted,
            Rgba::with_alpha(0x2E, 0xA0, 0x43, 0x40),
        );
        colors.insert(
            ThemeSlot::DataGridModified,
            Rgba::with_alpha(0xD2, 0x99, 0x22, 0x40),
        );
        colors.insert(
            ThemeSlot::DataGridDeleted,
            Rgba::with_alpha(0xF8, 0x51, 0x49, 0x40),
        );
        colors.insert(ThemeSlot::WindowBackground, Rgba::rgb(0x1B, 0x1C, 0x20));
        Self {
            id: ThemeId::builtin_dark(),
            name: "DataForge Dark".to_owned(),
            appearance: ThemeAppearance::Dark,
            is_builtin: true,
            colors,
        }
    }

    /// Built-in High-Contrast content palette.
    #[must_use]
    pub fn builtin_high_contrast() -> Self {
        let mut colors = BTreeMap::new();
        colors.insert(ThemeSlot::EditorBackground, Rgba::rgb(0x00, 0x00, 0x00));
        colors.insert(ThemeSlot::EditorText, Rgba::rgb(0xFF, 0xFF, 0xFF));
        colors.insert(ThemeSlot::EditorSelection, Rgba::rgb(0xFF, 0xFF, 0x00));
        colors.insert(ThemeSlot::EditorCaret, Rgba::rgb(0xFF, 0xFF, 0xFF));
        colors.insert(ThemeSlot::EditorCurrentLine, Rgba::rgb(0x14, 0x14, 0x14));
        colors.insert(
            ThemeSlot::EditorGutterBackground,
            Rgba::rgb(0x00, 0x00, 0x00),
        );
        colors.insert(ThemeSlot::EditorGutterText, Rgba::rgb(0xFF, 0xFF, 0xFF));
        colors.insert(ThemeSlot::EditorSyntaxKeyword, Rgba::rgb(0x56, 0x9C, 0xD6));
        colors.insert(ThemeSlot::EditorSyntaxString, Rgba::rgb(0xCE, 0x91, 0x78));
        colors.insert(ThemeSlot::EditorSyntaxComment, Rgba::rgb(0x7C, 0xA6, 0x68));
        colors.insert(ThemeSlot::EditorSyntaxNumber, Rgba::rgb(0xB5, 0xCE, 0xA8));
        colors.insert(ThemeSlot::EditorSyntaxOperator, Rgba::rgb(0xFF, 0xFF, 0xFF));
        colors.insert(
            ThemeSlot::EditorSyntaxPunctuation,
            Rgba::rgb(0xFF, 0xFF, 0xFF),
        );
        colors.insert(
            ThemeSlot::EditorSyntaxIdentifier,
            Rgba::rgb(0xFF, 0xFF, 0xFF),
        );
        colors.insert(
            ThemeSlot::EditorSyntaxQuotedIdentifier,
            Rgba::rgb(0x9C, 0xDC, 0xFE),
        );
        colors.insert(ThemeSlot::EditorSyntaxNull, Rgba::rgb(0xD4, 0xD4, 0xD4));
        colors.insert(
            ThemeSlot::DataGridHeaderBackground,
            Rgba::rgb(0x00, 0x00, 0x00),
        );
        colors.insert(ThemeSlot::DataGridHeaderText, Rgba::rgb(0xFF, 0xFF, 0xFF));
        colors.insert(ThemeSlot::DataGridSelection, Rgba::rgb(0x00, 0x78, 0xD7));
        colors.insert(
            ThemeSlot::DataGridInactiveSelection,
            Rgba::rgb(0x3F, 0x3F, 0x46),
        );
        colors.insert(
            ThemeSlot::DataGridSelectionText,
            Rgba::rgb(0xFF, 0xFF, 0xFF),
        );
        colors.insert(ThemeSlot::DataGridFocusBorder, Rgba::rgb(0xF3, 0x85, 0x18));
        colors.insert(
            ThemeSlot::DataGridInserted,
            Rgba::with_alpha(0x1B, 0x87, 0x3F, 0x60),
        );
        colors.insert(
            ThemeSlot::DataGridModified,
            Rgba::with_alpha(0xD2, 0x99, 0x22, 0x60),
        );
        colors.insert(
            ThemeSlot::DataGridDeleted,
            Rgba::with_alpha(0xE5, 0x14, 0x00, 0x60),
        );
        colors.insert(ThemeSlot::WindowBackground, Rgba::rgb(0x00, 0x00, 0x00));
        Self {
            id: ThemeId::builtin_high_contrast(),
            name: "DataForge High Contrast".to_owned(),
            appearance: ThemeAppearance::HighContrast,
            is_builtin: true,
            colors,
        }
    }

    /// Returns the built-in default theme for the given appearance.
    #[must_use]
    pub fn default_for(appearance: ThemeAppearance) -> Self {
        match appearance {
            ThemeAppearance::Light => Self::builtin_light(),
            ThemeAppearance::Dark => Self::builtin_dark(),
            ThemeAppearance::HighContrast => Self::builtin_high_contrast(),
        }
    }

    /// Returns all built-in UI + syntax highlight theme presets.
    #[must_use]
    pub fn builtin_presets() -> Vec<Self> {
        let mut out = vec![
            Self::builtin_dark(),
            Self::builtin_light(),
            Self::builtin_high_contrast(),
        ];
        for spec in BUILTIN_THEME_SPECS {
            out.push(build_theme_from_spec(spec));
        }
        out
    }

    /// Looks up a built-in preset by `id` (e.g. `"builtin.tokyo_night"`).
    #[must_use]
    pub fn from_builtin_id(id: &str) -> Option<Self> {
        match id {
            "builtin.light" => Some(Self::builtin_light()),
            "builtin.dark" => Some(Self::builtin_dark()),
            "builtin.high_contrast" => Some(Self::builtin_high_contrast()),
            _ => BUILTIN_THEME_SPECS
                .iter()
                .find(|s| s.id == id)
                .map(build_theme_from_spec),
        }
    }

    /// Returns a copy of `self` with all `editor.syntax.*` tokens (and caret/selection)
    /// replaced by those from `syntax_source`, preserving UI/chrome surfaces.
    #[must_use]
    pub fn with_syntax_from(&self, syntax_source: &Self) -> Self {
        const SYNTAX_SLOTS: &[ThemeSlot] = &[
            ThemeSlot::EditorSyntaxKeyword,
            ThemeSlot::EditorSyntaxString,
            ThemeSlot::EditorSyntaxComment,
            ThemeSlot::EditorSyntaxNumber,
            ThemeSlot::EditorSyntaxOperator,
            ThemeSlot::EditorSyntaxPunctuation,
            ThemeSlot::EditorSyntaxIdentifier,
            ThemeSlot::EditorSyntaxQuotedIdentifier,
            ThemeSlot::EditorSyntaxNull,
            ThemeSlot::EditorCaret,
        ];
        let mut next = self.clone();
        for &slot in SYNTAX_SLOTS {
            if let Some(&color) = syntax_source.colors.get(&slot) {
                next.colors.insert(slot, color);
            }
        }
        next
    }
}

struct ThemeSpec {
    id: &'static str,
    name: &'static str,
    appearance: ThemeAppearance,
    bg: u32,
    text: u32,
    selection: u32,
    caret: u32,
    current_line: u32,
    gutter_bg: u32,
    gutter_text: u32,
    keyword: u32,
    string: u32,
    comment: u32,
    number: u32,
    operator: u32,
    punctuation: u32,
    identifier: u32,
    quoted_id: u32,
    null_color: u32,
    accent: u32,
}

const fn rgb24(hex: u32) -> Rgba {
    Rgba::rgb(
        ((hex >> 16) & 0xFF) as u8,
        ((hex >> 8) & 0xFF) as u8,
        (hex & 0xFF) as u8,
    )
}

const fn rgba24(hex: u32, alpha: u8) -> Rgba {
    Rgba::with_alpha(
        ((hex >> 16) & 0xFF) as u8,
        ((hex >> 8) & 0xFF) as u8,
        (hex & 0xFF) as u8,
        alpha,
    )
}

const BUILTIN_THEME_SPECS: &[ThemeSpec] = &[
    ThemeSpec {
        id: "builtin.gaussian_dark",
        name: "Gaussian Glass Dark",
        appearance: ThemeAppearance::Dark,
        bg: 0x10141C,
        text: 0xE6EDF7,
        selection: 0x264F78,
        caret: 0x60A5FA,
        current_line: 0x192130,
        gutter_bg: 0x0D1118,
        gutter_text: 0x64748B,
        keyword: 0x7DD3FC,
        string: 0x6EE7B7,
        comment: 0x64748B,
        number: 0xFCD34D,
        operator: 0x38BDF8,
        punctuation: 0x94A3B8,
        identifier: 0xE2E8F0,
        quoted_id: 0xC4B5FD,
        null_color: 0x64748B,
        accent: 0x38BDF8,
    },
    ThemeSpec {
        id: "builtin.gaussian_light",
        name: "Gaussian Glass Light",
        appearance: ThemeAppearance::Light,
        bg: 0xF5F7FB,
        text: 0x1E293B,
        selection: 0xBFDBFE,
        caret: 0x0284C7,
        current_line: 0xEBF1FA,
        gutter_bg: 0xEDF2F8,
        gutter_text: 0x94A3B8,
        keyword: 0x0284C7,
        string: 0x059669,
        comment: 0x94A3B8,
        number: 0xD97706,
        operator: 0x4F46E5,
        punctuation: 0x64748B,
        identifier: 0x1E293B,
        quoted_id: 0x7C3AED,
        null_color: 0x94A3B8,
        accent: 0x0284C7,
    },
    ThemeSpec {
        id: "builtin.tokyo_night",
        name: "Tokyo Night",
        appearance: ThemeAppearance::Dark,
        bg: 0x1A1B26,
        text: 0xC0CAF5,
        selection: 0x33467C,
        caret: 0x7AA2F7,
        current_line: 0x24283B,
        gutter_bg: 0x16161E,
        gutter_text: 0x565F89,
        keyword: 0xBB9AF7,
        string: 0x9ECE6A,
        comment: 0x565F89,
        number: 0xFF9E64,
        operator: 0x89DDFF,
        punctuation: 0x9ABDF5,
        identifier: 0xC0CAF5,
        quoted_id: 0x7DCFFF,
        null_color: 0x565F89,
        accent: 0x7AA2F7,
    },
    ThemeSpec {
        id: "builtin.dracula",
        name: "Dracula",
        appearance: ThemeAppearance::Dark,
        bg: 0x282A36,
        text: 0xF8F8F2,
        selection: 0x44475A,
        caret: 0xFF79C6,
        current_line: 0x313442,
        gutter_bg: 0x21222C,
        gutter_text: 0x6272A4,
        keyword: 0xFF79C6,
        string: 0xF1FA8C,
        comment: 0x6272A4,
        number: 0xBD93F9,
        operator: 0xFF79C6,
        punctuation: 0xF8F8F2,
        identifier: 0x8BE9FD,
        quoted_id: 0x50FA7B,
        null_color: 0x6272A4,
        accent: 0xBD93F9,
    },
    ThemeSpec {
        id: "builtin.catppuccin_mocha",
        name: "Catppuccin Mocha",
        appearance: ThemeAppearance::Dark,
        bg: 0x1E1E2E,
        text: 0xCDD6F4,
        selection: 0x45475A,
        caret: 0xF5E0DC,
        current_line: 0x2A2B3C,
        gutter_bg: 0x181825,
        gutter_text: 0x6C7086,
        keyword: 0xCBA6F7,
        string: 0xA6E3A1,
        comment: 0x6C7086,
        number: 0xFAB387,
        operator: 0x89DCEB,
        punctuation: 0x9399B2,
        identifier: 0x89B4FA,
        quoted_id: 0xF9E2AF,
        null_color: 0x6C7086,
        accent: 0xCBA6F7,
    },
    ThemeSpec {
        id: "builtin.one_dark_pro",
        name: "One Dark Pro",
        appearance: ThemeAppearance::Dark,
        bg: 0x282C34,
        text: 0xABB2BF,
        selection: 0x3E4451,
        caret: 0x528BFF,
        current_line: 0x2C313C,
        gutter_bg: 0x21252B,
        gutter_text: 0x5C6370,
        keyword: 0xC678DD,
        string: 0x98C379,
        comment: 0x5C6370,
        number: 0xD19A66,
        operator: 0x56B6C2,
        punctuation: 0xABB2BF,
        identifier: 0xE06C75,
        quoted_id: 0xE5C07B,
        null_color: 0x5C6370,
        accent: 0x528BFF,
    },
    ThemeSpec {
        id: "builtin.nord",
        name: "Nord",
        appearance: ThemeAppearance::Dark,
        bg: 0x2E3440,
        text: 0xD8DEE9,
        selection: 0x434C5E,
        caret: 0x88C0D0,
        current_line: 0x3B4252,
        gutter_bg: 0x272C36,
        gutter_text: 0x616E88,
        keyword: 0x81A1C1,
        string: 0xA3BE8C,
        comment: 0x616E88,
        number: 0xB48EAD,
        operator: 0x81A1C1,
        punctuation: 0xECEFF4,
        identifier: 0x88C0D0,
        quoted_id: 0xEBCB8B,
        null_color: 0x616E88,
        accent: 0x88C0D0,
    },
    ThemeSpec {
        id: "builtin.monokai_pro",
        name: "Monokai Pro",
        appearance: ThemeAppearance::Dark,
        bg: 0x2D2A2E,
        text: 0xFCFCFA,
        selection: 0x5B595C,
        caret: 0xFFD866,
        current_line: 0x383539,
        gutter_bg: 0x221F22,
        gutter_text: 0x727072,
        keyword: 0xFF6188,
        string: 0xFFD866,
        comment: 0x727072,
        number: 0xAB9DF2,
        operator: 0xFF6188,
        punctuation: 0x939293,
        identifier: 0x78DCE8,
        quoted_id: 0xA9DC76,
        null_color: 0x727072,
        accent: 0xFFD866,
    },
    ThemeSpec {
        id: "builtin.gruvbox_dark",
        name: "Gruvbox Dark",
        appearance: ThemeAppearance::Dark,
        bg: 0x282828,
        text: 0xEBDBB2,
        selection: 0x504945,
        caret: 0xFE8019,
        current_line: 0x32302F,
        gutter_bg: 0x1D2021,
        gutter_text: 0x928374,
        keyword: 0xFB4934,
        string: 0xB8BB26,
        comment: 0x928374,
        number: 0xD3869B,
        operator: 0xFE8019,
        punctuation: 0xA89984,
        identifier: 0x83A598,
        quoted_id: 0xFABD2F,
        null_color: 0x928374,
        accent: 0xFE8019,
    },
    ThemeSpec {
        id: "builtin.github_light",
        name: "GitHub Light",
        appearance: ThemeAppearance::Light,
        bg: 0xFFFFFF,
        text: 0x24292F,
        selection: 0xDDF4FF,
        caret: 0x0969DA,
        current_line: 0xF6F8FA,
        gutter_bg: 0xF6F8FA,
        gutter_text: 0x8C959F,
        keyword: 0xCF222E,
        string: 0x0A3069,
        comment: 0x6E7781,
        number: 0x0550AE,
        operator: 0x0550AE,
        punctuation: 0x24292F,
        identifier: 0x953800,
        quoted_id: 0x8250DF,
        null_color: 0x6E7781,
        accent: 0x0969DA,
    },
    ThemeSpec {
        id: "builtin.catppuccin_latte",
        name: "Catppuccin Latte",
        appearance: ThemeAppearance::Light,
        bg: 0xEFF1F5,
        text: 0x4C4F69,
        selection: 0xCCD0DA,
        caret: 0x1E66F5,
        current_line: 0xE6E9EF,
        gutter_bg: 0xE6E9EF,
        gutter_text: 0x8C8FA1,
        keyword: 0x8839EF,
        string: 0x40A02B,
        comment: 0x8C8FA1,
        number: 0xFE640B,
        operator: 0x04A5E5,
        punctuation: 0x6C6F85,
        identifier: 0x1E66F5,
        quoted_id: 0xDF8E1D,
        null_color: 0x8C8FA1,
        accent: 0x1E66F5,
    },
];

fn build_theme_from_spec(spec: &ThemeSpec) -> ThemeDefinition {
    let mut colors = BTreeMap::new();
    let is_light = matches!(spec.appearance, ThemeAppearance::Light);
    colors.insert(ThemeSlot::EditorBackground, rgb24(spec.bg));
    colors.insert(ThemeSlot::EditorText, rgb24(spec.text));
    colors.insert(ThemeSlot::EditorSelection, rgb24(spec.selection));
    colors.insert(ThemeSlot::EditorCaret, rgb24(spec.caret));
    colors.insert(ThemeSlot::EditorCurrentLine, rgb24(spec.current_line));
    colors.insert(ThemeSlot::EditorGutterBackground, rgb24(spec.gutter_bg));
    colors.insert(ThemeSlot::EditorGutterText, rgb24(spec.gutter_text));
    colors.insert(ThemeSlot::EditorSyntaxKeyword, rgb24(spec.keyword));
    colors.insert(ThemeSlot::EditorSyntaxString, rgb24(spec.string));
    colors.insert(ThemeSlot::EditorSyntaxComment, rgb24(spec.comment));
    colors.insert(ThemeSlot::EditorSyntaxNumber, rgb24(spec.number));
    colors.insert(ThemeSlot::EditorSyntaxOperator, rgb24(spec.operator));
    colors.insert(ThemeSlot::EditorSyntaxPunctuation, rgb24(spec.punctuation));
    colors.insert(ThemeSlot::EditorSyntaxIdentifier, rgb24(spec.identifier));
    colors.insert(
        ThemeSlot::EditorSyntaxQuotedIdentifier,
        rgb24(spec.quoted_id),
    );
    colors.insert(ThemeSlot::EditorSyntaxNull, rgb24(spec.null_color));
    colors.insert(ThemeSlot::DataGridHeaderBackground, rgb24(spec.gutter_bg));
    colors.insert(ThemeSlot::DataGridHeaderText, rgb24(spec.text));
    colors.insert(ThemeSlot::DataGridSelection, rgb24(spec.accent));
    colors.insert(
        ThemeSlot::DataGridInactiveSelection,
        rgb24(spec.selection),
    );
    colors.insert(
        ThemeSlot::DataGridSelectionText,
        if is_light {
            Rgba::rgb(0xFF, 0xFF, 0xFF)
        } else {
            rgb24(spec.text)
        },
    );
    colors.insert(ThemeSlot::DataGridFocusBorder, rgb24(spec.accent));
    colors.insert(ThemeSlot::DataGridInserted, rgba24(0x2EA043, 0x38));
    colors.insert(ThemeSlot::DataGridModified, rgba24(0xD29922, 0x38));
    colors.insert(ThemeSlot::DataGridDeleted, rgba24(0xF85149, 0x38));
    colors.insert(ThemeSlot::WindowBackground, rgb24(spec.gutter_bg));
    ThemeDefinition {
        id: ThemeId(spec.id.to_owned()),
        name: spec.name.to_owned(),
        appearance: spec.appearance,
        is_builtin: true,
        colors,
    }
}

/// Scope from which a view resolves its theme colors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeScope {
    /// Follows the active catalog selection for the current appearance.
    Live,
    /// Pinned to a specific theme id (used by theme editor previews).
    Fixed(ThemeId),
}

/// Chrome backdrop state enforcing the Content vs Chrome split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeBackdropState {
    pub is_themed: bool,
    pub can_reach_chrome: bool,
    pub reduce_transparency: bool,
    pub increase_contrast: bool,
}

impl ThemeBackdropState {
    /// Returns the `window.background` fill for a glass sidebar/inspector pane
    /// only when Reduce Transparency is on, `can_reach_chrome` is true,
    /// `increase_contrast` is off, and the resolved theme sets `window.background`.
    #[must_use]
    pub fn chrome_reduce_transparency_fill(self, resolved: &ResolvedTheme) -> Option<Rgba> {
        if !self.is_themed
            || !self.can_reach_chrome
            || !self.reduce_transparency
            || self.increase_contrast
        {
            return None;
        }
        resolved.explicit_color(ThemeSlot::WindowBackground)
    }

    /// Chrome controls never read content slots; they only ask whether the window is themed.
    #[must_use]
    pub const fn chrome_uses_system_style(self) -> bool {
        true
    }
}

/// Result of resolving a theme for a given appearance and scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTheme {
    /// The id that was requested/saved (preserved even when falling back).
    pub saved_id: Option<ThemeId>,
    /// The id of the theme actually supplying colors.
    pub effective_id: ThemeId,
    pub appearance: ThemeAppearance,
    /// True when the requested theme was missing, rejected, or wrong appearance.
    pub used_fallback: bool,
    explicit_colors: BTreeMap<ThemeSlot, Rgba>,
    palette: BTreeMap<ThemeSlot, Rgba>,
}

impl ResolvedTheme {
    /// Returns the color for `slot`, or `fallback` if the slot is unset.
    #[must_use]
    pub fn color_or_fallback(&self, slot: ThemeSlot, fallback: Rgba) -> Rgba {
        self.palette.get(&slot).copied().unwrap_or(fallback)
    }

    /// Returns whether `slot` draws a theme color (explicitly set or derived).
    #[must_use]
    pub fn is_overridden(&self, slot: ThemeSlot) -> bool {
        self.palette.contains_key(&slot)
    }

    /// Returns whether `slot` has an explicit color in the theme definition
    /// (used only by the theme editor).
    #[must_use]
    pub fn has_explicit_color(&self, slot: ThemeSlot) -> bool {
        self.explicit_colors.contains_key(&slot)
    }

    /// Returns the explicit color for `slot`, if any.
    #[must_use]
    pub fn explicit_color(&self, slot: ThemeSlot) -> Option<Rgba> {
        self.explicit_colors.get(&slot).copied()
    }
}

/// Pure theme resolver.
#[derive(Debug, Clone, Copy, Default)]
pub struct ThemeResolver;

impl ThemeResolver {
    /// Resolves `scope` against `catalog` for `appearance`.
    ///
    /// A missing, rejected, or wrong-appearance theme draws the built-in default
    /// for `appearance` while preserving `saved_id` so the theme returns with its file.
    #[must_use]
    pub fn resolve(
        saved_id: Option<&ThemeId>,
        appearance: ThemeAppearance,
        scope: &ThemeScope,
        catalog: &BTreeMap<ThemeId, ThemeDefinition>,
    ) -> ResolvedTheme {
        let requested_id = match scope {
            ThemeScope::Live => saved_id,
            ThemeScope::Fixed(id) => Some(id),
        };
        let default_theme = ThemeDefinition::default_for(appearance);

        let Some(req_id) = requested_id else {
            return Self::from_definition(None, &default_theme, false);
        };

        if let Some(found) = catalog.get(req_id)
            && found.appearance == appearance
        {
            return Self::from_definition(Some(req_id.clone()), found, false);
        }

        if req_id == &default_theme.id {
            return Self::from_definition(Some(req_id.clone()), &default_theme, false);
        }

        // Missing or wrong-appearance: fall back to default while keeping `saved_id`.
        Self::from_definition(Some(req_id.clone()), &default_theme, true)
    }

    fn from_definition(
        saved_id: Option<ThemeId>,
        def: &ThemeDefinition,
        used_fallback: bool,
    ) -> ResolvedTheme {
        let mut palette = def.colors.clone();
        // Derive `editor.syntax.null` from `editor.text` at ~50% alpha if not explicitly set.
        if !palette.contains_key(&ThemeSlot::EditorSyntaxNull)
            && let Some(text) = palette.get(&ThemeSlot::EditorText).copied()
        {
            palette.insert(
                ThemeSlot::EditorSyntaxNull,
                Rgba::with_alpha(text.r, text.g, text.b, text.a / 2),
            );
        }
        ResolvedTheme {
            saved_id,
            effective_id: def.id.clone(),
            appearance: def.appearance,
            used_fallback,
            explicit_colors: def.colors.clone(),
            palette,
        }
    }
}

/// Errors raised when reading, writing, or importing a theme file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ThemeError {
    #[error("theme payload exceeds {MAX_THEME_FILE_BYTES} bytes ({size} bytes)")]
    FileTooLarge { size: usize },

    #[error("theme JSON exceeds maximum nesting depth of {MAX_THEME_JSON_DEPTH}")]
    NestingTooDeep,

    #[error("invalid JSON: {0}")]
    InvalidJson(String),

    #[error(
        "unsupported theme schema version {schema} (supported: {MIN_READABLE_SCHEMA}..={CURRENT_SCHEMA})"
    )]
    UnsupportedSchema { schema: u32 },

    #[error("unknown theme slot key '{0}'")]
    UnknownSlotKey(String),

    #[error("slot '{key}' requires schema {required}, but file declares schema {file_schema}")]
    SlotNewerThanSchema {
        key: String,
        required: u32,
        file_schema: u32,
    },

    #[error("invalid color hex '{value}' for slot '{key}'")]
    InvalidColor { key: String, value: String },

    #[error("theme id must not be empty")]
    EmptyId,

    #[error("theme name must not be empty")]
    EmptyName,

    #[error("remote theme cannot overwrite built-in theme id '{0}'")]
    CannotOverwriteBuiltin(String),

    #[error("remote theme category id '{expected}' does not match file id '{actual}'")]
    IdMismatch { expected: String, actual: String },

    #[error("VS Code theme include '{0}' escapes the theme directory")]
    IncludeEscapesDirectory(String),

    #[error("VS Code theme IO error: {0}")]
    Io(String),
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ThemeFilePayload {
    schema: u32,
    id: String,
    name: String,
    appearance: ThemeAppearance,
    colors: BTreeMap<String, String>,
}

/// Checks JSON nesting depth before full deserialization.
fn check_json_depth(value: &serde_json::Value, depth: usize) -> Result<(), ThemeError> {
    if depth > MAX_THEME_JSON_DEPTH {
        return Err(ThemeError::NestingTooDeep);
    }
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                check_json_depth(item, depth + 1)?;
            }
        }
        serde_json::Value::Object(map) => {
            for val in map.values() {
                check_json_depth(val, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Serializes and deserializes theme files with strict schema validation.
#[derive(Debug, Clone, Copy, Default)]
pub struct ThemeFileFormat;

impl ThemeFileFormat {
    /// Encodes `theme` using the oldest schema version that holds all of its keys.
    ///
    /// # Errors
    /// Returns [`ThemeError::EmptyId`] or [`ThemeError::EmptyName`] if required metadata is blank.
    pub fn encode(theme: &ThemeDefinition) -> Result<Vec<u8>, ThemeError> {
        if theme.id.0.trim().is_empty() {
            return Err(ThemeError::EmptyId);
        }
        if theme.name.trim().is_empty() {
            return Err(ThemeError::EmptyName);
        }
        let schema = ThemeSchema::required_schema(theme.colors.keys());
        let colors = theme
            .colors
            .iter()
            .map(|(slot, rgba)| (slot.key().to_owned(), rgba.to_hex()))
            .collect();
        let payload = ThemeFilePayload {
            schema,
            id: theme.id.0.clone(),
            name: theme.name.clone(),
            appearance: theme.appearance,
            colors,
        };
        let bytes = serde_json::to_vec_pretty(&payload)
            .map_err(|e| ThemeError::InvalidJson(e.to_string()))?;
        if bytes.len() > MAX_THEME_FILE_BYTES {
            return Err(ThemeError::FileTooLarge { size: bytes.len() });
        }
        Ok(bytes)
    }

    /// Decodes and validates an untrusted theme file payload.
    ///
    /// # Errors
    /// Refuses files exceeding 64 KiB, exceeding depth limits, declaring an unsupported
    /// schema, containing unknown keys, or containing keys newer than the file's schema.
    pub fn decode(bytes: &[u8]) -> Result<ThemeDefinition, ThemeError> {
        if bytes.len() > MAX_THEME_FILE_BYTES {
            return Err(ThemeError::FileTooLarge { size: bytes.len() });
        }
        let raw: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| ThemeError::InvalidJson(e.to_string()))?;
        check_json_depth(&raw, 1)?;

        let payload: ThemeFilePayload =
            serde_json::from_value(raw).map_err(|e| ThemeError::InvalidJson(e.to_string()))?;
        if !ThemeSchema::is_readable(payload.schema) {
            return Err(ThemeError::UnsupportedSchema {
                schema: payload.schema,
            });
        }
        if payload.id.trim().is_empty() {
            return Err(ThemeError::EmptyId);
        }
        if payload.name.trim().is_empty() {
            return Err(ThemeError::EmptyName);
        }

        let mut colors = BTreeMap::new();
        for (key, hex) in payload.colors {
            let slot =
                ThemeSlot::from_key(&key).ok_or_else(|| ThemeError::UnknownSlotKey(key.clone()))?;
            let required = slot.schema_introduced_in();
            if required > payload.schema {
                return Err(ThemeError::SlotNewerThanSchema {
                    key,
                    required,
                    file_schema: payload.schema,
                });
            }
            let rgba = Rgba::from_hex(&hex).ok_or(ThemeError::InvalidColor { key, value: hex })?;
            colors.insert(slot, rgba);
        }

        Ok(ThemeDefinition {
            id: ThemeId(payload.id),
            name: payload.name,
            appearance: payload.appearance,
            is_builtin: false,
            colors,
        })
    }

    /// Validates a synced remote theme payload for `category_id`.
    ///
    /// # Errors
    /// Rejects built-in ids, mismatched ids, or invalid theme payloads.
    pub fn validate_remote(category_id: &str, bytes: &[u8]) -> Result<ThemeDefinition, ThemeError> {
        let id = ThemeId(category_id.to_owned());
        if id.is_builtin() {
            return Err(ThemeError::CannotOverwriteBuiltin(category_id.to_owned()));
        }
        let theme = Self::decode(bytes)?;
        if theme.id.0 != category_id {
            return Err(ThemeError::IdMismatch {
                expected: category_id.to_owned(),
                actual: theme.id.0,
            });
        }
        Ok(theme)
    }
}

/// Imports VS Code JSON color themes into DataForge `ThemeDefinition`s.
///
/// Untrusted input rules:
/// - Checks size before reading (`<= MAX_THEME_FILE_BYTES`).
/// - Mints a fresh UUID `ThemeId` on every import.
/// - Never follows an `include` outside the theme file's folder.
#[derive(Debug, Clone, Copy, Default)]
pub struct VsCodeThemeImporter;

impl VsCodeThemeImporter {
    /// Imports a VS Code theme JSON file from `path`, following same-folder `include`s
    /// up to a bounded depth.
    ///
    /// # Errors
    /// Returns [`ThemeError`] on size limit violation, directory escape, or invalid JSON.
    pub fn import_file(path: &std::path::Path) -> Result<ThemeDefinition, ThemeError> {
        let base_dir = path
            .parent()
            .ok_or_else(|| ThemeError::Io("theme path has no parent directory".to_owned()))?;
        let canonical_base = base_dir
            .canonicalize()
            .map_err(|e| ThemeError::Io(e.to_string()))?;
        Self::import_file_inner(path, &canonical_base, 0)
    }

    fn import_file_inner(
        path: &std::path::Path,
        canonical_base: &std::path::Path,
        depth: usize,
    ) -> Result<ThemeDefinition, ThemeError> {
        if depth > 4 {
            return Err(ThemeError::NestingTooDeep);
        }
        let meta = std::fs::metadata(path).map_err(|e| ThemeError::Io(e.to_string()))?;
        let size = usize::try_from(meta.len()).unwrap_or(usize::MAX);
        if size > MAX_THEME_FILE_BYTES {
            return Err(ThemeError::FileTooLarge { size });
        }
        let bytes = std::fs::read(path).map_err(|e| ThemeError::Io(e.to_string()))?;
        if bytes.len() > MAX_THEME_FILE_BYTES {
            return Err(ThemeError::FileTooLarge { size: bytes.len() });
        }

        let raw: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| ThemeError::InvalidJson(e.to_string()))?;
        check_json_depth(&raw, 1)?;

        let mut merged =
            if let Some(include_rel) = raw.get("include").and_then(serde_json::Value::as_str) {
                let include_path = Self::resolve_safe_include(canonical_base, include_rel)?;
                Self::import_file_inner(&include_path, canonical_base, depth + 1)?
            } else {
                let appearance = Self::detect_appearance(&raw);
                let name = raw
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("Imported VS Code Theme")
                    .to_owned();
                ThemeDefinition {
                    id: ThemeId(uuid::Uuid::new_v4().to_string()),
                    name,
                    appearance,
                    is_builtin: false,
                    colors: BTreeMap::new(),
                }
            };

        // Always mint a fresh id for the top-level import.
        merged.id = ThemeId(uuid::Uuid::new_v4().to_string());
        if let Some(name) = raw
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            merged.name = name.to_owned();
        }
        if raw.get("type").is_some() {
            merged.appearance = Self::detect_appearance(&raw);
        }

        Self::apply_vscode_colors(&raw, &mut merged.colors)?;
        Self::apply_token_colors(&raw, &mut merged.colors)?;
        Ok(merged)
    }

    /// Imports from an in-memory VS Code theme JSON slice (no filesystem `include`s).
    ///
    /// # Errors
    /// Returns [`ThemeError`] if size/depth/JSON/color validation fails, or if an `include`
    /// is present without a directory context.
    pub fn import_bytes(bytes: &[u8]) -> Result<ThemeDefinition, ThemeError> {
        if bytes.len() > MAX_THEME_FILE_BYTES {
            return Err(ThemeError::FileTooLarge { size: bytes.len() });
        }
        let raw: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| ThemeError::InvalidJson(e.to_string()))?;
        check_json_depth(&raw, 1)?;
        if let Some(inc) = raw.get("include").and_then(serde_json::Value::as_str) {
            return Err(ThemeError::IncludeEscapesDirectory(inc.to_owned()));
        }
        let appearance = Self::detect_appearance(&raw);
        let name = raw
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Imported VS Code Theme")
            .to_owned();
        let mut colors = BTreeMap::new();
        Self::apply_vscode_colors(&raw, &mut colors)?;
        Self::apply_token_colors(&raw, &mut colors)?;
        Ok(ThemeDefinition {
            id: ThemeId(uuid::Uuid::new_v4().to_string()),
            name,
            appearance,
            is_builtin: false,
            colors,
        })
    }

    fn resolve_safe_include(
        canonical_base: &std::path::Path,
        include_rel: &str,
    ) -> Result<std::path::PathBuf, ThemeError> {
        let rel = std::path::Path::new(include_rel);
        if rel.is_absolute() {
            return Err(ThemeError::IncludeEscapesDirectory(include_rel.to_owned()));
        }
        for comp in rel.components() {
            if matches!(
                comp,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            ) {
                return Err(ThemeError::IncludeEscapesDirectory(include_rel.to_owned()));
            }
        }
        let candidate = canonical_base.join(rel);
        let canonical = candidate
            .canonicalize()
            .map_err(|e| ThemeError::Io(e.to_string()))?;
        if !canonical.starts_with(canonical_base) {
            return Err(ThemeError::IncludeEscapesDirectory(include_rel.to_owned()));
        }
        Ok(canonical)
    }

    fn detect_appearance(raw: &serde_json::Value) -> ThemeAppearance {
        match raw.get("type").and_then(serde_json::Value::as_str) {
            Some("light") => ThemeAppearance::Light,
            Some("hc" | "hcBlack" | "hcLight") => ThemeAppearance::HighContrast,
            _ => ThemeAppearance::Dark,
        }
    }

    fn apply_vscode_colors(
        raw: &serde_json::Value,
        out: &mut BTreeMap<ThemeSlot, Rgba>,
    ) -> Result<(), ThemeError> {
        let Some(serde_json::Value::Object(colors_obj)) = raw.get("colors") else {
            return Ok(());
        };
        for (key, val) in colors_obj {
            let Some(hex) = val.as_str() else {
                continue;
            };
            if let Some(slot) = Self::map_vscode_color_key(key) {
                let rgba = Rgba::from_hex(hex).ok_or_else(|| ThemeError::InvalidColor {
                    key: key.clone(),
                    value: hex.to_owned(),
                })?;
                out.insert(slot, rgba);
            }
        }
        Ok(())
    }

    fn map_vscode_color_key(key: &str) -> Option<ThemeSlot> {
        if let Some(direct) = ThemeSlot::from_key(key) {
            return Some(direct);
        }
        match key {
            "editor.foreground" => Some(ThemeSlot::EditorText),
            "editor.selectionBackground" => Some(ThemeSlot::EditorSelection),
            "editorCursor.foreground" => Some(ThemeSlot::EditorCaret),
            "editor.lineHighlightBackground" => Some(ThemeSlot::EditorCurrentLine),
            "editorGutter.background" => Some(ThemeSlot::EditorGutterBackground),
            "editorLineNumber.foreground" => Some(ThemeSlot::EditorGutterText),
            "sideBarSectionHeader.background" => Some(ThemeSlot::DataGridHeaderBackground),
            "sideBarSectionHeader.foreground" => Some(ThemeSlot::DataGridHeaderText),
            "list.activeSelectionBackground" => Some(ThemeSlot::DataGridSelection),
            "list.inactiveSelectionBackground" => Some(ThemeSlot::DataGridInactiveSelection),
            "list.activeSelectionForeground" => Some(ThemeSlot::DataGridSelectionText),
            "focusBorder" => Some(ThemeSlot::DataGridFocusBorder),
            "diffEditor.insertedLineBackground" => Some(ThemeSlot::DataGridInserted),
            "diffEditor.modifiedTextBackground" => Some(ThemeSlot::DataGridModified),
            "diffEditor.removedLineBackground" => Some(ThemeSlot::DataGridDeleted),
            _ => None,
        }
    }

    fn apply_token_colors(
        raw: &serde_json::Value,
        out: &mut BTreeMap<ThemeSlot, Rgba>,
    ) -> Result<(), ThemeError> {
        let Some(rules) = raw.get("tokenColors").and_then(serde_json::Value::as_array) else {
            return Ok(());
        };
        for rule in rules {
            let Some(hex) = rule
                .get("settings")
                .and_then(|s| s.get("foreground"))
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let rgba = Rgba::from_hex(hex).ok_or_else(|| ThemeError::InvalidColor {
                key: "tokenColors.settings.foreground".to_owned(),
                value: hex.to_owned(),
            })?;
            for scope in Self::extract_scopes(rule.get("scope")) {
                if let Some(slot) = Self::map_token_scope(scope.trim()) {
                    out.insert(slot, rgba);
                }
            }
        }
        Ok(())
    }

    fn extract_scopes(scope_val: Option<&serde_json::Value>) -> Vec<&str> {
        match scope_val {
            Some(serde_json::Value::String(s)) => s.split(',').collect(),
            Some(serde_json::Value::Array(arr)) => {
                arr.iter().filter_map(serde_json::Value::as_str).collect()
            }
            _ => Vec::new(),
        }
    }

    fn map_token_scope(scope: &str) -> Option<ThemeSlot> {
        match scope {
            "keyword" | "keyword.control" | "storage.type" => Some(ThemeSlot::EditorSyntaxKeyword),
            "string" | "string.quoted" => Some(ThemeSlot::EditorSyntaxString),
            "comment" | "comment.line" | "comment.block" => Some(ThemeSlot::EditorSyntaxComment),
            "constant.numeric" => Some(ThemeSlot::EditorSyntaxNumber),
            "keyword.operator" => Some(ThemeSlot::EditorSyntaxOperator),
            "punctuation" => Some(ThemeSlot::EditorSyntaxPunctuation),
            "variable" | "entity.name" => Some(ThemeSlot::EditorSyntaxIdentifier),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn slot_registry_removes_slot_when_last_reader_releases() {
        let mut reg = ThemeSlotRegistry::new();
        assert_eq!(reg.acquire(ThemeSlot::EditorBackground), 1);
        assert_eq!(reg.acquire(ThemeSlot::EditorBackground), 2);
        assert!(reg.contains(ThemeSlot::EditorBackground));

        reg.release(ThemeSlot::EditorBackground);
        assert_eq!(reg.reader_count(ThemeSlot::EditorBackground), 1);
        assert!(reg.contains(ThemeSlot::EditorBackground));

        reg.release(ThemeSlot::EditorBackground);
        assert_eq!(reg.reader_count(ThemeSlot::EditorBackground), 0);
        assert!(!reg.contains(ThemeSlot::EditorBackground));
    }

    #[test]
    fn schema_encoding_picks_oldest_compatible_schema() -> Result<(), Box<dyn std::error::Error>> {
        let mut colors = BTreeMap::new();
        colors.insert(ThemeSlot::EditorBackground, Rgba::rgb(0x10, 0x20, 0x30));
        let v3_theme = ThemeDefinition {
            id: ThemeId("custom-v3".to_owned()),
            name: "Custom V3".to_owned(),
            appearance: ThemeAppearance::Dark,
            is_builtin: false,
            colors,
        };
        let encoded = ThemeFileFormat::encode(&v3_theme)?;
        let raw: serde_json::Value = serde_json::from_slice(&encoded)?;
        assert_eq!(
            raw.get("schema").and_then(serde_json::Value::as_u64),
            Some(3)
        );

        let decoded = ThemeFileFormat::decode(&encoded)?;
        assert_eq!(decoded, v3_theme);
        Ok(())
    }

    #[test]
    fn decode_refuses_newer_schema_unknown_keys_and_future_keys() {
        let future_schema = br##"{
            "schema": 99,
            "id": "future",
            "name": "Future",
            "appearance": "dark",
            "colors": { "editor.background": "#000000" }
        }"##;
        assert!(matches!(
            ThemeFileFormat::decode(future_schema),
            Err(ThemeError::UnsupportedSchema { schema: 99 })
        ));

        let unknown_key = br##"{
            "schema": 5,
            "id": "u",
            "name": "U",
            "appearance": "dark",
            "colors": { "chrome.toolbar": "#123456" }
        }"##;
        assert!(matches!(
            ThemeFileFormat::decode(unknown_key),
            Err(ThemeError::UnknownSlotKey(_))
        ));

        let future_key_in_v3 = br##"{
            "schema": 3,
            "id": "mismatch",
            "name": "Mismatch",
            "appearance": "dark",
            "colors": { "dataGrid.inserted": "#00FF00" }
        }"##;
        assert!(matches!(
            ThemeFileFormat::decode(future_key_in_v3),
            Err(ThemeError::SlotNewerThanSchema {
                required: 5,
                file_schema: 3,
                ..
            })
        ));
    }

    #[test]
    fn resolver_preserves_saved_id_on_missing_or_wrong_appearance() {
        let mut catalog = BTreeMap::new();
        let dark_custom = ThemeDefinition {
            id: ThemeId("my-dark".to_owned()),
            name: "My Dark".to_owned(),
            appearance: ThemeAppearance::Dark,
            is_builtin: false,
            colors: BTreeMap::from([(ThemeSlot::EditorText, Rgba::rgb(0xEE, 0xEE, 0xEE))]),
        };
        catalog.insert(dark_custom.id.clone(), dark_custom.clone());

        // Missing id -> falls back to default, preserves saved_id.
        let missing = ThemeResolver::resolve(
            Some(&ThemeId("missing-id".to_owned())),
            ThemeAppearance::Dark,
            &ThemeScope::Live,
            &catalog,
        );
        assert!(missing.used_fallback);
        assert_eq!(missing.saved_id, Some(ThemeId("missing-id".to_owned())));
        assert_eq!(missing.effective_id, ThemeId::builtin_dark());

        // Wrong appearance -> falls back to Light default, preserves saved_id.
        let wrong_app = ThemeResolver::resolve(
            Some(&dark_custom.id),
            ThemeAppearance::Light,
            &ThemeScope::Live,
            &catalog,
        );
        assert!(wrong_app.used_fallback);
        assert_eq!(wrong_app.saved_id, Some(dark_custom.id));
        assert_eq!(wrong_app.effective_id, ThemeId::builtin_light());
    }

    #[test]
    fn chrome_reduce_transparency_only_uses_window_background_when_allowed() {
        let mut catalog = BTreeMap::new();
        let bg = Rgba::rgb(0x22, 0x24, 0x26);
        let custom = ThemeDefinition {
            id: ThemeId("win-bg".to_owned()),
            name: "Win BG".to_owned(),
            appearance: ThemeAppearance::Dark,
            is_builtin: false,
            colors: BTreeMap::from([(ThemeSlot::WindowBackground, bg)]),
        };
        catalog.insert(custom.id.clone(), custom.clone());
        let resolved = ThemeResolver::resolve(
            Some(&custom.id),
            ThemeAppearance::Dark,
            &ThemeScope::Live,
            &catalog,
        );

        let allowed = ThemeBackdropState {
            is_themed: true,
            can_reach_chrome: true,
            reduce_transparency: true,
            increase_contrast: false,
        };
        assert_eq!(allowed.chrome_reduce_transparency_fill(&resolved), Some(bg));

        let high_contrast_system = ThemeBackdropState {
            increase_contrast: true,
            ..allowed
        };
        assert_eq!(
            high_contrast_system.chrome_reduce_transparency_fill(&resolved),
            None
        );
    }

    #[test]
    fn vscode_importer_rejects_parent_escape_and_mints_fresh_uuid()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let escape_file = dir.path().join("theme.json");
        std::fs::write(
            &escape_file,
            br#"{"name":"Bad","include":"../secret.json","colors":{}}"#,
        )?;
        assert!(matches!(
            VsCodeThemeImporter::import_file(&escape_file),
            Err(ThemeError::IncludeEscapesDirectory(_))
        ));

        let valid = br##"{
            "name": "Ocean",
            "type": "dark",
            "colors": {
                "editor.background": "#112233",
                "editor.foreground": "#AABBCC",
                "dataGrid.headerBackground": "#223344",
                "list.activeSelectionBackground": "#334455",
                "focusBorder": "#445566",
                "window.background": "#0A0B0C"
            },
            "tokenColors": [
                { "scope": "keyword", "settings": { "foreground": "#FF00AA" } }
            ]
        }"##;
        let a = VsCodeThemeImporter::import_bytes(valid)?;
        let b = VsCodeThemeImporter::import_bytes(valid)?;
        assert_ne!(a.id, b.id);
        assert_eq!(
            a.colors.get(&ThemeSlot::EditorBackground),
            Some(&Rgba::rgb(0x11, 0x22, 0x33))
        );
        assert_eq!(
            a.colors.get(&ThemeSlot::EditorSyntaxKeyword),
            Some(&Rgba::rgb(0xFF, 0x00, 0xAA))
        );
        assert_eq!(
            a.colors.get(&ThemeSlot::DataGridFocusBorder),
            Some(&Rgba::rgb(0x44, 0x55, 0x66))
        );
        Ok(())
    }

    proptest! {
        #[test]
        fn prop_rgba_hex_roundtrip(r: u8, g: u8, b: u8, a: u8) {
            let color = Rgba::with_alpha(r, g, b, a);
            let hex = color.to_hex();
            prop_assert_eq!(Rgba::from_hex(&hex), Some(color));
        }

        #[test]
        fn prop_theme_file_roundtrip(
            r in any::<u8>(),
            g in any::<u8>(),
            b in any::<u8>(),
            slot_idx in 0usize..ThemeSlot::ALL.len(),
        ) {
            let slot = ThemeSlot::ALL[slot_idx];
            let theme = ThemeDefinition {
                id: ThemeId("prop-theme".to_owned()),
                name: "Prop Theme".to_owned(),
                appearance: ThemeAppearance::Dark,
                is_builtin: false,
                colors: BTreeMap::from([(slot, Rgba::rgb(r, g, b))]),
            };
            let encoded = ThemeFileFormat::encode(&theme).map_err(|e| TestCaseError::fail(e.to_string()))?;
            let decoded = ThemeFileFormat::decode(&encoded).map_err(|e| TestCaseError::fail(e.to_string()))?;
            prop_assert_eq!(decoded, theme);
        }
    }
}
