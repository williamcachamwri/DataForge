//! Direct2D view modules for every shell region and workspace screen.

use tableminus_gfx::context::rect_f;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use crate::shell_layout::Rect;

pub mod ai_drawer;
pub mod content;
pub mod inspector;
pub mod library;
pub mod rail;
pub mod settings;
pub mod sidebar;
pub mod status_bar;
pub mod tabs;
pub mod toolbar;

/// Converts a shell layout [`Rect`] (`x, y, width, height`) to a Direct2D [`D2D_RECT_F`].
#[must_use]
pub(crate) const fn to_d2d_rect(r: Rect) -> D2D_RECT_F {
    rect_f(r.x, r.y, r.width, r.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_d2d_rect_does_not_double_offset_origin() {
        let r = to_d2d_rect(Rect::new(56.0, 88.0, 204.0, 28.0));
        assert!((r.left - 56.0).abs() < f32::EPSILON);
        assert!((r.top - 88.0).abs() < f32::EPSILON);
        assert!((r.right - 260.0).abs() < f32::EPSILON);
        assert!((r.bottom - 116.0).abs() < f32::EPSILON);
    }
}
