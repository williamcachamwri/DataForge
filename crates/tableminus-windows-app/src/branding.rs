//! Pure-Rust TableMinus Win32 icon fallback using `CreateIconFromResourceEx`
//! and embedded Apple HIG Liquid Glass app logo (`assets/tableminus_logo.png`).

use std::sync::{Arc, OnceLock};

use herogpui::gpui::{Image, ImageFormat};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, HICON, ICON_BIG, ICON_SMALL, LR_DEFAULTCOLOR,
    LookupIconIdFromDirectoryEx, SendMessageW, WM_SETICON,
};

/// Embedded multi-size Apple HIG TableMinus icon (`assets/tableminus.ico`).
pub const TABLEMINUS_ICO_BYTES: &[u8] = include_bytes!("../assets/tableminus.ico");

/// Embedded transparent-background Apple HIG Liquid Glass logo (`assets/tableminus_logo.png`).
pub const TABLEMINUS_LOGO_PNG_BYTES: &[u8] = include_bytes!("../assets/tableminus_logo.png");

/// Returns the cached [`Image`] handle for the embedded TableMinus Apple HIG logo.
#[must_use]
pub fn tableminus_logo_image() -> Arc<Image> {
    static LOGO: OnceLock<Arc<Image>> = OnceLock::new();
    Arc::clone(LOGO.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            TABLEMINUS_LOGO_PNG_BYTES.to_vec(),
        ))
    }))
}

/// Creates a Win32 `HICON` of `desired_px x desired_px` from [`TABLEMINUS_ICO_BYTES`].
#[must_use]
pub fn create_tableminus_hicon(desired_px: i32) -> Option<HICON> {
    if TABLEMINUS_ICO_BYTES.len() < 22 {
        return None;
    }
    // SAFETY: `TABLEMINUS_ICO_BYTES` is a valid static `.ico` directory buffer.
    let offset = unsafe {
        LookupIconIdFromDirectoryEx(
            TABLEMINUS_ICO_BYTES.as_ptr(),
            true,
            desired_px,
            desired_px,
            LR_DEFAULTCOLOR,
        )
    };
    if offset <= 0 {
        return None;
    }
    let start = usize::try_from(offset).ok()?;
    let slice = TABLEMINUS_ICO_BYTES.get(start..)?;
    // SAFETY: `slice` points into the static icon image payload (PNG or DIB) with known length.
    let icon = unsafe {
        CreateIconFromResourceEx(
            slice,
            true,
            0x0003_0000,
            desired_px,
            desired_px,
            LR_DEFAULTCOLOR,
        )
    }
    .ok()?;
    if icon.is_invalid() { None } else { Some(icon) }
}

/// Applies the TableMinus small (16px) and large (32px) icons to `hwnd` via `WM_SETICON`.
pub fn apply_window_icons(hwnd: HWND) {
    if let Some(small) = create_tableminus_hicon(16) {
        // SAFETY: `hwnd` is a valid window and `small` is a valid `HICON`.
        unsafe {
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_SMALL as usize)),
                Some(LPARAM(small.0 as isize)),
            );
        }
    }
    if let Some(big) = create_tableminus_hicon(32) {
        // SAFETY: `hwnd` is a valid window and `big` is a valid `HICON`.
        unsafe {
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_BIG as usize)),
                Some(LPARAM(big.0 as isize)),
            );
        }
    }
}
