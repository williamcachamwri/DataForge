//! Chrome, content, grid, syntax, and Windows High-Contrast color palettes.
//!
//! Light and dark chrome tokens match TableMinus's reference screenshots
//! (`.github/assets/app-light.png` and `.github/assets/app-dark.png`).

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Gdi::{
    COLOR_BTNTEXT, COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW,
    COLOR_WINDOWTEXT, GetSysColor,
};

/// Linear-ish sRGBA color in `[0.0, 1.0]` ready for Direct2D.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    /// Red channel in `[0.0, 1.0]`.
    pub r: f32,
    /// Green channel in `[0.0, 1.0]`.
    pub g: f32,
    /// Blue channel in `[0.0, 1.0]`.
    pub b: f32,
    /// Alpha channel in `[0.0, 1.0]`.
    pub a: f32,
}

impl Color {
    /// Creates an opaque color from a `0xRRGGBB` hex literal.
    #[must_use]
    pub const fn from_rgb_hex(hex: u32) -> Self {
        let r = ((hex >> 16) & 0xFF) as f32 / 255.0;
        let g = ((hex >> 8) & 0xFF) as f32 / 255.0;
        let b = (hex & 0xFF) as f32 / 255.0;
        Self { r, g, b, a: 1.0 }
    }

    /// Creates a color from a `0xRRGGBBAA` hex literal.
    #[must_use]
    pub const fn from_rgba_hex(hex: u32) -> Self {
        let r = ((hex >> 24) & 0xFF) as f32 / 255.0;
        let g = ((hex >> 16) & 0xFF) as f32 / 255.0;
        let b = ((hex >> 8) & 0xFF) as f32 / 255.0;
        let a = (hex & 0xFF) as f32 / 255.0;
        Self { r, g, b, a }
    }

    /// Creates a color from a Win32 `COLORREF` (`0x00BBGGRR`).
    #[must_use]
    pub const fn from_colorref(colorref: u32) -> Self {
        let r = (colorref & 0xFF) as f32 / 255.0;
        let g = ((colorref >> 8) & 0xFF) as f32 / 255.0;
        let b = ((colorref >> 16) & 0xFF) as f32 / 255.0;
        Self { r, g, b, a: 1.0 }
    }

    /// Returns a copy of this color with the given alpha channel.
    #[must_use]
    pub const fn with_alpha(self, alpha: f32) -> Self {
        Self { a: alpha, ..self }
    }

    /// Converts to a Direct2D `D2D1_COLOR_F`.
    #[must_use]
    pub const fn to_d2d(self) -> D2D1_COLOR_F {
        D2D1_COLOR_F {
            r: self.r,
            g: self.g,
            b: self.b,
            a: self.a,
        }
    }

    /// Quantizes the color to packed `0xRRGGBBAA` for brush cache keying.
    #[must_use]
    pub fn to_packed_rgba(self) -> u32 {
        let r = (self.r.clamp(0.0, 1.0) * 255.0).round() as u32;
        let g = (self.g.clamp(0.0, 1.0) * 255.0).round() as u32;
        let b = (self.b.clamp(0.0, 1.0) * 255.0).round() as u32;
        let a = (self.a.clamp(0.0, 1.0) * 255.0).round() as u32;
        (r << 24) | (g << 16) | (b << 8) | a
    }

    /// Computes relative luminance (WCAG) for contrast checks.
    #[must_use]
    pub fn relative_luminance(self) -> f32 {
        0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b
    }
}

/// Active theme variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeMode {
    /// Standard light theme.
    Light,
    /// Standard dark theme.
    Dark,
    /// Windows High-Contrast light theme.
    HighContrastLight,
    /// Windows High-Contrast dark theme.
    HighContrastDark,
}

impl ThemeMode {
    /// Returns `true` if this mode is one of the high-contrast variants.
    #[must_use]
    pub const fn is_high_contrast(self) -> bool {
        matches!(self, Self::HighContrastLight | Self::HighContrastDark)
    }

    /// Returns `true` if this mode has a dark surface background.
    #[must_use]
    pub const fn is_dark(self) -> bool {
        matches!(self, Self::Dark | Self::HighContrastDark)
    }
}

/// Window chrome colors (sidebar, toolbar, tabs, status bar, borders).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChromePalette {
    /// Sidebar background (`#fafafa` light, `#1c1c24` dark).
    pub sidebar_bg: Color,
    /// Toolbar background (`#f6f6f6` light, `#1f202b` dark).
    pub toolbar_bg: Color,
    /// Inactive tab background (`#f1f1f1` light, `#1e1f2b` dark).
    pub tab_inactive_bg: Color,
    /// Active tab background (`#fafaf9` light, `#1c1c26` dark).
    pub tab_active_bg: Color,
    /// Status bar background (`#edeff0` light, `#272a36` dark).
    pub status_bar_bg: Color,
    /// Subtle divider / hairline border between chrome regions.
    pub border: Color,
    /// Chrome primary text.
    pub text: Color,
    /// Chrome muted / secondary text.
    pub text_muted: Color,
    /// Active selection / hover pill in chrome.
    pub hover_bg: Color,
    /// Focus border accent.
    pub focus_border: Color,
}

/// Content area colors shared by editors, grids, and popups.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContentPalette {
    /// Main content surface background.
    pub bg: Color,
    /// Elevated popup / card background.
    pub surface_elevated: Color,
    /// Primary content text.
    pub text: Color,
    /// Secondary / muted content text.
    pub text_muted: Color,
    /// Primary accent color.
    pub accent: Color,
    /// Text drawn on top of `accent`.
    pub on_accent: Color,
    /// Selection fill.
    pub selection_bg: Color,
    /// Outer / inner focus border color.
    pub focus_border: Color,
    /// Grid / card border color.
    pub border: Color,
}

/// Data grid specific semantic colors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridPalette {
    /// Header row background.
    pub header_bg: Color,
    /// Header row text.
    pub header_text: Color,
    /// Alternating even/odd row tint.
    pub row_alt_bg: Color,
    /// Row-number (`#`) gutter background.
    pub gutter_bg: Color,
    /// Row-number (`#`) gutter text.
    pub gutter_text: Color,
    /// Frozen column divider line.
    pub frozen_divider: Color,
    /// `NULL` badge/text color.
    pub null_text: Color,
    /// `DEFAULT` badge/text color.
    pub default_text: Color,
    /// Foreign-key arrow badge tint.
    pub fk_badge: Color,
    /// Inserted row background tint.
    pub row_inserted_bg: Color,
    /// Modified row background tint.
    pub row_modified_bg: Color,
    /// Deleted row background tint.
    pub row_deleted_bg: Color,
}

/// SQL syntax highlighting colors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyntaxPalette {
    /// SQL keywords (`SELECT`, `FROM`, `WHERE`, ...).
    pub keyword: Color,
    /// Table / column identifiers.
    pub identifier: Color,
    /// String literals.
    pub string_literal: Color,
    /// Numeric literals.
    pub number: Color,
    /// Line and block comments.
    pub comment: Color,
    /// Operators (`=`, `<>`, `+`, ...).
    pub operator: Color,
    /// Punctuation (`,`, `;`, `(`, `)`).
    pub punctuation: Color,
    /// Built-in SQL functions (`COUNT`, `COALESCE`, ...).
    pub function: Color,
    /// SQL type names (`INT`, `TEXT`, `TIMESTAMPTZ`, ...).
    pub type_name: Color,
    /// Gutter active line number color.
    pub active_line_number: Color,
    /// Current line background highlight.
    pub current_line_bg: Color,
}

/// Complete resolved theme palette.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemePalette {
    /// Active theme mode.
    pub mode: ThemeMode,
    /// Chrome surface tokens.
    pub chrome: ChromePalette,
    /// Content surface tokens.
    pub content: ContentPalette,
    /// Data grid semantic tokens.
    pub grid: GridPalette,
    /// SQL editor syntax tokens.
    pub syntax: SyntaxPalette,
}

impl ThemePalette {
    /// Resolves a built-in palette for the requested [`ThemeMode`].
    #[must_use]
    pub const fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Light => Self::light(),
            ThemeMode::Dark => Self::dark(),
            ThemeMode::HighContrastLight => Self::high_contrast_light(),
            ThemeMode::HighContrastDark => Self::high_contrast_dark(),
        }
    }

    /// Standard TableMinus light palette.
    #[must_use]
    pub const fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            chrome: ChromePalette {
                sidebar_bg: Color::from_rgb_hex(0xFAFAFA),
                toolbar_bg: Color::from_rgb_hex(0xF6F6F6),
                tab_inactive_bg: Color::from_rgb_hex(0xF1F1F1),
                tab_active_bg: Color::from_rgb_hex(0xFAFAF9),
                status_bar_bg: Color::from_rgb_hex(0xEDEFF0),
                border: Color::from_rgb_hex(0xE1E4E8),
                text: Color::from_rgb_hex(0x1E2029),
                text_muted: Color::from_rgb_hex(0x6E7781),
                hover_bg: Color::from_rgb_hex(0xEAECEF),
                focus_border: Color::from_rgb_hex(0x0969DA),
            },
            content: ContentPalette {
                bg: Color::from_rgb_hex(0xFFFFFF),
                surface_elevated: Color::from_rgb_hex(0xFFFFFF),
                text: Color::from_rgb_hex(0x1E2029),
                text_muted: Color::from_rgb_hex(0x6E7781),
                accent: Color::from_rgb_hex(0x0969DA),
                on_accent: Color::from_rgb_hex(0xFFFFFF),
                selection_bg: Color::from_rgba_hex(0x0969DA26),
                focus_border: Color::from_rgb_hex(0x0969DA),
                border: Color::from_rgb_hex(0xE5E7EB),
            },
            grid: GridPalette {
                header_bg: Color::from_rgb_hex(0xF6F8FA),
                header_text: Color::from_rgb_hex(0x24292F),
                row_alt_bg: Color::from_rgb_hex(0xF9FAFB),
                gutter_bg: Color::from_rgb_hex(0xF6F8FA),
                gutter_text: Color::from_rgb_hex(0x8C959F),
                frozen_divider: Color::from_rgb_hex(0xCBD5E1),
                null_text: Color::from_rgb_hex(0x8C959F),
                default_text: Color::from_rgb_hex(0x0969DA),
                fk_badge: Color::from_rgb_hex(0x0969DA),
                row_inserted_bg: Color::from_rgba_hex(0x2DA44E1F),
                row_modified_bg: Color::from_rgba_hex(0xBF87001F),
                row_deleted_bg: Color::from_rgba_hex(0xCF222E1F),
            },
            syntax: SyntaxPalette {
                keyword: Color::from_rgb_hex(0xCF222E),
                identifier: Color::from_rgb_hex(0x1E2029),
                string_literal: Color::from_rgb_hex(0x0A3069),
                number: Color::from_rgb_hex(0x0550AE),
                comment: Color::from_rgb_hex(0x6E7781),
                operator: Color::from_rgb_hex(0x24292F),
                punctuation: Color::from_rgb_hex(0x57606A),
                function: Color::from_rgb_hex(0x8250DF),
                type_name: Color::from_rgb_hex(0x953800),
                active_line_number: Color::from_rgb_hex(0x1E2029),
                current_line_bg: Color::from_rgba_hex(0x0969DA0D),
            },
        }
    }

    /// Standard TableMinus dark palette.
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            chrome: ChromePalette {
                sidebar_bg: Color::from_rgb_hex(0x1C1C24),
                toolbar_bg: Color::from_rgb_hex(0x1F202B),
                tab_inactive_bg: Color::from_rgb_hex(0x1E1F2B),
                tab_active_bg: Color::from_rgb_hex(0x1C1C26),
                status_bar_bg: Color::from_rgb_hex(0x272A36),
                border: Color::from_rgb_hex(0x2E3140),
                text: Color::from_rgb_hex(0xE6EDF3),
                text_muted: Color::from_rgb_hex(0x8B949E),
                hover_bg: Color::from_rgb_hex(0x2A2D3D),
                focus_border: Color::from_rgb_hex(0x58A6FF),
            },
            content: ContentPalette {
                bg: Color::from_rgb_hex(0x16161D),
                surface_elevated: Color::from_rgb_hex(0x1F202B),
                text: Color::from_rgb_hex(0xE6EDF3),
                text_muted: Color::from_rgb_hex(0x8B949E),
                accent: Color::from_rgb_hex(0x58A6FF),
                on_accent: Color::from_rgb_hex(0x0D1117),
                selection_bg: Color::from_rgba_hex(0x58A6FF33),
                focus_border: Color::from_rgb_hex(0x58A6FF),
                border: Color::from_rgb_hex(0x2E3140),
            },
            grid: GridPalette {
                header_bg: Color::from_rgb_hex(0x1C1D28),
                header_text: Color::from_rgb_hex(0xE6EDF3),
                row_alt_bg: Color::from_rgb_hex(0x191A23),
                gutter_bg: Color::from_rgb_hex(0x1C1D28),
                gutter_text: Color::from_rgb_hex(0x6E7681),
                frozen_divider: Color::from_rgb_hex(0x444A60),
                null_text: Color::from_rgb_hex(0x6E7681),
                default_text: Color::from_rgb_hex(0x58A6FF),
                fk_badge: Color::from_rgb_hex(0x58A6FF),
                row_inserted_bg: Color::from_rgba_hex(0x2EA04329),
                row_modified_bg: Color::from_rgba_hex(0xD2992229),
                row_deleted_bg: Color::from_rgba_hex(0xF8514929),
            },
            syntax: SyntaxPalette {
                keyword: Color::from_rgb_hex(0xFF7B72),
                identifier: Color::from_rgb_hex(0xE6EDF3),
                string_literal: Color::from_rgb_hex(0xA5D6FF),
                number: Color::from_rgb_hex(0x79C0FF),
                comment: Color::from_rgb_hex(0x8B949E),
                operator: Color::from_rgb_hex(0xE6EDF3),
                punctuation: Color::from_rgb_hex(0x8B949E),
                function: Color::from_rgb_hex(0xD2A8FF),
                type_name: Color::from_rgb_hex(0xFFA657),
                active_line_number: Color::from_rgb_hex(0xE6EDF3),
                current_line_bg: Color::from_rgba_hex(0x58A6FF14),
            },
        }
    }

    /// Deterministic Windows High-Contrast light palette.
    #[must_use]
    pub const fn high_contrast_light() -> Self {
        Self::from_high_contrast_colors(
            ThemeMode::HighContrastLight,
            Color::from_rgb_hex(0xFFFFFF),
            Color::from_rgb_hex(0x000000),
            Color::from_rgb_hex(0x37006E),
            Color::from_rgb_hex(0xFFFFFF),
            Color::from_rgb_hex(0x600000),
        )
    }

    /// Deterministic Windows High-Contrast dark palette.
    #[must_use]
    pub const fn high_contrast_dark() -> Self {
        Self::from_high_contrast_colors(
            ThemeMode::HighContrastDark,
            Color::from_rgb_hex(0x000000),
            Color::from_rgb_hex(0xFFFFFF),
            Color::from_rgb_hex(0x1AEBFF),
            Color::from_rgb_hex(0x000000),
            Color::from_rgb_hex(0x3FF23F),
        )
    }

    /// Builds a High-Contrast palette from explicit system colors.
    #[must_use]
    pub const fn from_high_contrast_colors(
        mode: ThemeMode,
        window_bg: Color,
        window_text: Color,
        highlight_bg: Color,
        highlight_text: Color,
        muted_text: Color,
    ) -> Self {
        Self {
            mode,
            chrome: ChromePalette {
                sidebar_bg: window_bg,
                toolbar_bg: window_bg,
                tab_inactive_bg: window_bg,
                tab_active_bg: highlight_bg,
                status_bar_bg: window_bg,
                border: window_text,
                text: window_text,
                text_muted: muted_text,
                hover_bg: highlight_bg,
                focus_border: highlight_bg,
            },
            content: ContentPalette {
                bg: window_bg,
                surface_elevated: window_bg,
                text: window_text,
                text_muted: muted_text,
                accent: highlight_bg,
                on_accent: highlight_text,
                selection_bg: highlight_bg,
                focus_border: highlight_bg,
                border: window_text,
            },
            grid: GridPalette {
                header_bg: window_bg,
                header_text: window_text,
                row_alt_bg: window_bg,
                gutter_bg: window_bg,
                gutter_text: muted_text,
                frozen_divider: window_text,
                null_text: muted_text,
                default_text: highlight_bg,
                fk_badge: highlight_bg,
                row_inserted_bg: window_bg,
                row_modified_bg: window_bg,
                row_deleted_bg: window_bg,
            },
            syntax: SyntaxPalette {
                keyword: highlight_bg,
                identifier: window_text,
                string_literal: muted_text,
                number: highlight_bg,
                comment: muted_text,
                operator: window_text,
                punctuation: window_text,
                function: highlight_bg,
                type_name: muted_text,
                active_line_number: window_text,
                current_line_bg: window_bg,
            },
        }
    }

    /// Queries Windows `GetSysColor` to build a live High-Contrast palette.
    #[must_use]
    pub fn from_system_high_contrast() -> Self {
        // SAFETY: `GetSysColor` is a thread-safe Win32 query taking predefined index constants.
        let (window_bg, window_text, highlight_bg, highlight_text, btn_text, gray_text) = unsafe {
            (
                Color::from_colorref(GetSysColor(COLOR_WINDOW)),
                Color::from_colorref(GetSysColor(COLOR_WINDOWTEXT)),
                Color::from_colorref(GetSysColor(COLOR_HIGHLIGHT)),
                Color::from_colorref(GetSysColor(COLOR_HIGHLIGHTTEXT)),
                Color::from_colorref(GetSysColor(COLOR_BTNTEXT)),
                Color::from_colorref(GetSysColor(COLOR_GRAYTEXT)),
            )
        };
        let mode = if window_bg.relative_luminance() < 0.5 {
            ThemeMode::HighContrastDark
        } else {
            ThemeMode::HighContrastLight
        };
        let text = if window_text == window_bg {
            btn_text
        } else {
            window_text
        };
        Self::from_high_contrast_colors(
            mode,
            window_bg,
            text,
            highlight_bg,
            highlight_text,
            gray_text,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_chrome_tokens_match_spec() {
        let p = ThemePalette::for_mode(ThemeMode::Light);
        assert_eq!(p.chrome.sidebar_bg.to_packed_rgba(), 0xFAFA_FAFF);
        assert_eq!(p.chrome.toolbar_bg.to_packed_rgba(), 0xF6F6_F6FF);
        assert_eq!(p.chrome.tab_inactive_bg.to_packed_rgba(), 0xF1F1_F1FF);
        assert_eq!(p.chrome.tab_active_bg.to_packed_rgba(), 0xFAFA_F9FF);
        assert_eq!(p.chrome.status_bar_bg.to_packed_rgba(), 0xEDEF_F0FF);
        assert!(!p.mode.is_dark());
        assert!(!p.mode.is_high_contrast());
    }

    #[test]
    fn dark_chrome_tokens_match_spec() {
        let p = ThemePalette::for_mode(ThemeMode::Dark);
        assert_eq!(p.chrome.sidebar_bg.to_packed_rgba(), 0x1C1C_24FF);
        assert_eq!(p.chrome.toolbar_bg.to_packed_rgba(), 0x1F20_2BFF);
        assert_eq!(p.chrome.tab_inactive_bg.to_packed_rgba(), 0x1E1F_2BFF);
        assert_eq!(p.chrome.tab_active_bg.to_packed_rgba(), 0x1C1C_26FF);
        assert_eq!(p.chrome.status_bar_bg.to_packed_rgba(), 0x272A_36FF);
        assert!(p.mode.is_dark());
        assert!(!p.mode.is_high_contrast());
    }

    #[test]
    fn high_contrast_modes_and_colorref_conversion() {
        let hc_light = ThemePalette::for_mode(ThemeMode::HighContrastLight);
        let hc_dark = ThemePalette::for_mode(ThemeMode::HighContrastDark);
        assert!(hc_light.mode.is_high_contrast());
        assert!(hc_dark.mode.is_high_contrast());
        assert!(hc_light.content.bg.relative_luminance() > 0.9);
        assert!(hc_dark.content.bg.relative_luminance() < 0.1);

        // Win32 COLORREF is 0x00BBGGRR.
        let c = Color::from_colorref(0x00AA_5511);
        assert_eq!(c.to_packed_rgba(), 0x1155_AAFF);
    }
}
