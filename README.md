<div align="center">

<img src="crates/dataforge-windows-app/assets/dataforge_logo.png" alt="DataForge Logo" width="112" height="112" />

# DataForge

**A fast, native SQL & database workbench built in Rust for Windows.**

[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![GPUI](https://img.shields.io/badge/UI-HeroGPUI%20%2F%20Direct3D%2011-0078D4?style=flat-square&logo=windows&logoColor=white)](#why-dataforge)
[![Drivers](https://img.shields.io/badge/Drivers-9%20Engines-10B981?style=flat-square)](#supported-databases)
[![Memory](https://img.shields.io/badge/RAM-~55%20MB%20under%20load-F59E0B?style=flat-square)](#real-world-benchmarks)
[![Startup](https://img.shields.io/badge/Cold%20Startup-~10%20ms-6366F1?style=flat-square)](#real-world-benchmarks)
[![Windows](https://img.shields.io/badge/Platform-Windows%2011-0078D4?style=flat-square&logo=windows11&logoColor=white)](#building-from-source)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue?style=flat-square)](LICENSE-MIT)

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [Español](README.es.md) · [Français](README.fr.md)

</div>

---

## Why DataForge?

Most desktop database tools force a trade-off: either they run inside Electron and eat a gigabyte of RAM just sitting idle, or they wrap a heavy JVM that takes five seconds to open a local SQLite file.

DataForge takes a different route. The entire stack—from the database drivers and query session engine down to the GPU-accelerated UI—is written in native **Rust** using **HeroGPUI (Direct3D 11)** and **Windows 11 DWM Gaussian Blur / Mica** backdrops. It boots in around **10 ms**, stays around **55 MB of RAM** even when querying a 10-million-row SQLite database, and still gives you the ergonomics of a modern workbench: schema-aware autocomplete, Vim keybindings, inline grid editing, safe-mode guards, and a local-first AI SQL assistant.

---

## Screenshots

### SQL Editor & 10M-Row Query Execution (`15 ms`)
*Full-window Windows 11 Gaussian Blur, syntax-highlighted SQL editor with Vim mode & schema-aware autocomplete, and a live results pane.*

![DataForge SQL Editor querying 10 million rows](docs/screenshots/sql-editor-10m.jpg)

---

### High-Density Data Grid & Filtering
*Virtualized table rendering with foreign-key jump links, column visibility toggles, multi-operator filters, pagination, and CSV/JSON/SQL export.*

![DataForge Data Grid view](docs/screenshots/data-grid.jpg)

---

### Schema-Aware AI Assistant (Privacy-First)
*Connects to local Ollama models (e.g. `llama3.2`) or cloud providers. Only schema metadata is shared—your row data never leaves your machine.*

![DataForge Schema-Aware AI Assistant](docs/screenshots/ai-assistant.jpg)

---

## What's Inside

- **Native Rust + Direct3D 11 UI**: Smooth 60+ FPS scrolling across large result sets with zero Electron/Chromium or JVM overhead.
- **9 Built-in Database Drivers**: Connect to relational, analytical, document, and key-value stores out of the box.
- **Smart SQL Editor**:
  - Context-aware autocomplete for tables, columns, schemas, SQL keywords, and snippets (`FROM`, `JOIN`, `INTO`, `UPDATE`, etc.).
  - Independent **UI Theme** and **Codeblock Syntax Palette** (14+ built-in presets including Tokyo Night, Catppuccin, Dracula, Nord, One Dark, plus `.json` VS Code theme import).
  - Optional **Vim mode**, multi-cursor support, and query library / history.
- **Interactive Data Grid & Structure Inspector**:
  - Inline cell editing with uncommitted change tracking (`Commit` / `Rollback`).
  - Safe Mode protection (`Confirm` / `Strict`) for mutating statements.
  - Foreign-key navigation, column/index inspection, and DDL viewer.
- **Built-in MCP Server & Local AI Drawer**:
  - Expose schema tools over Model Context Protocol (`MCP`) for your IDE or agent workflows.
  - Built-in AI drawer to explain SQL, optimize joins, or draft `SELECT` queries using only schema context.
- **Windows 11 Native Aesthetics**:
  - Supports **Gaussian Blur (Frosted Glass)**, **Mica**, **Mica Alt**, and **Opaque** window backdrops.

---

## Supported Databases

| Engine | Driver Crate |
| :--- | :--- |
| **PostgreSQL** | `tablepro-driver-postgres` |
| **MySQL / MariaDB** | `tablepro-driver-mysql` |
| **SQLite** | `tablepro-driver-sqlite` |
| **DuckDB** | `tablepro-driver-duckdb` |
| **Microsoft SQL Server** | `tablepro-driver-mssql` |
| **ClickHouse** | `tablepro-driver-clickhouse` |
| **Cassandra / ScyllaDB** | `tablepro-driver-cassandra` |
| **MongoDB** | `tablepro-driver-mongodb` |
| **Redis** | `tablepro-driver-redis` |

---

## Real-World Benchmarks

Measured on Windows 11 with a release build of `dataforge.exe` (`--release`):

| Benchmark | Observed Result |
| :--- | :--- |
| **Cold startup (`--smoke-startup`, 9/9 drivers)** | `9.21 – 10.11 ms` |
| **100k-row SQLite E2E seed (`sqlite_100k_rows_load_and_scroll_perf`)** | `78.97 ms` |
| **100k-row grid load + 10k-frame scroll** | `24.43 ms` load · `0.432 ms` scroll (`0.043 µs/frame`) |
| **10M-row SQLite indexed query (`ORDER BY event_ts DESC LIMIT 1000`)** | `5 – 15 ms` |
| **10M-row full-table JSON scan + 3-table JOIN + 3 Window Functions** | `40.01 s` |
| **Process RAM during 100k / 500k / 1M-row stress queries** | `~55 MB` steady |

---

## Building from Source

### Prerequisites
- **Windows 10 / 11** (x64)
- **Rust toolchain** (stable, MSVC target)
- **Visual Studio Build Tools** (C++ / Windows SDK)

### Build & Run

```powershell
# Build the release binary
cargo build --release -p dataforge-windows-app

# Verify cold startup & driver registration
.\target\release\dataforge.exe --smoke-startup

# Launch DataForge
.\target\release\dataforge.exe
```

### Run Tests

```powershell
# Core filter & dialect tests
cargo test --release -p tablepro-core filter

# SQLite 100k-row load & scroll E2E benchmark
cargo test --release -p dataforge-e2e --test sqlite_e2e -- --nocapture
```

---

## Workspace Layout

```text
DataForge/
├── core/                         # Database engine core, drivers (9), session, CLI, MCP, AI
└── crates/
    ├── dataforge-gfx/            # Vector glyphs & engine icons
    ├── dataforge-state/          # Editor state, autocomplete, tabs, themes
    ├── dataforge-services/       # Async runtime & workspace session services
    ├── dataforge-windows-platform/ # Settings persistence, Win32 backdrop & motion helpers
    ├── dataforge-windows-app/    # HeroGPUI desktop UI, titlebar, grid, editor, modals
    └── dataforge-e2e/            # End-to-end SQLite & UI performance benchmarks
```


---

## Community, Security & License

- **Contributing**: See [CONTRIBUTING.md](CONTRIBUTING.md) for architecture notes, build rules, UI/L10n guidelines, and pull request conventions.
- **Code of Conduct**: See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
- **Security Policy**: See [SECURITY.md](SECURITY.md) for vulnerability reporting and local-first privacy guarantees.
- **License**: DataForge is available under the [MIT License](LICENSE-MIT) (with upstream core attribution to [TableProApp/TablePro](https://github.com/TableProApp/TablePro), also under MIT) alongside [LICENSE](LICENSE).
