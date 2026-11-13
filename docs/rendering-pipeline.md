# Direct2D & DirectWrite Rendering Pipeline (`dataforge-gfx`)

`crates/dataforge-gfx` renders the entire DataForge workbench directly onto Win32 `HWND`s using hardware-accelerated Direct2D (`ID2D1HwndRenderTarget`) and DirectWrite (`IDWriteFactory`).

## 1. Device Context & Resource Budget (`context.rs`)

- **`D2dContext`**: Owns the single-threaded `ID2D1Factory1` and shared `IDWriteFactory` (`DWRITE_FACTORY_TYPE_SHARED`).
- **`RenderSurface`**: Binds an `ID2D1HwndRenderTarget` (`DXGI_FORMAT_B8G8R8A8_UNORM`, `D2D1_ALPHA_MODE_PREMULTIPLIED`) to a window `HWND`, handling resize, DPI scaling (`BASELINE_DPI = 96.0`), and `D2DERR_RECREATE_TARGET` device-loss recovery.
- **Bounded Memory Discipline (`MemoryBudget`)**: Enforces `DEFAULT_MEMORY_BUDGET_BYTES` (`60 MB`) across cached GPU/CPU resources, capping `RenderSurface` caches at `256` solid-color brushes (`ID2D1SolidColorBrush`) and `64` DirectWrite text formats (`IDWriteTextFormat`).

## 2. Virtualized Data Grid Rendering (`grid_renderer.rs`)

`DataGridRenderer` draws only the visible row/column slice computed by `GridLayout`:

- **Frozen + Scrollable Panes**: Computes `frozen_pane_width` for leading frozen columns and `scrollable_origin_x` for horizontally scrolled columns, clipping each pane cleanly.
- **Typed Cell Styling**: Differentiates `CellKind` (`Text`, `Number`, `Null`, `Boolean`, `Json`, `Binary`) and `RowState` (`Normal`, `Selected`, `Modified`, `Inserted`, `Deleted`) using `GridPalette`.
- **Hit Testing**: `DataGridRenderer::hit_test` maps viewport `(x, y)` coordinates back to `GridHitTest` targets (column headers, resize handles, row numbers, and data cells) without allocating per-cell widgets.

## 3. SQL Editor Rendering (`editor_renderer.rs`)

`SqlEditorRenderer` pairs DirectWrite `IDWriteTextLayout` with a bounded `LineLayoutCache`:

- **Tokenization (`tokenize_sql_line`)**: Produces `SyntaxSpan` runs (`SqlTokenKind`: `Keyword`, `StringLiteral`, `Number`, `Comment`, `Identifier`, `Operator`) colored via `SyntaxPalette`.
- **Visible Line Windowing (`EditorLayout`)**: Only lines in `visible_lines()` are laid out and painted, with gutter line numbers (`text_origin_x`), active-line highlight, selections (`EditorSelection`), and `EditorCursor` (`CursorMode`).
- **Autocomplete Popup**: `AutocompletePopup` renders ranked `AutocompleteItem`s (`CompletionKind`) anchored to `(line, column)`.
- **Hit Testing**: `EditorHitTest` resolves pointer clicks to line/column offsets or gutter actions.

## 4. Adaptive Vector Glyphs & Theme Palette (`glyphs.rs`, `palette.rs`)

- **Vector Engine Glyphs (`EngineKind::ALL`)**: All 11 supported engines (`Postgresql`, `Mysql`, `Mariadb`, `Sqlite`, `Duckdb`, `Mssql`, `Clickhouse`, `Cassandra`, `Scylladb`, `Mongodb`, `Redis`) and the DataForge icon are defined in a normalized `16.0 x 16.0` DIP viewbox (`NORMALIZED_VIEWBOX`). A pure-Rust SVG path parser converts path commands into `ID2D1PathGeometry` at any DPI scale.
- **Theme Palette (`ThemePalette`)**: Maps `ThemeMode` (`Light` / `Dark` / `HighContrast`) and imported VS Code themes into `ChromePalette`, `ContentPalette`, `GridPalette`, and `SyntaxPalette`.
