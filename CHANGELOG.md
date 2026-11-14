# Changelog

All notable changes to **DataForge** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2026-11-15

### Fixed

- **app**: Suppress Windows console window on GUI launch while preserving CLI output (`windows_subsystem = "windows"`)
- **updater**: Separate Velopack event queue from wakeup signal so `Check for Updates` never drops events or stalls indefinitely
- **ui**: Widen library detail pane for wide windows
- **ui**: Make settings layout responsive across viewports

### Documentation

- **readme**: Rewrite French README in DBeaver/TablePro style
- **readme**: Rewrite Spanish README in DBeaver/TablePro style
- **readme**: Rewrite Japanese README in DBeaver/TablePro style
- **readme**: Rewrite Simplified Chinese README in DBeaver/TablePro style
- **readme**: Rewrite Vietnamese README in DBeaver/TablePro style
- **readme**: Rewrite English README in DBeaver/TablePro style

## [0.1.0] - 2026-11-14

### Added

- **Native Windows Desktop Workbench**: Built in Rust (`2024` Edition) with `HeroGPUI` (Direct3D 11) and Windows 11 DWM backdrops (`GaussianBlur`, `Mica`, `MicaAlt`, `Opaque`), single-instance IPC, and Windows Credential Manager secret storage.
- **9 Built-in Database Drivers**: Support for PostgreSQL, MySQL / MariaDB, SQLite, DuckDB, Microsoft SQL Server, ClickHouse, Cassandra / ScyllaDB, MongoDB, and Redis out of the box (`tablepro-driver-*`).
- **SQL Editor & Themes**: Schema-aware autocomplete, optional Vim keybindings, query library/history, and 14+ built-in UI and syntax palettes with VS Code `.json` theme import.
- **Data Grid & Structure Inspector**: Virtualized result grid with inline cell editing (`Commit` / `Rollback`), Safe Mode (`Confirm` / `Strict`), foreign-key navigation, DDL/index/column inspection, and CSV/JSON/SQL export.
- **MCP & Privacy-First AI Assistant**: Built-in Model Context Protocol (`MCP`) server and schema-only AI drawer for explaining, optimizing, and drafting SQL queries.
- **Full 6-Locale Localization (i18n)**: Complete UI and diagnostic coverage for `en-US`, `vi-VN`, `zh-CN`, `ja-JP`, `es-ES`, and `fr-FR`, including runtime language switching in Settings.
- **CI/CD & Release Packaging**: Automated GitHub Actions release verification, Velopack (`Setup.exe`, `Portable.zip`, and delta `.nupkg`) packaging, `SHA256SUMS.txt`, and `git-cliff` release notes.

[Unreleased]: https://github.com/williamcachamwri/DataForge/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/williamcachamwri/DataForge/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/williamcachamwri/DataForge/releases/tag/v0.1.0
