//! Pure Win32 + Direct2D `MainWindow` with DWM dark titlebar, live theme/DPI
//! updates, keyboard/mouse dispatch, Safe Mode close guard, and UIA hook.

use std::ffi::c_void;
use std::sync::Arc;

use dataforge_gfx::context::{D2dContext, RenderSurface};
use dataforge_windows_platform::detect_theme_or_default;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
    GetClientRect, GetMessageW, GetWindowLongPtrW, IDC_ARROW, LoadCursorW, MSG, PostQuitMessage,
    RegisterClassExW, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER, SetForegroundWindow,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, WM_CLOSE, WM_DESTROY,
    WM_DPICHANGED, WM_GETOBJECT, WM_KEYDOWN, WM_LBUTTONDOWN, WM_NCCREATE, WM_PAINT,
    WM_SETTINGCHANGE, WM_SIZE, WM_THEMECHANGED, WNDCLASSEXW, WS_EX_APPWINDOW, WS_OVERLAPPEDWINDOW,
};
use windows::core::PCWSTR;

use crate::accessibility::{Modifiers, handle_wm_getobject, translate_key};
use crate::app::AppController;
use crate::branding::apply_window_icons;
use crate::dialogs::{DialogKind, DialogOutcome, show_native_dialog};
use crate::error::{AppError, Result};
use crate::ipc::{IpcInbox, WM_APP_IPC_TARGETS};

const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;

struct WindowState {
    controller: AppController,
    ctx: D2dContext,
    surface: RenderSurface,
    ipc_inbox: Arc<IpcInbox>,
}

/// Pure Win32 + Direct2D desktop window for DataForge.
pub struct MainWindow {
    hwnd: HWND,
    _state: Box<WindowState>,
}

impl MainWindow {
    /// Creates, shows, and enters the Win32 message loop for `controller`.
    pub fn run(controller: AppController, ipc_inbox: Arc<IpcInbox>) -> Result<()> {
        let window = Self::create(controller, ipc_inbox)?;
        window.show();
        run_message_loop();
        Ok(())
    }

    /// Creates the Win32 `MainWindow` and its Direct2D `RenderSurface`.
    pub fn create(controller: AppController, ipc_inbox: Arc<IpcInbox>) -> Result<Self> {
        let class_name = to_wide_null("DataForgeMainWindow");
        let title = to_wide_null(controller.strings.app_title);

        // SAFETY: `GetModuleHandleW(None)` returns the current executable's HINSTANCE.
        let hinstance = unsafe { GetModuleHandleW(None) }.map_err(|err| AppError::Win32 {
            operation: "GetModuleHandleW",
            source: std::io::Error::from_raw_os_error(err.code().0),
        })?;

        // SAFETY: `IDC_ARROW` is a standard system cursor resource.
        let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default();

        let wc = WNDCLASSEXW {
            cbSize: u32::try_from(std::mem::size_of::<WNDCLASSEXW>()).unwrap_or(0),
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance.into(),
            hCursor: cursor,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };

        // SAFETY: `wc` is a valid `WNDCLASSEXW` with a static `wnd_proc` and NUL-terminated class name.
        let atom = unsafe { RegisterClassExW(&wc) };
        if atom == 0 {
            let last = std::io::Error::last_os_error();
            if last.raw_os_error() != Some(1410) {
                return Err(AppError::Win32 {
                    operation: "RegisterClassExW",
                    source: last,
                });
            }
        }

        let width = controller.settings.window.width as i32;
        let height = controller.settings.window.height as i32;

        // SAFETY: `class_name` and `title` are NUL-terminated UTF-16 strings; `wnd_proc` handles `WM_NCCREATE`.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_APPWINDOW,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                width.max(640),
                height.max(480),
                None,
                None,
                Some(hinstance.into()),
                None,
            )
        }
        .map_err(|err| AppError::Win32 {
            operation: "CreateWindowExW",
            source: std::io::Error::from_raw_os_error(err.code().0),
        })?;

        let ctx = D2dContext::new()?;
        let (client_w, client_h) = client_pixel_size(hwnd);
        // SAFETY: `hwnd` is a valid window handle.
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let dpi_f32 = if dpi > 0 { dpi as f32 } else { 96.0 };
        let surface = ctx.create_hwnd_surface(hwnd, client_w, client_h, dpi_f32)?;

        let dark_titlebar = controller.uses_dark_titlebar();
        apply_dwm_dark_mode(hwnd, dark_titlebar);
        apply_window_icons(hwnd);
        ipc_inbox.attach_hwnd(hwnd);

        let mut state = Box::new(WindowState {
            controller,
            ctx,
            surface,
            ipc_inbox,
        });

        let ptr = (&raw mut *state) as isize;
        // SAFETY: `hwnd` is owned by this struct and `state` outlives the window via `_state`.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr);
        }

        Ok(Self {
            hwnd,
            _state: state,
        })
    }

    /// Shows the window on screen.
    pub fn show(&self) {
        // SAFETY: `self.hwnd` is a valid created window.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
        }
    }

    /// Returns the underlying Win32 `HWND`.
    #[must_use]
    pub const fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

fn run_message_loop() {
    let mut msg = MSG::default();
    // SAFETY: Standard Win32 message loop for the current thread.
    unsafe {
        while GetMessageW(&raw mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&raw const msg);
            DispatchMessageW(&raw const msg);
        }
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        // SAFETY: Forward `WM_NCCREATE` to `DefWindowProcW`.
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }

    // SAFETY: `GWLP_USERDATA` holds either 0 or a valid `*mut WindowState`.
    let state_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut WindowState;
    if state_ptr.is_null() {
        // SAFETY: No `WindowState` attached yet; delegate to `DefWindowProcW`.
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }

    // SAFETY: `state_ptr` is valid for the lifetime of `MainWindow` until `WM_DESTROY`.
    let state = unsafe { &mut *state_ptr };
    dispatch_message(hwnd, state, msg, wparam, lparam)
}

fn dispatch_message(
    hwnd: HWND,
    state: &mut WindowState,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_SIZE => {
            let width = u32::from((lparam.0 & 0xFFFF) as u16);
            let height = u32::from(((lparam.0 >> 16) & 0xFFFF) as u16);
            if width > 0 && height > 0 {
                let _ = state.surface.resize(width, height);
            }
            invalidate(hwnd);
            LRESULT(0)
        }
        WM_DPICHANGED => {
            handle_dpi_changed(hwnd, state, wparam, lparam);
            LRESULT(0)
        }
        WM_SETTINGCHANGE | WM_THEMECHANGED => {
            let detected = detect_theme_or_default();
            state.controller.on_system_theme_changed(detected);
            apply_dwm_dark_mode(hwnd, state.controller.uses_dark_titlebar());
            invalidate(hwnd);
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            // SAFETY: `hwnd` and `ps` are valid for `BeginPaint`/`EndPaint`.
            unsafe {
                let _ = BeginPaint(hwnd, &raw mut ps);
            }
            let _ = state
                .controller
                .render_frame(&state.ctx, &mut state.surface);
            // SAFETY: Matches `BeginPaint` above.
            unsafe {
                let _ = EndPaint(hwnd, &raw const ps);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vkey = u32::try_from(wparam.0).unwrap_or(0);
            if let Some(cmd) = translate_key(vkey, current_modifiers()) {
                if state.controller.handle_key_command(cmd) {
                    request_close(hwnd, state);
                } else {
                    invalidate(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let px_x = f32::from((lparam.0 & 0xFFFF) as i16);
            let px_y = f32::from(((lparam.0 >> 16) & 0xFFFF) as i16);
            let scale = state.surface.scale_factor().max(1.0);
            let _ = state.controller.handle_click(px_x / scale, px_y / scale);
            invalidate(hwnd);
            LRESULT(0)
        }
        WM_GETOBJECT => match handle_wm_getobject(lparam.0) {
            Some(res) => LRESULT(res),
            None => {
                // SAFETY: Unhandled `WM_GETOBJECT` falls back to `DefWindowProcW`.
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
        },
        WM_APP_IPC_TARGETS => {
            handle_ipc_targets(hwnd, state);
            LRESULT(0)
        }
        WM_CLOSE => {
            request_close(hwnd, state);
            LRESULT(0)
        }
        WM_DESTROY => {
            state.ipc_inbox.detach_hwnd();
            // SAFETY: Clear `GWLP_USERDATA` and quit message loop on window destruction.
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => {
            // SAFETY: Default window procedure for unhandled messages.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
    }
}

fn handle_dpi_changed(hwnd: HWND, state: &mut WindowState, wparam: WPARAM, lparam: LPARAM) {
    let new_dpi = f32::from((wparam.0 & 0xFFFF) as u16);
    if new_dpi > 0.0 {
        let _ = state.surface.set_dpi(new_dpi);
    }
    // SAFETY: `lparam` points to a Win32 `RECT` for `WM_DPICHANGED`.
    let suggested = unsafe { &*(lparam.0 as *const RECT) };
    // SAFETY: `hwnd` and `suggested` are valid for `SetWindowPos`.
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            suggested.left,
            suggested.top,
            suggested.right - suggested.left,
            suggested.bottom - suggested.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    invalidate(hwnd);
}

fn handle_ipc_targets(hwnd: HWND, state: &mut WindowState) {
    let targets = state.ipc_inbox.drain_targets();
    if !targets.is_empty() {
        state.controller.open_forwarded_targets(&targets);
    }
    // SAFETY: Bring existing window to foreground when secondary launch forwards targets.
    unsafe {
        let _ = SetForegroundWindow(hwnd);
    }
    invalidate(hwnd);
}

fn request_close(hwnd: HWND, state: &WindowState) {
    if state.controller.needs_close_guard() {
        let kind = DialogKind::CloseTransactionGuard {
            connection_name: state.controller.toolbar_status.database.clone(),
        };
        let outcome = show_native_dialog(Some(hwnd), &kind, &state.controller.strings);
        if !matches!(outcome, DialogOutcome::Accepted(_)) {
            return;
        }
    }
    // SAFETY: `hwnd` is a valid window owned by this thread.
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}

fn invalidate(hwnd: HWND) {
    // SAFETY: `hwnd` is valid; `None` invalidates the entire client area.
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn client_pixel_size(hwnd: HWND) -> (u32, u32) {
    let mut rect = RECT::default();
    // SAFETY: `hwnd` and `rect` are valid.
    let ok = unsafe { GetClientRect(hwnd, &raw mut rect) }.is_ok();
    if ok {
        let w = u32::try_from(rect.right - rect.left).unwrap_or(1280).max(1);
        let h = u32::try_from(rect.bottom - rect.top).unwrap_or(800).max(1);
        (w, h)
    } else {
        (1280, 800)
    }
}

fn current_modifiers() -> Modifiers {
    // SAFETY: `GetKeyState` reads synchronous key state for standard virtual keys.
    unsafe {
        Modifiers {
            ctrl: GetKeyState(i32::from(VK_CONTROL.0)) < 0,
            shift: GetKeyState(i32::from(VK_SHIFT.0)) < 0,
            alt: GetKeyState(i32::from(VK_MENU.0)) < 0,
        }
    }
}

/// Applies `DwmSetWindowAttribute(DWMWA_USE_IMMERSIVE_DARK_MODE)` dynamically from `dwmapi.dll`.
fn apply_dwm_dark_mode(hwnd: HWND, dark: bool) {
    type DwmSetWindowAttributeFn = unsafe extern "system" fn(HWND, u32, *const c_void, u32) -> i32;

    let dll = to_wide_null("dwmapi.dll");
    // SAFETY: `dll` is NUL-terminated; `LoadLibraryW` loads `dwmapi.dll` if present.
    let Ok(module) = (unsafe { LoadLibraryW(PCWSTR(dll.as_ptr())) }) else {
        return;
    };
    let proc_name = c"DwmSetWindowAttribute";
    // SAFETY: `module` is a valid DLL handle and `proc_name` is a NUL-terminated ASCII name.
    let Some(proc_addr) =
        (unsafe { GetProcAddress(module, windows::core::PCSTR(proc_name.as_ptr().cast())) })
    else {
        return;
    };
    // SAFETY: `DwmSetWindowAttribute` signature matches `DwmSetWindowAttributeFn`.
    let func: DwmSetWindowAttributeFn = unsafe { std::mem::transmute(proc_addr) };
    let enabled: i32 = i32::from(dark);
    // SAFETY: `enabled` is a 4-byte `BOOL` value valid for the call duration.
    let _ = unsafe {
        func(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&raw const enabled).cast(),
            u32::try_from(std::mem::size_of::<i32>()).unwrap_or(4),
        )
    };
}

fn to_wide_null(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
