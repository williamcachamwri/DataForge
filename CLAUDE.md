# CLAUDE.md

This file provides guidance to Claude Code (and compatible coding agents) when working with code in this repository.

See [`AGENTS.md`](AGENTS.md) for the full workspace layout, build/test commands, and GPUI rendering constraints.

## Quick Reference

- **Target OS / Shell**: Windows 11, PowerShell (`;` to chain commands).
- **Primary Check Command**: `cargo check --release -j 1 -p tableminus-windows-app`
- **Primary Build Command**: `cargo build --release -j 1 -p tableminus-windows-app`
- **Smoke Test**: `.\target\release\tableminus.exe --smoke-startup`
- **Important**: Always prefer `--release` for `cargo test` / `cargo check` / `cargo build` to avoid generating 140+ GB of `target\debug` PDB artifacts.
- **UI Architecture**:
  - `crates/tableminus-windows-app/src/branding.rs`: Embedded `.ico` (`WM_SETICON`) and transparent Apple HIG PNG logo (`tableminus_logo.png`).
  - `crates/tableminus-windows-app/src/ui/theme.rs`: Chrome & Content palettes, backdrop translucency, and `animate_*` helpers.
  - `crates/tableminus-windows-app/src/ui/sidebar.rs`: Object explorer tree, vertical scroll container, and async `refresh_active_schema` (reloads expanded table details automatically).
  - `crates/tableminus-windows-app/src/ui/status_bar.rs`: Live connection, sub-view, schema, object count, dialect, cursor/Vim, and tab metrics.
