//! Content vs Chrome theming bridge for HeroGPUI.
//!
//! Strictly enforces the Content vs Chrome architecture:
//! - [`ContentThemePalette`] maps all 26 [`ThemeSlot`]s from [`ThemeDefinition`]
//!   into GPUI `Hsla` colors exclusively for content surfaces (SQL editor,
//!   DataGrid, diff tints, and `window.background`).
//! - Chrome surfaces use HeroGPUI's [`Theme`]: Light and Dark follow Windows
//!   appearance, while High Contrast builds a live system [`Theme`] via
//!   [`ThemePalette::from_system_high_contrast`].

use std::time::Duration;

use herogpui::core::{readable_color, with_alpha};
use herogpui::gpui::{
    Animation, AnimationExt, AnyElement, App, ElementId, Font, FontFeatures, FontStyle, Hsla,
    IntoElement, Rgba as GpuiRgba, Styled, font as gpui_font, px,
};
use herogpui::theme::{ActiveTheme, Appearance, Theme, set_reduce_motion, set_theme};
use tableminus_gfx::palette::{Color as GfxColor, ThemeMode, ThemePalette};
use tableminus_state::theme::{
    ResolvedTheme, Rgba as StateRgba, ThemeAppearance, ThemeDefinition, ThemeSlot,
};
use tableminus_windows_platform::{
    AppSettings, EditorFont, FontSpec, WindowBackdropPreference, detect_reduced_motion,
};

/// Theme id registered in HeroGPUI when Light mode is active.
pub const LIGHT_THEME_ID: &str = "tableminus-light";

/// Theme id registered in HeroGPUI when Dark mode is active.
pub const DARK_THEME_ID: &str = "tableminus-dark";

/// Theme id registered in HeroGPUI when Windows High Contrast mode is active.
pub const HIGH_CONTRAST_THEME_ID: &str = "tableminus-high-contrast";

/// Crisp hairline border width (`1.0` DIP) for dense developer tool surfaces.
pub const HAIRLINE_BORDER_PX: f32 = 1.0;

/// Compact control corner radius (`6.0` DIP) aligned with Windows Fluent / HeroGPUI.
pub const CONTROL_RADIUS_PX: f32 = 6.0;

/// Panel and card corner radius (`8.0` DIP).
pub const PANEL_RADIUS_PX: f32 = 8.0;

/// Micro layout gap (`4.0` DIP) between tight icon/label/badge pairs.
pub const MICRO_GAP_PX: f32 = 4.0;

/// Standard compact spacing gap (`8.0` DIP) inside toolbars, sidebars, and drawers.
pub const COMPACT_GAP_PX: f32 = 8.0;

/// Title/section heading font size (`14.0` DIP) for 1536x864 readability.
pub const TEXT_TITLE_PX: f32 = 14.0;

/// Primary UI body/label font size (`13.0` DIP) for sidebar items, menus, and active pills.
pub const TEXT_BODY_PX: f32 = 13.0;

/// Compact control/toolbar/tab font size (`12.5` DIP).
pub const TEXT_CONTROL_PX: f32 = 12.5;

/// Metadata/status-bar/secondary label font size (`12.0` DIP).
pub const TEXT_META_PX: f32 = 12.0;

/// Compact badge/chip/kbd shortcut font size (`11.0` DIP).
pub const TEXT_BADGE_PX: f32 = 11.0;

/// Zero-delay instant transition (`0ms`) for high-frequency actions (100+/day).
pub const INSTANT_ACTION_MS: u64 = 0;

/// Crisp hover/button feedback duration (`100ms`, <= `150ms`).
pub const HOVER_FEEDBACK_MS: u64 = 100;

/// Overlay/modal entrance duration (`150ms`, < `250ms`).
pub const OVERLAY_ENTER_MS: u64 = 150;

/// Overlay/modal exit duration (`100ms`).
pub const OVERLAY_EXIT_MS: u64 = 100;

/// Popover and dropdown menu entrance duration (`120ms`).
pub const POPOVER_ENTER_MS: u64 = 120;

/// Drawer/panel slide transition duration (`180ms`, < `250ms`).
pub const DRAWER_SLIDE_MS: u64 = 180;

/// Screen transition fade duration (`160ms`, < `250ms`).
pub const SCREEN_TRANSITION_MS: u64 = 160;

const fn rgb_hsla(r: u8, g: u8, b: u8) -> GpuiRgba {
    GpuiRgba {
        r: (r as f32) / 255.0,
        g: (g as f32) / 255.0,
        b: (b as f32) / 255.0,
        a: 1.0,
    }
}

const fn rgba_hsla(r: u8, g: u8, b: u8, a: f32) -> GpuiRgba {
    GpuiRgba {
        r: (r as f32) / 255.0,
        g: (g as f32) / 255.0,
        b: (b as f32) / 255.0,
        a,
    }
}

/// Converts a `tableminus-state` 8-bit [`StateRgba`] color into GPUI [`GpuiRgba`].
#[must_use]
pub fn state_rgba_to_gpui(rgba: StateRgba) -> GpuiRgba {
    GpuiRgba {
        r: f32::from(rgba.r) / 255.0,
        g: f32::from(rgba.g) / 255.0,
        b: f32::from(rgba.b) / 255.0,
        a: f32::from(rgba.a) / 255.0,
    }
}

/// Converts a `tableminus-state` 8-bit [`StateRgba`] color into GPUI [`Hsla`].
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

/// Builds the harmonious Apple-style Dark Chrome [`Theme`].
///
/// Surface elevation ladder:
/// - Recessed/Base (`background`, `surface_secondary`): `#17181C`
/// - Editor/Content (`field.background`, content palette): `#1B1C20`
/// - Chrome/Toolbar/Sidebar (`surface.background`): `#1F2025`
/// - Elevated/Card/Popover (`overlay.background`, `surface_tertiary`): `#25272D`
/// - Hairline border (`border`, `separator`): `rgba(255, 255, 255, 0.08)`
#[must_use]
pub fn dark_chrome_theme() -> Theme {
    let base = Theme::dark();
    let fg = base.colors.foreground;
    let recessed: Hsla = rgb_hsla(0x17, 0x18, 0x1C).into();
    let editor: Hsla = rgb_hsla(0x1B, 0x1C, 0x20).into();
    let chrome: Hsla = rgb_hsla(0x1F, 0x20, 0x25).into();
    let elevated: Hsla = rgb_hsla(0x25, 0x27, 0x2D).into();
    let hairline: Hsla = rgba_hsla(255, 255, 255, 0.08).into();

    Theme::builder(DARK_THEME_ID, base)
        .appearance(Appearance::Dark)
        .background(recessed)
        .surface(chrome, fg)
        .surface_levels(recessed, elevated)
        .role(herogpui::core::Color::Default, elevated, fg)
        .overlay(elevated, fg)
        .border(hairline)
        .separator(hairline)
        .field(editor, fg)
        .field_border(hairline)
        .backdrop(rgba_hsla(0, 0, 0, 0.55).into())
        .build()
}

/// Builds the harmonious Apple-style Light Chrome [`Theme`].
///
/// Surface elevation ladder:
/// - Base/Recessed (`background`, `surface_secondary`): `#F3F4F6`
/// - Chrome/Toolbar/Sidebar (`surface.background`): `#F8F9FB`
/// - Editor/Content & Elevated/Card/Popover (`overlay.background`, `surface_tertiary`): `#FFFFFF`
/// - Hairline border (`border`, `separator`): `rgba(0, 0, 0, 0.08)`
#[must_use]
pub fn light_chrome_theme() -> Theme {
    let base = Theme::light();
    let fg = base.colors.foreground;
    let recessed: Hsla = rgb_hsla(0xF3, 0xF4, 0xF6).into();
    let chrome: Hsla = rgb_hsla(0xF8, 0xF9, 0xFB).into();
    let elevated: Hsla = rgb_hsla(0xFF, 0xFF, 0xFF).into();
    let hairline: Hsla = rgba_hsla(0, 0, 0, 0.08).into();

    Theme::builder(LIGHT_THEME_ID, base)
        .appearance(Appearance::Light)
        .background(recessed)
        .surface(chrome, fg)
        .surface_levels(recessed, elevated)
        .overlay(elevated, fg)
        .border(hairline)
        .separator(hairline)
        .field(elevated, fg)
        .field_border(hairline)
        .backdrop(rgba_hsla(15, 23, 42, 0.28).into())
        .build()
}

/// Selects the HeroGPUI Chrome [`Theme`] matching `mode`.
#[must_use]
pub fn chrome_theme_for_mode(mode: ThemeMode) -> Theme {
    match mode {
        ThemeMode::Light => light_chrome_theme(),
        ThemeMode::Dark => dark_chrome_theme(),
        ThemeMode::HighContrastLight | ThemeMode::HighContrastDark => high_contrast_chrome_theme(),
    }
}

/// Builds the active HeroGPUI Chrome [`Theme`] with backdrop translucency applied.
#[must_use]
pub fn chrome_theme_with_backdrop(mode: ThemeMode, backdrop: WindowBackdropPreference) -> Theme {
    let mut theme = chrome_theme_for_mode(mode);
    let alphas = match backdrop {
        WindowBackdropPreference::Opaque => None,
        // Gaussian Blur: translucent across window & panel surfaces so frosted glass shows
        // across the workspace, while keeping `overlay.background` opaque because GPUI
        // does not support an in-app `backdrop-filter: blur()` shader over UI text.
        WindowBackdropPreference::GaussianBlur => Some((0.46, 0.36, 0.28, 0.24)),
        WindowBackdropPreference::Mica | WindowBackdropPreference::MicaAlt => {
            Some((0.66, 0.56, 0.44, 0.38))
        }
    };
    if let Some((bg_a, surf_a, sec_a, tert_a)) = alphas {
        theme.colors.background = with_alpha(theme.colors.background, bg_a);
        theme.colors.surface.background = with_alpha(theme.colors.surface.background, surf_a);
        theme.colors.surface_secondary = with_alpha(theme.colors.surface_secondary, sec_a);
        theme.colors.surface_tertiary = with_alpha(theme.colors.surface_tertiary, tert_a);
    }
    theme
}

/// Derives an opaque [`Theme`] snapshot for floating toast cards from `base`.
///
/// HeroGPUI's `ToastCardEl` paints toast cards with `colors.surface.background` rather
/// than `colors.overlay.background`. Under translucent Windows backdrops (`GaussianBlur`,
/// `Mica`, `MicaAlt`), `surface.background` carries panel translucency, letting underlying
/// sharp text bleed through because GPUI has no in-app `backdrop-filter: blur()` shader.
/// Promoting `surface.background` to the opaque (`1.0` alpha) `overlay.background` keeps
/// toast cards solid across every backdrop and theme mode while leaving workspace panels
/// translucent.
#[must_use]
pub fn toast_opaque_theme(base: &Theme) -> Theme {
    let mut theme = base.clone();
    let opaque_overlay = with_alpha(base.colors.overlay.background, 1.0);
    theme.colors.overlay.background = opaque_overlay;
    theme.colors.surface.background = opaque_overlay;
    theme.colors.surface.foreground = base.colors.overlay.foreground;
    theme.colors.background = with_alpha(base.colors.background, 1.0);
    theme.colors.surface_secondary = with_alpha(base.colors.surface_secondary, 1.0);
    theme.colors.surface_tertiary = with_alpha(base.colors.surface_tertiary, 1.0);
    theme
}

/// Applies the active HeroGPUI Chrome [`Theme`], backdrop translucency, and Windows reduced-motion setting.
pub fn apply_chrome_theme_and_motion(
    mode: ThemeMode,
    backdrop: WindowBackdropPreference,
    cx: &mut App,
) {
    let theme = chrome_theme_with_backdrop(mode, backdrop);
    set_theme(theme, cx);
    set_reduce_motion(detect_reduced_motion(), cx);
}

/// Applies backdrop translucency to content surfaces (SQL editor, gutter, grid header)
/// so Gaussian Blur / Mica covers the entire window uniformly.
pub fn apply_backdrop_to_content_palette(
    palette: &mut ContentThemePalette,
    backdrop: WindowBackdropPreference,
) {
    let (editor_a, gutter_a, header_a) = match backdrop {
        WindowBackdropPreference::Opaque => return,
        WindowBackdropPreference::GaussianBlur => (0.18, 0.14, 0.30),
        WindowBackdropPreference::Mica | WindowBackdropPreference::MicaAlt => (0.35, 0.28, 0.45),
    };
    palette.editor_background = with_alpha(palette.editor_background, editor_a);
    palette.editor_gutter_background = with_alpha(palette.editor_gutter_background, gutter_a);
    palette.editor_current_line = with_alpha(palette.editor_current_line, editor_a);
    palette.data_grid_header_background = with_alpha(palette.data_grid_header_background, header_a);
    palette.window_background = with_alpha(palette.window_background, editor_a);
}

fn build_font(family: &str, ligatures: bool) -> Font {
    let mut built = gpui_font(family.to_owned());
    built.features = if ligatures {
        FontFeatures::default()
    } else {
        FontFeatures::disable_ligatures()
    };
    built.style = FontStyle::Normal;
    built
}

/// Converts [`EditorFont`] settings into a GPUI [`Font`] for SQL editing and code blocks.
#[must_use]
pub fn gpui_font_from_editor(spec: &EditorFont) -> Font {
    build_font(&spec.family, spec.ligatures)
}

/// Converts tabular [`FontSpec`] settings into a GPUI [`Font`] for grid cells and values.
#[must_use]
pub fn gpui_font_from_spec(spec: &FontSpec) -> Font {
    build_font(&spec.family, spec.ligatures)
}

/// Returns the active SQL editor [`Font`] configured in [`AppSettings`].
#[must_use]
pub fn editor_gpui_font(settings: &AppSettings) -> Font {
    gpui_font_from_editor(&settings.editor_font)
}

/// Returns the active tabular value/grid [`Font`] configured in [`AppSettings`].
#[must_use]
pub fn value_gpui_font(settings: &AppSettings) -> Font {
    gpui_font_from_spec(&settings.value_font)
}

/// Crisp hairline border color from the active HeroGPUI Chrome [`Theme`].
#[must_use]
pub fn hairline_border_color(cx: &App) -> Hsla {
    cx.colors().border
}

/// Primary elevated panel surface fill from the active Chrome [`Theme`].
#[must_use]
pub fn panel_surface_bg(cx: &App) -> Hsla {
    cx.colors().surface.background
}

/// Secondary/muted surface fill (headers, wells, recessed strips).
#[must_use]
pub fn muted_surface_bg(cx: &App) -> Hsla {
    cx.colors().surface_secondary
}

/// Floating overlay/popover/modal surface fill from the active Chrome [`Theme`].
#[must_use]
pub fn elevated_overlay_bg(cx: &App) -> Hsla {
    cx.colors().overlay.background
}

/// Subtle zebra-stripe tint for odd rows in dense grids (none for even rows).
#[must_use]
pub fn row_zebra_bg(row_index: usize, cx: &App) -> Option<Hsla> {
    if row_index.is_multiple_of(2) {
        None
    } else {
        Some(with_alpha(cx.colors().surface_secondary, 0.45))
    }
}

/// Returns `true` when reduced motion is enabled in GPUI or Windows `SPI_GETCLIENTAREAANIMATION`.
#[must_use]
pub fn is_reduced_motion(cx: &App) -> bool {
    ActiveTheme::reduce_motion(cx) || detect_reduced_motion()
}

/// Returns `0ms` when reduced motion is active, otherwise `base_ms`.
#[must_use]
pub fn effective_duration_ms(base_ms: u64, cx: &App) -> u64 {
    if is_reduced_motion(cx) { 0 } else { base_ms }
}

/// Zero-delay duration (`0ms`) for high-frequency navigation, selection, and tab switching.
#[must_use]
pub const fn instant_duration() -> Duration {
    Duration::from_millis(INSTANT_ACTION_MS)
}

/// Hover/button micro-transition duration (`<= 150ms`, or `0ms` under reduced motion).
#[must_use]
pub fn hover_duration(cx: &App) -> Duration {
    Duration::from_millis(effective_duration_ms(HOVER_FEEDBACK_MS, cx))
}

/// Modal/popover entrance transition duration (`< 250ms`, or `0ms` under reduced motion).
#[must_use]
pub fn overlay_duration(cx: &App) -> Duration {
    Duration::from_millis(effective_duration_ms(OVERLAY_ENTER_MS, cx))
}

/// Drawer/panel slide transition duration (`< 250ms`, or `0ms` under reduced motion).
#[must_use]
pub fn drawer_duration(cx: &App) -> Duration {
    Duration::from_millis(effective_duration_ms(DRAWER_SLIDE_MS, cx))
}

/// Smooth quadratic ease-out curve for compact UI micro-transitions.
#[must_use]
pub fn ease_out_quad(t: f32) -> f32 {
    let clamped = t.clamp(0.0, 1.0);
    1.0 - (1.0 - clamped) * (1.0 - clamped)
}

/// Apple-style cubic ease-out curve for crisp hover, fade, and active indicator transitions.
#[must_use]
pub fn ease_out_cubic(t: f32) -> f32 {
    let inv = 1.0 - t.clamp(0.0, 1.0);
    1.0 - inv * inv * inv
}

/// Apple-style quintic ease-out curve for decelerating drawers, modals, and toast entrances.
#[must_use]
pub fn ease_out_quint(t: f32) -> f32 {
    let inv = 1.0 - t.clamp(0.0, 1.0);
    let inv2 = inv * inv;
    1.0 - inv2 * inv2 * inv
}

/// Smoothly fades an element in from `0.0` to `1.0` using [`ease_out_cubic`].
///
/// Returns `el` untouched when reduced motion is active or `duration_ms == 0`.
pub fn animate_fade_in<E>(el: E, id: impl Into<ElementId>, duration_ms: u64, cx: &App) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    let ms = effective_duration_ms(duration_ms, cx);
    if ms == 0 {
        return el.into_any_element();
    }
    el.with_animation(
        id.into(),
        Animation::new(Duration::from_millis(ms)).with_easing(ease_out_cubic),
        |el, delta| if delta >= 1.0 { el } else { el.opacity(delta) },
    )
    .into_any_element()
}

/// Smoothly fades and settles an element vertically by `offset_y_px` using [`ease_out_quint`].
///
/// Returns `el` untouched when reduced motion is active or `duration_ms == 0`.
pub fn animate_slide_fade_in<E>(
    el: E,
    id: impl Into<ElementId>,
    duration_ms: u64,
    offset_y_px: f32,
    cx: &App,
) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    let ms = effective_duration_ms(duration_ms, cx);
    if ms == 0 {
        return el.into_any_element();
    }
    el.with_animation(
        id.into(),
        Animation::new(Duration::from_millis(ms)).with_easing(ease_out_quint),
        move |el, delta| {
            if delta >= 1.0 {
                el
            } else {
                el.opacity(delta).mt(px(offset_y_px * (1.0 - delta)))
            }
        },
    )
    .into_any_element()
}

/// Smooth Apple-style drawer/panel entrance (opacity + subtle 14px horizontal slide).
///
/// Returns `el` untouched when reduced motion is active.
pub fn animate_drawer_enter<E>(
    el: E,
    id: impl Into<ElementId>,
    from_right: bool,
    cx: &App,
) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    let ms = effective_duration_ms(DRAWER_SLIDE_MS, cx);
    if ms == 0 {
        return el.into_any_element();
    }
    el.with_animation(
        id.into(),
        Animation::new(Duration::from_millis(ms)).with_easing(ease_out_quint),
        move |el, delta| {
            if delta >= 1.0 {
                return el;
            }
            let offset = px(14.0 * (1.0 - delta));
            let el = el.opacity(delta);
            if from_right {
                el.mr(-offset)
            } else {
                el.ml(-offset)
            }
        },
    )
    .into_any_element()
}

/// Smooth Apple-style modal entrance (opacity + subtle 10px upward settle).
///
/// Returns `el` untouched when reduced motion is active.
pub fn animate_modal_enter<E>(el: E, id: impl Into<ElementId>, cx: &App) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    animate_slide_fade_in(el, id, OVERLAY_ENTER_MS, 10.0, cx)
}

/// Smooth modal exit transition (opacity `1 -> 0` + subtle 8px downward settle).
///
/// Returns `el` untouched when reduced motion is active.
pub fn animate_modal_exit<E>(el: E, id: impl Into<ElementId>, cx: &App) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    let ms = effective_duration_ms(OVERLAY_EXIT_MS, cx);
    if ms == 0 {
        return el.into_any_element();
    }
    el.with_animation(
        id.into(),
        Animation::new(Duration::from_millis(ms)).with_easing(ease_out_quad),
        |el, delta| {
            let alpha = (1.0 - delta).clamp(0.0, 1.0);
            el.opacity(alpha).mt(px(8.0 * delta))
        },
    )
    .into_any_element()
}

/// Smooth backdrop scrim fade for opening or closing modals.
///
/// Returns `el` untouched when reduced motion is active.
pub fn animate_backdrop_fade<E>(
    el: E,
    id: impl Into<ElementId>,
    closing: bool,
    cx: &App,
) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    let base_ms = if closing {
        OVERLAY_EXIT_MS
    } else {
        OVERLAY_ENTER_MS
    };
    let ms = effective_duration_ms(base_ms, cx);
    if ms == 0 {
        return el.into_any_element();
    }
    el.with_animation(
        id.into(),
        Animation::new(Duration::from_millis(ms)).with_easing(ease_out_cubic),
        move |el, delta| {
            let alpha = if closing {
                (1.0 - delta).clamp(0.0, 1.0)
            } else {
                delta
            };
            if !closing && alpha >= 1.0 {
                el
            } else {
                el.opacity(alpha)
            }
        },
    )
    .into_any_element()
}

/// Crisp dropdown/popover menu entrance (opacity + subtle 6px vertical settle).
///
/// Returns `el` untouched when reduced motion is active.
pub fn animate_popover_enter<E>(el: E, id: impl Into<ElementId>, cx: &App) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    animate_slide_fade_in(el, id, POPOVER_ENTER_MS, 6.0, cx)
}

/// Directional horizontal slide + fade for wizard step transitions.
///
/// Returns `el` untouched when reduced motion is active.
pub fn animate_step_slide<E>(el: E, id: impl Into<ElementId>, forward: bool, cx: &App) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    let ms = effective_duration_ms(DRAWER_SLIDE_MS, cx);
    if ms == 0 {
        return el.into_any_element();
    }
    el.with_animation(
        id.into(),
        Animation::new(Duration::from_millis(ms)).with_easing(ease_out_quint),
        move |el, delta| {
            if delta >= 1.0 {
                return el;
            }
            let offset = px(16.0 * (1.0 - delta));
            let el = el.opacity(delta);
            if forward {
                el.ml(offset).mr(-offset)
            } else {
                el.ml(-offset).mr(offset)
            }
        },
    )
    .into_any_element()
}

/// Subtle staggered card/row entrance capped at 8 steps so lists ripple cleanly without lag.
///
/// Returns `el` untouched when reduced motion is active.
pub fn animate_staggered_enter<E>(
    el: E,
    id: impl Into<ElementId>,
    index: usize,
    cx: &App,
) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    let step = index.min(8) as u64;
    let total_ms = effective_duration_ms(OVERLAY_ENTER_MS + step * 16, cx);
    if total_ms == 0 {
        return el.into_any_element();
    }
    let delay_frac = if step == 0 {
        0.0
    } else {
        ((step * 16) as f32 / total_ms as f32).min(0.45)
    };
    el.with_animation(
        id.into(),
        Animation::new(Duration::from_millis(total_ms)).with_easing(ease_out_quint),
        move |el, delta| {
            let t = ((delta - delay_frac) / (1.0 - delay_frac)).clamp(0.0, 1.0);
            if t >= 1.0 {
                el
            } else {
                el.opacity(t).mt(px(6.0 * (1.0 - t)))
            }
        },
    )
    .into_any_element()
}

/// Smooth Apple-style toast viewport entrance (opacity + subtle 10px slide from bottom).
///
/// Returns `el` untouched when reduced motion is active.
pub fn animate_toast_slide_in<E>(el: E, id: impl Into<ElementId>, cx: &App) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    animate_slide_fade_in(el, id, OVERLAY_ENTER_MS, 10.0, cx)
}

/// Crisp active-indicator entrance for rail and tab selection highlights.
///
/// Returns `el` untouched when reduced motion is active.
pub fn animate_active_indicator<E>(el: E, id: impl Into<ElementId>, cx: &App) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    animate_fade_in(el, id, HOVER_FEEDBACK_MS, cx)
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

    #[test]
    fn dark_and_light_chrome_themes_follow_surface_ladder() {
        let dark = dark_chrome_theme();
        assert_eq!(dark.id.as_ref(), DARK_THEME_ID);
        assert_eq!(dark.appearance, Appearance::Dark);
        assert_eq!(
            dark.colors.background,
            Hsla::from(rgb_hsla(0x17, 0x18, 0x1C))
        );
        assert_eq!(
            dark.colors.surface.background,
            Hsla::from(rgb_hsla(0x1F, 0x20, 0x25))
        );
        assert_eq!(
            dark.colors.overlay.background,
            Hsla::from(rgb_hsla(0x25, 0x27, 0x2D))
        );
        assert_eq!(
            dark.colors.border,
            Hsla::from(rgba_hsla(255, 255, 255, 0.08))
        );
        assert_eq!(dark.colors.backdrop, Hsla::from(rgba_hsla(0, 0, 0, 0.55)));

        let light = light_chrome_theme();
        assert_eq!(light.id.as_ref(), LIGHT_THEME_ID);
        assert_eq!(light.appearance, Appearance::Light);
        assert_eq!(
            light.colors.background,
            Hsla::from(rgb_hsla(0xF3, 0xF4, 0xF6))
        );
        assert_eq!(
            light.colors.surface.background,
            Hsla::from(rgb_hsla(0xF8, 0xF9, 0xFB))
        );
        assert_eq!(
            light.colors.overlay.background,
            Hsla::from(rgb_hsla(0xFF, 0xFF, 0xFF))
        );
        assert_eq!(light.colors.border, Hsla::from(rgba_hsla(0, 0, 0, 0.08)));
        assert_eq!(
            light.colors.backdrop,
            Hsla::from(rgba_hsla(15, 23, 42, 0.28))
        );
    }

    #[test]
    fn apple_easing_curves_are_monotonic_and_bounded() {
        for curve in [ease_out_quad, ease_out_cubic, ease_out_quint] {
            assert_eq!(curve(-0.5), 0.0);
            assert_eq!(curve(0.0), 0.0);
            assert_eq!(curve(1.0), 1.0);
            assert_eq!(curve(1.5), 1.0);
            assert!(curve(0.5) > 0.5);
        }
    }

    #[test]
    fn toast_opaque_theme_stays_opaque_across_all_backdrops_and_modes() {
        let modes = [
            ThemeMode::Light,
            ThemeMode::Dark,
            ThemeMode::HighContrastLight,
            ThemeMode::HighContrastDark,
        ];
        let backdrops = [
            WindowBackdropPreference::Opaque,
            WindowBackdropPreference::GaussianBlur,
            WindowBackdropPreference::Mica,
            WindowBackdropPreference::MicaAlt,
        ];

        for mode in modes {
            for backdrop in backdrops {
                let chrome = chrome_theme_with_backdrop(mode, backdrop);
                if backdrop == WindowBackdropPreference::Opaque {
                    assert!(
                        (chrome.colors.surface.background.a - 1.0).abs() < f32::EPSILON,
                        "opaque backdrop should keep surface opaque for {mode:?}"
                    );
                } else {
                    assert!(
                        chrome.colors.surface.background.a < 1.0,
                        "translucent backdrop {backdrop:?} should preserve panel translucency for {mode:?}"
                    );
                }
                assert!(
                    (chrome.colors.overlay.background.a - 1.0).abs() < f32::EPSILON,
                    "overlay.background must stay opaque for {mode:?} / {backdrop:?}"
                );

                let toast = toast_opaque_theme(&chrome);
                assert!(
                    (toast.colors.surface.background.a - 1.0).abs() < f32::EPSILON,
                    "toast surface.background must be opaque for {mode:?} / {backdrop:?}"
                );
                assert_eq!(
                    toast.colors.surface.background,
                    with_alpha(chrome.colors.overlay.background, 1.0),
                    "toast surface.background should match opaque overlay.background for {mode:?} / {backdrop:?}"
                );
                assert!(
                    (toast.colors.overlay.background.a - 1.0).abs() < f32::EPSILON,
                    "toast overlay.background must be opaque for {mode:?} / {backdrop:?}"
                );
                assert!(
                    (toast.colors.background.a - 1.0).abs() < f32::EPSILON,
                    "toast background must be opaque for {mode:?} / {backdrop:?}"
                );
                assert!(
                    (toast.colors.surface_secondary.a - 1.0).abs() < f32::EPSILON,
                    "toast surface_secondary must be opaque for {mode:?} / {backdrop:?}"
                );
                assert!(
                    (toast.colors.surface_tertiary.a - 1.0).abs() < f32::EPSILON,
                    "toast surface_tertiary must be opaque for {mode:?} / {backdrop:?}"
                );
            }
        }
    }
}
