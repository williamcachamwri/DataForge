# Direct2D & DirectWrite Rendering Pipeline (`dataforge-gfx`)

`crates/dataforge-gfx` renders the entire DataForge workbench directly onto Win32 `HWND`s using hardware-accelerated Direct2D (`ID2D1HwndRenderTarget`) and DirectWrite (`IDWriteFactory`).

## 1. Device Context & Resource Budget (`context.rs`)

- **`D2dContext`**: Owns the single-threaded `ID2D1Factory1` and shared `IDWriteFactory` (`DWRITE_FACTORY_TYPE_SHARED`).
- **`RenderSurface`**: Binds an `ID2D1HwndRenderTarget` (`DXGI_FORMAT_B8G8R8A8_UNORM`, `D2D1_ALPHA_MODE_PREMULTIPLIED`) to a window `HWND`, handling resize, DPI scaling (`BASELINE_DPI = 96.0`), and `D2DERR_RECREATE_TARGET` device-loss recovery.
- **Bounded Memory Discipline (`MemoryBudget`)**: Enforces `DEFAULT_MEMORY_BUDGET_BYTES` (`60 MB`) across cached GPU/CPU resources, capping `RenderSurface` caches at `256` solid-color brushes (`ID2D1SolidColorBrush`) and `64` DirectWrite text formats (`IDWriteTextFormat`).
