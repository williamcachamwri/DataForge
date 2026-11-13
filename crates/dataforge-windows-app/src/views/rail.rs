//! Collapsible Connection Rail view (Connections Library, open connections with
//! 11 engine glyphs from `dataforge-gfx`, collapse toggle, and Settings).

use dataforge_gfx::Result;
use dataforge_gfx::context::{
    D2dContext, FontWeight, RenderSurface, TextAlign, TextFormatSpec, VerticalAlign, point_f,
    rect_f,
};
use dataforge_gfx::glyphs::EngineKind;
use dataforge_gfx::palette::ThemePalette;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows_numerics::Matrix3x2;

use crate::shell_layout::{Rect, ShellLayout};

/// An open connection entry displayed in the Connection Rail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailConnectionItem {
    pub id: String,
    pub label: String,
    pub engine: EngineKind,
    pub active: bool,
    pub connected: bool,
}

/// Renders the Connection Rail using Direct2D and `dataforge-gfx` engine glyphs.
pub fn render_rail(
    ctx: &D2dContext,
    surface: &mut RenderSurface,
    layout: &ShellLayout,
    palette: &ThemePalette,
    connections: &[RailConnectionItem],
    expanded: bool,
    library_active: bool,
) -> Result<()> {
    let r = layout.rail;
    surface.fill_rect(to_d2d_rect(r), palette.chrome.sidebar_bg)?;
    surface.draw_line(
        point_f(r.right() - 0.5, r.y),
        point_f(r.right() - 0.5, r.bottom()),
        palette.chrome.border,
        1.0,
    )?;

    draw_rail_button(
        surface,
        layout.rail_library_btn,
        palette,
        if expanded { "Library" } else { "Lib" },
        library_active,
    )?;

    for (idx, btn_rect) in layout.rail_connections.iter().enumerate() {
        if let Some(item) = connections.get(idx) {
            draw_connection_item(ctx, surface, *btn_rect, palette, item, expanded)?;
        }
    }

    draw_rail_button(
        surface,
        layout.rail_collapse_btn,
        palette,
        if expanded { "<<" } else { ">>" },
        false,
    )?;
    draw_rail_button(
        surface,
        layout.rail_settings_btn,
        palette,
        if expanded { "Settings" } else { "Cfg" },
        false,
    )?;

    Ok(())
}

fn draw_connection_item(
    ctx: &D2dContext,
    surface: &mut RenderSurface,
    rect: Rect,
    palette: &ThemePalette,
    item: &RailConnectionItem,
    expanded: bool,
) -> Result<()> {
    if item.active {
        surface.fill_rounded_rect(to_d2d_rect(rect), 6.0, palette.chrome.hover_bg)?;
        surface.draw_rounded_rect(to_d2d_rect(rect), 6.0, palette.chrome.focus_border, 1.0)?;
    }

    let glyph = item.engine.glyph()?;
    let geom = glyph.to_d2d_geometry(ctx.d2d_factory(), 16.0)?;
    let glyph_x = if expanded {
        rect.x + 10.0
    } else {
        rect.x + (rect.width - 16.0) * 0.5
    };
    let glyph_y = rect.y + (rect.height - 16.0) * 0.5;
    let transform = Matrix3x2::translation(glyph_x, glyph_y);
    surface.set_transform(&transform);
    let color = if item.connected {
        palette.content.accent
    } else {
        palette.chrome.text_muted
    };
    surface.fill_geometry(&geom, color)?;
    surface.reset_transform();

    if expanded {
        let text_rect = Rect::new(
            glyph_x + 22.0,
            rect.y,
            (rect.right() - glyph_x - 26.0).max(0.0),
            rect.height,
        );
        let fmt = TextFormatSpec::new("Segoe UI", 12.0)
            .with_weight(if item.active {
                FontWeight::SemiBold
            } else {
                FontWeight::Normal
            })
            .with_align(TextAlign::Leading)
            .with_valign(VerticalAlign::Center);
        surface.draw_text(
            &item.label,
            to_d2d_rect(text_rect),
            &fmt,
            palette.chrome.text,
        )?;
    }

    Ok(())
}

fn draw_rail_button(
    surface: &mut RenderSurface,
    rect: Rect,
    palette: &ThemePalette,
    label: &str,
    active: bool,
) -> Result<()> {
    if active {
        surface.fill_rounded_rect(to_d2d_rect(rect), 6.0, palette.chrome.hover_bg)?;
    }
    let fmt = TextFormatSpec::new("Segoe UI", 11.5)
        .with_weight(FontWeight::Medium)
        .with_align(TextAlign::Center)
        .with_valign(VerticalAlign::Center);
    surface.draw_text(label, to_d2d_rect(rect), &fmt, palette.chrome.text)?;
    Ok(())
}

const fn to_d2d_rect(r: Rect) -> D2D_RECT_F {
    rect_f(r.x, r.y, r.right(), r.bottom())
}
