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
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
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
                Some(Self::rgba(r, g, b, a))
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
                Some(Self::rgba(r, g, b, a))
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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
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
        colors.insert(ThemeSlot::EditorSelection, Rgba::rgba(0xAD, 0xD6, 0xFF, 0xC0));
        colors.insert(ThemeSlot::EditorCaret, Rgba::rgb(0x00, 0x00, 0x00));
        colors.insert(ThemeSlot::EditorCurrentLine, Rgba::rgb(0xF5, 0xF7, 0xFA));
        colors.insert(ThemeSlot::EditorGutterBackground, Rgba::rgb(0xF8, 0xF8, 0xF8));
        colors.insert(ThemeSlot::EditorGutterText, Rgba::rgb(0x6E, 0x76, 0x81));
        colors.insert(ThemeSlot::EditorSyntaxKeyword, Rgba::rgb(0x00, 0x33, 0xB3));
        colors.insert(ThemeSlot::EditorSyntaxString, Rgba::rgb(0x06, 0x7D, 0x17));
        colors.insert(ThemeSlot::EditorSyntaxComment, Rgba::rgb(0x6A, 0x73, 0x7D));
        colors.insert(ThemeSlot::EditorSyntaxNumber, Rgba::rgb(0x17, 0x50, 0xEB));
        colors.insert(ThemeSlot::EditorSyntaxOperator, Rgba::rgb(0x24, 0x29, 0x2E));
        colors.insert(ThemeSlot::EditorSyntaxPunctuation, Rgba::rgb(0x4E, 0x56, 0x60));
        colors.insert(ThemeSlot::EditorSyntaxIdentifier, Rgba::rgb(0x1E, 0x1E, 0x1E));
        colors.insert(ThemeSlot::EditorSyntaxQuotedIdentifier, Rgba::rgb(0x04, 0x51, 0xA5));
        colors.insert(ThemeSlot::EditorSyntaxNull, Rgba::rgb(0x80, 0x80, 0x80));
        colors.insert(ThemeSlot::DataGridHeaderBackground, Rgba::rgb(0xF3, 0xF4, 0xF6));
        colors.insert(ThemeSlot::DataGridHeaderText, Rgba::rgb(0x1F, 0x23, 0x28));
        colors.insert(ThemeSlot::DataGridSelection, Rgba::rgb(0x09, 0x69, 0xDA));
        colors.insert(ThemeSlot::DataGridInactiveSelection, Rgba::rgb(0xD0, 0xD7, 0xDE));
        colors.insert(ThemeSlot::DataGridSelectionText, Rgba::rgb(0xFF, 0xFF, 0xFF));
        colors.insert(ThemeSlot::DataGridFocusBorder, Rgba::rgb(0x09, 0x69, 0xDA));
        colors.insert(ThemeSlot::DataGridInserted, Rgba::rgba(0x2D, 0xA4, 0x4E, 0x38));
        colors.insert(ThemeSlot::DataGridModified, Rgba::rgba(0xBF, 0x87, 0x00, 0x38));
        colors.insert(ThemeSlot::DataGridDeleted, Rgba::rgba(0xCF, 0x22, 0x2E, 0x38));
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
        colors.insert(ThemeSlot::EditorBackground, Rgba::rgb(0x1E, 0x1E, 0x1E));
        colors.insert(ThemeSlot::EditorText, Rgba::rgb(0xD4, 0xD4, 0xD4));
        colors.insert(ThemeSlot::EditorSelection, Rgba::rgba(0x26, 0x4F, 0x78, 0xCC));
        colors.insert(ThemeSlot::EditorCaret, Rgba::rgb(0xAE, 0xAF, 0xAD));
        colors.insert(ThemeSlot::EditorCurrentLine, Rgba::rgb(0x28, 0x28, 0x28));
        colors.insert(ThemeSlot::EditorGutterBackground, Rgba::rgb(0x1E, 0x1E, 0x1E));
        colors.insert(ThemeSlot::EditorGutterText, Rgba::rgb(0x85, 0x85, 0x85));
        colors.insert(ThemeSlot::EditorSyntaxKeyword, Rgba::rgb(0x56, 0x9C, 0xD6));
        colors.insert(ThemeSlot::EditorSyntaxString, Rgba::rgb(0xCE, 0x91, 0x78));
        colors.insert(ThemeSlot::EditorSyntaxComment, Rgba::rgb(0x6A, 0x99, 0x55));
        colors.insert(ThemeSlot::EditorSyntaxNumber, Rgba::rgb(0xB5, 0xCE, 0xA8));
        colors.insert(ThemeSlot::EditorSyntaxOperator, Rgba::rgb(0xD4, 0xD4, 0xD4));
        colors.insert(ThemeSlot::EditorSyntaxPunctuation, Rgba::rgb(0xA0, 0xA0, 0xA0));
        colors.insert(ThemeSlot::EditorSyntaxIdentifier, Rgba::rgb(0xD4, 0xD4, 0xD4));
        colors.insert(ThemeSlot::EditorSyntaxQuotedIdentifier, Rgba::rgb(0x9C, 0xDC, 0xFE));
        colors.insert(ThemeSlot::EditorSyntaxNull, Rgba::rgb(0x80, 0x80, 0x80));
        colors.insert(ThemeSlot::DataGridHeaderBackground, Rgba::rgb(0x25, 0x25, 0x26));
        colors.insert(ThemeSlot::DataGridHeaderText, Rgba::rgb(0xCC, 0xCC, 0xCC));
        colors.insert(ThemeSlot::DataGridSelection, Rgba::rgb(0x09, 0x47, 0x71));
        colors.insert(ThemeSlot::DataGridInactiveSelection, Rgba::rgb(0x3A, 0x3D, 0x41));
        colors.insert(ThemeSlot::DataGridSelectionText, Rgba::rgb(0xFF, 0xFF, 0xFF));
        colors.insert(ThemeSlot::DataGridFocusBorder, Rgba::rgb(0x00, 0x7F, 0xD4));
        colors.insert(ThemeSlot::DataGridInserted, Rgba::rgba(0x2E, 0xA0, 0x43, 0x40));
        colors.insert(ThemeSlot::DataGridModified, Rgba::rgba(0xD2, 0x99, 0x22, 0x40));
        colors.insert(ThemeSlot::DataGridDeleted, Rgba::rgba(0xF8, 0x51, 0x49, 0x40));
        colors.insert(ThemeSlot::WindowBackground, Rgba::rgb(0x1E, 0x1E, 0x1E));
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
        colors.insert(ThemeSlot::EditorGutterBackground, Rgba::rgb(0x00, 0x00, 0x00));
        colors.insert(ThemeSlot::EditorGutterText, Rgba::rgb(0xFF, 0xFF, 0xFF));
        colors.insert(ThemeSlot::EditorSyntaxKeyword, Rgba::rgb(0x56, 0x9C, 0xD6));
        colors.insert(ThemeSlot::EditorSyntaxString, Rgba::rgb(0xCE, 0x91, 0x78));
        colors.insert(ThemeSlot::EditorSyntaxComment, Rgba::rgb(0x7C, 0xA6, 0x68));
        colors.insert(ThemeSlot::EditorSyntaxNumber, Rgba::rgb(0xB5, 0xCE, 0xA8));
        colors.insert(ThemeSlot::EditorSyntaxOperator, Rgba::rgb(0xFF, 0xFF, 0xFF));
        colors.insert(ThemeSlot::EditorSyntaxPunctuation, Rgba::rgb(0xFF, 0xFF, 0xFF));
        colors.insert(ThemeSlot::EditorSyntaxIdentifier, Rgba::rgb(0xFF, 0xFF, 0xFF));
        colors.insert(ThemeSlot::EditorSyntaxQuotedIdentifier, Rgba::rgb(0x9C, 0xDC, 0xFE));
        colors.insert(ThemeSlot::EditorSyntaxNull, Rgba::rgb(0xD4, 0xD4, 0xD4));
        colors.insert(ThemeSlot::DataGridHeaderBackground, Rgba::rgb(0x00, 0x00, 0x00));
        colors.insert(ThemeSlot::DataGridHeaderText, Rgba::rgb(0xFF, 0xFF, 0xFF));
        colors.insert(ThemeSlot::DataGridSelection, Rgba::rgb(0x00, 0x78, 0xD7));
        colors.insert(ThemeSlot::DataGridInactiveSelection, Rgba::rgb(0x3F, 0x3F, 0x46));
        colors.insert(ThemeSlot::DataGridSelectionText, Rgba::rgb(0xFF, 0xFF, 0xFF));
        colors.insert(ThemeSlot::DataGridFocusBorder, Rgba::rgb(0xF3, 0x85, 0x18));
        colors.insert(ThemeSlot::DataGridInserted, Rgba::rgba(0x1B, 0x87, 0x3F, 0x60));
        colors.insert(ThemeSlot::DataGridModified, Rgba::rgba(0xD2, 0x99, 0x22, 0x60));
        colors.insert(ThemeSlot::DataGridDeleted, Rgba::rgba(0xE5, 0x14, 0x00, 0x60));
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
        if !self.is_themed || !self.can_reach_chrome || !self.reduce_transparency || self.increase_contrast {
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

    fn from_definition(saved_id: Option<ThemeId>, def: &ThemeDefinition, used_fallback: bool) -> ResolvedTheme {
        let mut palette = def.colors.clone();
        // Derive `editor.syntax.null` from `editor.text` at ~50% alpha if not explicitly set.
        if !palette.contains_key(&ThemeSlot::EditorSyntaxNull)
            && let Some(text) = palette.get(&ThemeSlot::EditorText).copied()
        {
            palette.insert(
                ThemeSlot::EditorSyntaxNull,
                Rgba::rgba(text.r, text.g, text.b, text.a / 2),
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

    #[error("unsupported theme schema version {schema} (supported: {MIN_READABLE_SCHEMA}..={CURRENT_SCHEMA})")]
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
        let bytes = serde_json::to_vec_pretty(&payload).map_err(|e| ThemeError::InvalidJson(e.to_string()))?;
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
            return Err(ThemeError::UnsupportedSchema { schema: payload.schema });
        }
        if payload.id.trim().is_empty() {
            return Err(ThemeError::EmptyId);
        }
        if payload.name.trim().is_empty() {
            return Err(ThemeError::EmptyName);
        }

        let mut colors = BTreeMap::new();
        for (key, hex) in payload.colors {
            let slot = ThemeSlot::from_key(&key).ok_or_else(|| ThemeError::UnknownSlotKey(key.clone()))?;
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
