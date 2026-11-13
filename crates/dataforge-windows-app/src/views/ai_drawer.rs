//! AI Assistant right drawer view.

use dataforge_gfx::Result;
use dataforge_gfx::context::{
    FontWeight, RenderSurface, TextAlign, TextFormatSpec, VerticalAlign, point_f, rect_f,
};
use dataforge_gfx::palette::ThemePalette;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use crate::shell_layout::{Rect, ShellLayout};

/// Message in the AI Assistant drawer transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiMessage {
    pub is_user: bool,
    pub text: String,
}

/// State backing the AI Assistant drawer.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AiDrawerState {
    pub messages: Vec<AiMessage>,
    pub draft_prompt: String,
}

/// Renders the AI Assistant drawer using Direct2D.
pub fn render_ai_drawer(
    surface: &mut RenderSurface,
    layout: &ShellLayout,
    palette: &ThemePalette,
    title: &str,
    state: &AiDrawerState,
) -> Result<()> {
    let r = layout.ai_drawer;
    if r.width <= 0.0 || r.height <= 0.0 {
        return Ok(());
    }

    surface.fill_rect(to_d2d_rect(r), palette.chrome.sidebar_bg)?;
    surface.draw_line(
        point_f(r.x + 0.5, r.y),
        point_f(r.x + 0.5, r.bottom()),
        palette.chrome.border,
        1.0,
    )?;

    let header = Rect::new(r.x + 12.0, r.y + 8.0, (r.width - 24.0).max(0.0), 28.0);
    let header_fmt = TextFormatSpec::new("Segoe UI", 13.0)
        .with_weight(FontWeight::SemiBold)
        .with_align(TextAlign::Leading)
        .with_valign(VerticalAlign::Center);
    surface.draw_text(title, to_d2d_rect(header), &header_fmt, palette.chrome.text)?;

    let input_rect = Rect::new(
        r.x + 12.0,
        (r.bottom() - 44.0).max(header.bottom() + 8.0),
        header.width,
        32.0,
    );
    surface.fill_rounded_rect(to_d2d_rect(input_rect), 6.0, palette.content.bg)?;
    surface.draw_rounded_rect(to_d2d_rect(input_rect), 6.0, palette.chrome.border, 1.0)?;
    let (prompt_label, prompt_color) = if state.draft_prompt.is_empty() {
        ("Ask AI about schema or SQL...", palette.chrome.text_muted)
    } else {
        (state.draft_prompt.as_str(), palette.content.text)
    };
    let prompt_rect = Rect::new(
        input_rect.x + 8.0,
        input_rect.y,
        (input_rect.width - 16.0).max(0.0),
        input_rect.height,
    );
    let fmt = TextFormatSpec::new("Segoe UI", 11.5)
        .with_align(TextAlign::Leading)
        .with_valign(VerticalAlign::Center);
    surface.draw_text(prompt_label, to_d2d_rect(prompt_rect), &fmt, prompt_color)?;

    let mut y = header.bottom() + 8.0;
    for msg in &state.messages {
        if y + 36.0 > input_rect.y {
            break;
        }
        let bubble = Rect::new(r.x + 12.0, y, header.width, 32.0);
        let bg = if msg.is_user {
            palette.content.selection_bg
        } else {
            palette.content.bg
        };
        surface.fill_rounded_rect(to_d2d_rect(bubble), 6.0, bg)?;
        let text_rect = Rect::new(
            bubble.x + 8.0,
            bubble.y,
            (bubble.width - 16.0).max(0.0),
            bubble.height,
        );
        surface.draw_text(
            &msg.text,
            to_d2d_rect(text_rect),
            &fmt,
            palette.content.text,
        )?;
        y += 38.0;
    }

    Ok(())
}

const fn to_d2d_rect(r: Rect) -> D2D_RECT_F {
    rect_f(r.x, r.y, r.right(), r.bottom())
}
