//! Pure-Rust DataForge Win32 icon fallback using `CreateIconFromResourceEx`.

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, HICON, ICON_BIG, ICON_SMALL, LR_DEFAULTCOLOR,
    LookupIconIdFromDirectoryEx, SendMessageW, WM_SETICON,
};

/// Embedded multi-size geometric DataForge icon (`assets/dataforge.ico`).
pub const DATAFORGE_ICO_BYTES: &[u8] = include_bytes!("../assets/dataforge.ico");

/// Creates a Win32 `HICON` of `desired_px x desired_px` from [`DATAFORGE_ICO_BYTES`].
#[must_use]
pub fn create_dataforge_hicon(desired_px: i32) -> Option<HICON> {
    if DATAFORGE_ICO_BYTES.len() < 22 {
        return None;
    }
    // SAFETY: `DATAFORGE_ICO_BYTES` is a valid static `.ico` directory buffer.
    let offset = unsafe {
        LookupIconIdFromDirectoryEx(
            DATAFORGE_ICO_BYTES.as_ptr(),
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
    let slice = DATAFORGE_ICO_BYTES.get(start..)?;
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

/// Applies the DataForge small (16px) and large (32px) icons to `hwnd` via `WM_SETICON`.
pub fn apply_window_icons(hwnd: HWND) {
    if let Some(small) = create_dataforge_hicon(16) {
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
    if let Some(big) = create_dataforge_hicon(32) {
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
