//! Content vs Chrome theming bridge for HeroGPUI.
//!
//! Strictly enforces the Content vs Chrome architecture:
//! - [`ContentThemePalette`] maps all 26 [`ThemeSlot`]s from [`ThemeDefinition`]
//!   into GPUI `Hsla` colors exclusively for content surfaces (SQL editor,
//!   DataGrid, diff tints, and `window.background`).
//! - Chrome surfaces use HeroGPUI's [`Theme`]: Light and Dark follow Windows
//!   appearance, while High Contrast builds a live system [`Theme`] via
//!   [`ThemePalette::from_system_high_contrast`].

use dataforge_gfx::palette::{Color as GfxColor, ThemeMode, ThemePalette};
use dataforge_state::theme::{
    ResolvedTheme, Rgba as StateRgba, ThemeAppearance, ThemeDefinition, ThemeSlot,
};
use dataforge_windows_platform::detect_reduced_motion;
use herogpui::core::{readable_color, with_alpha};
use herogpui::gpui::{App, Hsla, Rgba as GpuiRgba};
use herogpui::theme::{Appearance, Theme, set_reduce_motion, set_theme};

/// Theme id registered in HeroGPUI when Windows High Contrast mode is active.
pub const HIGH_CONTRAST_THEME_ID: &str = "dataforge-high-contrast";

/// Converts a `dataforge-state` 8-bit [`StateRgba`] color into GPUI [`GpuiRgba`].
#[must_use]
pub fn state_rgba_to_gpui(rgba: StateRgba) -> GpuiRgba {
    GpuiRgba {
        r: f32::from(rgba.r) / 255.0,
        g: f32::from(rgba.g) / 255.0,
        b: f32::from(rgba.b) / 255.0,
        a: f32::from(rgba.a) / 255.0,
    }
}

/// Converts a `dataforge-state` 8-bit [`StateRgba`] color into GPUI [`Hsla`].
#[must_use]
pub fn state_rgba_to_hsla(rgba: StateRgba) -> Hsla {
    Hsla::from(state_rgba_to_gpui(rgba))
}

/// Converts a normalized `[0.0, 1.0]` [`GfxColor`] into GPUI [`Hsla`].
#[must_use]
pub fn gfx_color_to_hsla(color: GfxColor) -> Hsla {
    Hsla::from(GpuiRgba {
        r: color.r,
        g: color.g,
        b: color.b,
        a: color.a,
    })
}

/// All 26 resolved content theme slots converted to GPUI `Hsla` for SQL Editor,
/// DataGrid, Diff row tints, and `window.background`.
///
/// Chrome surfaces MUST NOT read from this palette; they read `cx.colors()`.
#[derive(Debug, Clone, PartialEq)]
pub struct ContentThemePalette {
    pub editor_background: Hsla,
    pub editor_text: Hsla,
    pub editor_selection: Hsla,
    pub editor_caret: Hsla,
    pub editor_current_line: Hsla,
    pub editor_gutter_background: Hsla,
    pub editor_gutter_text: Hsla,
    pub editor_syntax_keyword: Hsla,
    pub editor_syntax_string: Hsla,
    pub editor_syntax_comment: Hsla,
    pub editor_syntax_number: Hsla,
    pub editor_syntax_operator: Hsla,
    pub editor_syntax_punctuation: Hsla,
    pub editor_syntax_identifier: Hsla,
    pub editor_syntax_quoted_identifier: Hsla,
    pub editor_syntax_null: Hsla,
    pub data_grid_header_background: Hsla,
    pub data_grid_header_text: Hsla,
    pub data_grid_selection: Hsla,
    pub data_grid_inactive_selection: Hsla,
    pub data_grid_selection_text: Hsla,
    pub data_grid_focus_border: Hsla,
    pub data_grid_inserted: Hsla,
    pub data_grid_modified: Hsla,
    pub data_grid_deleted: Hsla,
    pub window_background: Hsla,
}

impl ContentThemePalette {
    /// Resolves all 26 [`ThemeSlot`]s from a [`ThemeDefinition`], falling back to the
    /// built-in palette for `def.appearance` when an optional slot is omitted.
    #[must_use]
    pub fn from_definition(def: &ThemeDefinition) -> Self {
        let defaults = ThemeDefinition::default_for(def.appearance);
        let slot_hsla = |slot: ThemeSlot| -> Hsla {
            if let Some(c) = def.colors.get(&slot).copied() {
                return state_rgba_to_hsla(c);
            }
            if slot == ThemeSlot::EditorSyntaxNull
                && let Some(text) = def.colors.get(&ThemeSlot::EditorText).copied()
            {
                return with_alpha(state_rgba_to_hsla(text), 0.5);
            }
            let fallback = defaults
                .colors
                .get(&slot)
                .copied()
                .unwrap_or_else(|| StateRgba::rgb(0x80, 0x80, 0x80));
            state_rgba_to_hsla(fallback)
        };

        Self::from_slot_lookup(slot_hsla)
    }

    /// Resolves all 26 [`ThemeSlot`]s from an already-resolved [`ResolvedTheme`].
    #[must_use]
    pub fn from_resolved(resolved: &ResolvedTheme) -> Self {
        let defaults = ThemeDefinition::default_for(resolved.appearance);
        let slot_hsla = |slot: ThemeSlot| -> Hsla {
            let fallback = defaults
                .colors
                .get(&slot)
                .copied()
                .unwrap_or_else(|| StateRgba::rgb(0x80, 0x80, 0x80));
            state_rgba_to_hsla(resolved.color_or_fallback(slot, fallback))
        };

        Self::from_slot_lookup(slot_hsla)
    }

    /// Resolves the active content palette for a Windows [`ThemeMode`] with an optional
    /// user-selected custom [`ThemeDefinition`].
    #[must_use]
    pub fn resolve(mode: ThemeMode, custom: Option<&ThemeDefinition>) -> Self {
        if let Some(def) = custom {
            return Self::from_definition(def);
        }
        let appearance = match mode {
            ThemeMode::Light => ThemeAppearance::Light,
            ThemeMode::Dark => ThemeAppearance::Dark,
            ThemeMode::HighContrastLight | ThemeMode::HighContrastDark => {
                ThemeAppearance::HighContrast
            }
        };
        Self::from_definition(&ThemeDefinition::default_for(appearance))
    }

    /// Looks up the GPUI [`Hsla`] color for any of the 26 [`ThemeSlot`] variants.
    #[must_use]
    pub const fn color(&self, slot: ThemeSlot) -> Hsla {
        match slot {
            ThemeSlot::EditorBackground => self.editor_background,
            ThemeSlot::EditorText => self.editor_text,
            ThemeSlot::EditorSelection => self.editor_selection,
            ThemeSlot::EditorCaret => self.editor_caret,
            ThemeSlot::EditorCurrentLine => self.editor_current_line,
            ThemeSlot::EditorGutterBackground => self.editor_gutter_background,
            ThemeSlot::EditorGutterText => self.editor_gutter_text,
            ThemeSlot::EditorSyntaxKeyword => self.editor_syntax_keyword,
            ThemeSlot::EditorSyntaxString => self.editor_syntax_string,
            ThemeSlot::EditorSyntaxComment => self.editor_syntax_comment,
            ThemeSlot::EditorSyntaxNumber => self.editor_syntax_number,
            ThemeSlot::EditorSyntaxOperator => self.editor_syntax_operator,
            ThemeSlot::EditorSyntaxPunctuation => self.editor_syntax_punctuation,
            ThemeSlot::EditorSyntaxIdentifier => self.editor_syntax_identifier,
            ThemeSlot::EditorSyntaxQuotedIdentifier => self.editor_syntax_quoted_identifier,
            ThemeSlot::EditorSyntaxNull => self.editor_syntax_null,
            ThemeSlot::DataGridHeaderBackground => self.data_grid_header_background,
            ThemeSlot::DataGridHeaderText => self.data_grid_header_text,
            ThemeSlot::DataGridSelection => self.data_grid_selection,
            ThemeSlot::DataGridInactiveSelection => self.data_grid_inactive_selection,
            ThemeSlot::DataGridSelectionText => self.data_grid_selection_text,
            ThemeSlot::DataGridFocusBorder => self.data_grid_focus_border,
            ThemeSlot::DataGridInserted => self.data_grid_inserted,
            ThemeSlot::DataGridModified => self.data_grid_modified,
            ThemeSlot::DataGridDeleted => self.data_grid_deleted,
            ThemeSlot::WindowBackground => self.window_background,
        }
    }

    fn from_slot_lookup(mut lookup: impl FnMut(ThemeSlot) -> Hsla) -> Self {
        Self {
            editor_background: lookup(ThemeSlot::EditorBackground),
            editor_text: lookup(ThemeSlot::EditorText),
            editor_selection: lookup(ThemeSlot::EditorSelection),
            editor_caret: lookup(ThemeSlot::EditorCaret),
            editor_current_line: lookup(ThemeSlot::EditorCurrentLine),
            editor_gutter_background: lookup(ThemeSlot::EditorGutterBackground),
            editor_gutter_text: lookup(ThemeSlot::EditorGutterText),
            editor_syntax_keyword: lookup(ThemeSlot::EditorSyntaxKeyword),
            editor_syntax_string: lookup(ThemeSlot::EditorSyntaxString),
            editor_syntax_comment: lookup(ThemeSlot::EditorSyntaxComment),
            editor_syntax_number: lookup(ThemeSlot::EditorSyntaxNumber),
            editor_syntax_operator: lookup(ThemeSlot::EditorSyntaxOperator),
            editor_syntax_punctuation: lookup(ThemeSlot::EditorSyntaxPunctuation),
            editor_syntax_identifier: lookup(ThemeSlot::EditorSyntaxIdentifier),
            editor_syntax_quoted_identifier: lookup(ThemeSlot::EditorSyntaxQuotedIdentifier),
            editor_syntax_null: lookup(ThemeSlot::EditorSyntaxNull),
            data_grid_header_background: lookup(ThemeSlot::DataGridHeaderBackground),
            data_grid_header_text: lookup(ThemeSlot::DataGridHeaderText),
            data_grid_selection: lookup(ThemeSlot::DataGridSelection),
            data_grid_inactive_selection: lookup(ThemeSlot::DataGridInactiveSelection),
            data_grid_selection_text: lookup(ThemeSlot::DataGridSelectionText),
            data_grid_focus_border: lookup(ThemeSlot::DataGridFocusBorder),
            data_grid_inserted: lookup(ThemeSlot::DataGridInserted),
            data_grid_modified: lookup(ThemeSlot::DataGridModified),
            data_grid_deleted: lookup(ThemeSlot::DataGridDeleted),
            window_background: lookup(ThemeSlot::WindowBackground),
        }
    }
}

/// Builds a HeroGPUI Chrome [`Theme`] from a High Contrast [`ThemePalette`].
#[must_use]
pub fn high_contrast_theme_from_palette(palette: &ThemePalette) -> Theme {
    let is_dark = matches!(palette.mode, ThemeMode::HighContrastDark);
    let (appearance, base) = if is_dark {
        (Appearance::Dark, Theme::dark())
    } else {
        (Appearance::Light, Theme::light())
    };
    let bg = gfx_color_to_hsla(palette.content.bg);
    let fg = gfx_color_to_hsla(palette.chrome.text);
    let muted = gfx_color_to_hsla(palette.chrome.text_muted);
    let border = gfx_color_to_hsla(palette.chrome.border);
    let accent = gfx_color_to_hsla(palette.content.accent);
    let surface_bg = gfx_color_to_hsla(palette.chrome.toolbar_bg);
    let hover = gfx_color_to_hsla(palette.chrome.hover_bg);

    Theme::builder(HIGH_CONTRAST_THEME_ID, base)
        .appearance(appearance)
        .background(bg)
        .foreground(fg)
        .muted(muted)
        .border(border)
        .separator(border)
        .focus(accent)
        .link(accent)
        .surface(surface_bg, fg)
        .surface_levels(surface_bg, hover)
        .overlay(bg, fg)
        .segment(surface_bg, fg)
        .accent(accent)
        .role(
            herogpui::core::Color::Default,
            surface_bg,
            readable_color(surface_bg),
        )
        .field(bg, fg)
        .field_placeholder(muted)
        .field_border(border)
        .border_width(herogpui::gpui::px(1.5))
        .build()
}

/// Builds a live Windows High Contrast HeroGPUI [`Theme`] using `GetSysColor`.
#[must_use]
pub fn high_contrast_chrome_theme() -> Theme {
    let sys = ThemePalette::from_system_high_contrast();
    high_contrast_theme_from_palette(&sys)
}

/// Selects the HeroGPUI Chrome [`Theme`] matching `mode`.
#[must_use]
pub fn chrome_theme_for_mode(mode: ThemeMode) -> Theme {
    match mode {
        ThemeMode::Light => Theme::light(),
        ThemeMode::Dark => Theme::dark(),
        ThemeMode::HighContrastLight | ThemeMode::HighContrastDark => high_contrast_chrome_theme(),
    }
}

/// Applies the active HeroGPUI Chrome [`Theme`] and Windows reduced-motion setting.
pub fn apply_chrome_theme_and_motion(mode: ThemeMode, cx: &mut App) {
    set_theme(chrome_theme_for_mode(mode), cx);
    set_reduce_motion(detect_reduced_motion(), cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_palette_covers_all_26_slots_for_every_builtin_theme() {
        for def in [
            ThemeDefinition::builtin_light(),
            ThemeDefinition::builtin_dark(),
            ThemeDefinition::builtin_high_contrast(),
        ] {
            let palette = ContentThemePalette::from_definition(&def);
            for slot in ThemeSlot::ALL {
                let expected = def.colors.get(&slot).copied().map(state_rgba_to_hsla);
                assert_eq!(
                    Some(palette.color(slot)),
                    expected,
                    "slot {} mismatch for theme {}",
                    slot.key(),
                    def.name
                );
            }
        }
    }

    #[test]
    fn omitted_null_syntax_derives_half_alpha_from_editor_text() {
        let mut def = ThemeDefinition::builtin_dark();
        def.colors.remove(&ThemeSlot::EditorSyntaxNull);
        let text = StateRgba::rgb(0xCC, 0xDD, 0xEE);
        def.colors.insert(ThemeSlot::EditorText, text);

        let palette = ContentThemePalette::from_definition(&def);
        let expected = with_alpha(state_rgba_to_hsla(text), 0.5);
        assert_eq!(palette.color(ThemeSlot::EditorSyntaxNull), expected);
    }

    #[test]
    fn high_contrast_chrome_theme_builds_cleanly() {
        let hc_palette = ThemePalette::for_mode(ThemeMode::HighContrastDark);
        let theme = high_contrast_theme_from_palette(&hc_palette);
        assert_eq!(theme.id.as_ref(), HIGH_CONTRAST_THEME_ID);
        assert_eq!(theme.appearance, Appearance::Dark);
        assert_eq!(
            theme.colors.background,
            gfx_color_to_hsla(hc_palette.content.bg)
        );
        assert_eq!(
            theme.colors.border,
            gfx_color_to_hsla(hc_palette.chrome.border)
        );
    }
}
