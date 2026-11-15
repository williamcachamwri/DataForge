//! Automated screenshot tour: walks every workspace view at several window sizes
//! and writes PNG captures, so layout regressions can be reviewed visually.
//!
//! Enabled by setting the `TABLEMINUS_SCREENSHOT_DIR` environment variable before
//! launching `tableminus.exe` (optionally passing a database file to open). The
//! app captures every step to `<dir>/<size>-<view>.png` and then quits.

use std::path::{Path, PathBuf};
use std::time::Duration;

use herogpui::gpui::{App, AsyncApp, Context, WeakEntity, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap,
    CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC,
    SRCCOPY, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, SWP_NOMOVE, SWP_NOZORDER, SetForegroundWindow, SetWindowPos,
};

use super::TableMinusWorkspace;
use super::shell_state::{WorkspaceSubView, WorkspaceToolView};

/// Environment variable that enables the tour and names the output directory.
pub const SCREENSHOT_DIR_ENV: &str = "TABLEMINUS_SCREENSHOT_DIR";

/// Window sizes (outer, physical pixels) captured for every view.
const SIZES: [(i32, i32); 3] = [(1440, 900), (1100, 720), (900, 600)];
const SETTLE: Duration = Duration::from_millis(900);

#[derive(Clone, Copy)]
enum Step {
    SubView(WorkspaceSubView, &'static str),
    Tool(WorkspaceToolView, &'static str),
}

const STEPS: [Step; 12] = [
    Step::SubView(WorkspaceSubView::SqlEditor, "sql-editor"),
    Step::SubView(WorkspaceSubView::DataGrid, "data-grid"),
    Step::SubView(WorkspaceSubView::Structure, "structure"),
    Step::SubView(WorkspaceSubView::SplitSqlAndGrid, "split"),
    Step::Tool(WorkspaceToolView::SchemaDesigner, "schema-designer"),
    Step::Tool(WorkspaceToolView::ImportWizard, "import-wizard"),
    Step::Tool(WorkspaceToolView::ErDiagram, "er-diagram"),
    Step::Tool(WorkspaceToolView::QueryBuilder, "query-builder"),
    Step::Tool(WorkspaceToolView::Compare, "compare"),
    Step::Tool(WorkspaceToolView::Admin, "admin"),
    Step::Tool(WorkspaceToolView::MockGenerator, "mock-generator"),
    Step::Tool(WorkspaceToolView::DataTransfer, "data-transfer"),
];

/// Returns the configured output directory when the tour is enabled.
#[must_use]
pub fn output_dir() -> Option<PathBuf> {
    std::env::var_os(SCREENSHOT_DIR_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Starts the tour for `window`, quitting the app when every capture is written.
pub fn start(
    dir: PathBuf,
    window: &Window,
    entity: &herogpui::gpui::Entity<TableMinusWorkspace>,
    cx: &mut App,
) {
    let Some(hwnd) = hwnd_of(window) else {
        tracing::error!("screenshot tour: no Win32 window handle");
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    entity.update(cx, |_, cx: &mut Context<TableMinusWorkspace>| {
        cx.spawn(
            async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
                cx.background_executor().timer(SETTLE * 2).await;
                for (w, h) in SIZES {
                    // SAFETY: valid HWND owned by this process; resizing is idempotent.
                    unsafe {
                        let _ = SetWindowPos(hwnd, None, 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER);
                    }
                    for step in STEPS {
                        let label = match step {
                            Step::SubView(_, n) | Step::Tool(_, n) => n,
                        };
                        let applied = this.update(cx, |ws, cx| {
                            match step {
                                Step::SubView(v, _) => {
                                    ws.workspace_pane.close_tool();
                                    ws.workspace_pane.sub_view = v;
                                }
                                Step::Tool(t, _) => ws.workspace_pane.open_tool(t),
                            }
                            cx.notify();
                        });
                        if applied.is_err() {
                            return;
                        }
                        cx.background_executor().timer(SETTLE).await;
                        let path = dir.join(format!("{w}x{h}-{label}.png"));
                        if let Err(err) = capture_window(hwnd, &path) {
                            tracing::error!(error = %err, "screenshot capture failed");
                        }
                    }
                }
                cx.update(|cx| cx.quit());
            },
        )
        .detach();
    });
}

fn hwnd_of(window: &Window) -> Option<HWND> {
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut std::ffi::c_void)),
        _ => None,
    }
}

fn capture_window(hwnd: HWND, path: &Path) -> Result<(), String> {
    // SAFETY: GDI calls on handles created and released within this function.
    unsafe {
        let _ = SetForegroundWindow(hwnd);
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).map_err(|e| e.to_string())?;
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        if w <= 0 || h <= 0 {
            return Err("empty window rect".into());
        }
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        let blt = BitBlt(
            mem,
            0,
            0,
            w,
            h,
            Some(screen),
            rect.left,
            rect.top,
            SRCCOPY | CAPTUREBLT,
        );
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bgra = vec![0u8; (w as usize) * (h as usize) * 4];
        let rows = GetDIBits(
            mem,
            bmp,
            0,
            h as u32,
            Some(bgra.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        blt.map_err(|e| e.to_string())?;
        if rows == 0 {
            return Err("GetDIBits returned 0 rows".into());
        }
        let mut rgb = Vec::with_capacity((w as usize) * (h as usize) * 3);
        for px in bgra.as_chunks::<4>().0 {
            rgb.extend_from_slice(&[px[2], px[1], px[0]]);
        }
        std::fs::write(path, encode_png(w as u32, h as u32, &rgb)).map_err(|e| e.to_string())
    }
}

fn encode_png(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    let stride = width as usize * 3;
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    for row in rgb.chunks_exact(stride) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::fast());
    let _ = enc.write_all(&raw);
    let idat = enc.finish().unwrap_or_default();

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    write_chunk(&mut out, b"IHDR", &ihdr);
    write_chunk(&mut out, b"IDAT", &idat);
    write_chunk(&mut out, b"IEND", &[]);
    out
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = flate2::Crc::new();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(&crc.sum().to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::encode_png;

    #[test]
    fn png_has_signature_and_chunks() {
        let png = encode_png(2, 2, &[255; 12]);
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert!(png.windows(4).any(|w| w == b"IHDR"));
        assert!(png.windows(4).any(|w| w == b"IDAT"));
        assert!(png.ends_with(&[0xAE, 0x42, 0x60, 0x82]));
    }
}
