# AGENTS.md

Guidance for AI coding agents working in `d:\DataForge`.

## Workspace Overview

DataForge is a native Windows SQL & database workbench written in Rust:
- `core/`: Upstream TablePro core workspace (`tablepro-core`, `tablepro-session`, `tablepro-cli`, `tablepro-mcp`, `tablepro-ai`, and 9 database drivers under `core/crates/drivers/*`).
- `crates/dataforge-gfx`: Vector glyphs and database engine icons.
- `crates/dataforge-state`: Editor state, SQL grammar/autocomplete, tab state, and 14+ built-in UI/syntax themes.
- `crates/dataforge-services`: Async Tokio runtime and session orchestration.
- `crates/dataforge-windows-platform`: Settings persistence, Windows DWM backdrop (`GaussianBlur`, `Mica`, `MicaAlt`, `Opaque`), and reduced-motion detection.
- `crates/dataforge-windows-app`: HeroGPUI (Direct3D 11) desktop application (`dataforge.exe`).
- `crates/dataforge-e2e`: End-to-end SQLite & grid performance benchmarks.

## Build, Test & Verification Rules

- **Shell**: Windows PowerShell — use `;` instead of `&&` to chain commands.
- **Avoid Debug PDB Bloat**: Never run unprofiled debug `cargo build` or `cargo test` unless explicitly requested; debug PDBs in `target\debug` grow beyond 140 GB. Always use `--release` (and `-j 1` on memory-constrained builds):
  - Check: `cargo check --release -j 1 -p dataforge-windows-app`
  - Build: `cargo build --release -j 1 -p dataforge-windows-app`
  - Smoke test: `.\target\release\dataforge.exe --smoke-startup`
  - Filter tests: `cargo test --release -j 1 -p tablepro-core filter`
  - 100k E2E benchmark: `cargo test --release -j 1 -p dataforge-e2e --test sqlite_e2e -- --nocapture`
- **Locked Binary**: Before running `cargo build --release -p dataforge-windows-app`, check if `dataforge.exe` is running (`Get-Process dataforge`).

## UI & GPUI Constraints

- **Backdrop Blur vs. Overlays**: `gpui-pre-windows 0.3.11` uses Windows DWM `WindowBackgroundAppearance::Blurred` for the OS window backdrop, which blurs desktop pixels behind the window. GPUI does not have an in-app `backdrop-filter: blur()` shader for floating `div`s over in-app text. Keep `theme.colors.overlay.background` opaque (`1.0` alpha) so modals, dropdowns, command palette, and toasts remain legible without sharp text bleed-through.
- **Single-Layer Translucent Surfaces**: Never stack multiple `.bg(...)` fills on nested editor/panel containers when backdrop translucency is active; paint `code_bounds` and `gutter_bounds` once without overlap.
- **Scroll Containers**: Wrap scrollable `TreeView` / lists in a `div().id(...).flex_1().min_h_0().overflow_y_scroll()` (requires `StatefulInteractiveElement` in scope).
