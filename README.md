<div align="center">

<img src="crates/dataforge-windows-app/assets/dataforge_logo.png" alt="DataForge Logo" width="112" height="112" />

# DataForge

**A fast, native SQL & database workbench built in Rust for Windows.**

[![CI](https://img.shields.io/github/actions/workflow/status/williamcachamwri/DataForge/ci.yml?branch=main&style=flat-square&label=CI&logo=github)](https://github.com/williamcachamwri/DataForge/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/actions/workflow/status/williamcachamwri/DataForge/release.yml?style=flat-square&label=Release&logo=github)](https://github.com/williamcachamwri/DataForge/actions/workflows/release.yml)
[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![GPUI](https://img.shields.io/badge/UI-HeroGPUI%20%2F%20Direct3D%2011-0078D4?style=flat-square&logo=windows&logoColor=white)](#why-dataforge-exists)
[![Drivers](https://img.shields.io/badge/Drivers-9%20Engines-10B981?style=flat-square)](#supported-engines--connection-security)
[![Memory](https://img.shields.io/badge/RAM-~55%20MB%20under%20load-F59E0B?style=flat-square)](#real-world-benchmarks)
[![Startup](https://img.shields.io/badge/Cold%20Startup-~10%20ms-6366F1?style=flat-square)](#real-world-benchmarks)
[![Windows](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-0078D4?style=flat-square&logo=windows11&logoColor=white)](#installation--auto-updates)
[![License: MIT / AGPL-3.0](https://img.shields.io/badge/License-MIT%20%2F%20AGPL--3.0-blue?style=flat-square)](LICENSE-MIT)

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [Español](README.es.md) · [Français](README.fr.md)

</div>

---

## Why DataForge Exists

Most desktop database clients ask you to pick between two compromises: an Electron wrapper that burns a gigabyte of RAM just sitting idle on an empty query tab, or a JVM tool that takes five seconds to wake up before you can inspect a local SQLite file.

DataForge takes a different path. Inspired by [TablePro](https://github.com/TableProApp/TablePro) and powered by its Rust database core, DataForge replaces cross-platform webviews with a native Windows desktop stack rendered directly on **Direct3D 11** via **HeroGPUI** and integrated with **Windows 11 DWM** backdrop materials (**Gaussian Blur**, **Mica**, **Mica Alt**, and **Opaque**).

The result feels immediate: cold startup lands around **10 ms**, memory stays near **~55 MB** even when scrolling and querying **10-million-row** SQLite tables, and you still get the modern ergonomics you actually want day-to-day—schema-aware autocomplete, optional Vim bindings, staged inline grid edits, Safe Mode write guards, a built-in local MCP server, and a privacy-first AI SQL assistant.

---

## Visual Tour

### High-Speed SQL Editor, Vim Mode & Live Results (`15 ms` on 10M rows)
*Windows 11 Gaussian Blur across the workspace, dialect-aware syntax highlighting, schema-aware autocomplete, optional Vim modal editing (`NORMAL` / `INSERT` / `VISUAL` / `COMMAND`), and a split live results pane.*

![DataForge SQL Editor querying 10 million rows](docs/screenshots/sql-editor-10m.jpg)

---

### Virtualized Data Grid, Foreign Keys & Parameterized Filtering
*60+ FPS virtualized table rendering with inline editing, staged `Commit` / `Rollback`, frozen leading columns, column visibility controls, multi-operator parameterized filters, structure/DDL/foreign-key inspection, and atomic file export.*

![DataForge Data Grid view](docs/screenshots/data-grid.jpg)

---

### Schema-Only AI Assistant (Privacy-First)
*Non-blocking `320px` right-hand AI drawer connected to local Ollama models (e.g., `llama3.2`) or BYOK cloud endpoints. Only schema metadata leaves your machine—your table rows stay strictly local.*

![DataForge Schema-Aware AI Assistant](docs/screenshots/ai-assistant.jpg)

---

## Deep Feature Breakdown

### Supported Engines & Connection Security

DataForge compiles **9 native database drivers** directly into the binary—no external ODBC driver hunting or runtime plugins needed:

| Engine | Driver Crate | Highlights |
| :--- | :--- | :--- |
| **PostgreSQL** | `tablepro-driver-postgres` | `tokio-postgres` + `deadpool`, `COPY IN` support, schema/DDL inspection |
| **MySQL / MariaDB** | `tablepro-driver-mysql` | Full SQL dialect & catalog metadata support |
| **SQLite** | `tablepro-driver-sqlite` | Zero-config local `.sqlite` / `.db` files with direct file-drop/CLI open |
| **DuckDB** | `tablepro-driver-duckdb` | Bundled DuckDB 2.0 with built-in `json` and `parquet` support |
| **Microsoft SQL Server** | `tablepro-driver-mssql` | Pure-Rust TDS 8.0 (`tiberius`) with Windows-friendly TLS & multi-result batches |
| **ClickHouse** | `tablepro-driver-clickhouse` | Native HTTP/TLS analytical queries via `hyper-rustls` |
| **Cassandra / ScyllaDB** | `tablepro-driver-cassandra` | Native CQL driver (`scylla`) with rustls transport |
| **MongoDB** | `tablepro-driver-mongodb` | Document queries, BSON 3 mapping, and DNS seed list resolution |
| **Redis** | `tablepro-driver-redis` | Key-value inspection and command execution over async TLS/TCP |

Security is built into the connection layer rather than bolted on later:

- **In-Process SSH Tunneling**: Built-in `russh` SSH client (`tablepro-ssh`) with password or private-key authentication, interactive passphrase prompts, and host-key verification modals—no external `ssh.exe` wrapper needed.
- **TLS & Windows System Certificate Store**: Powered by `rustls` (`aws-lc-rs`) and `rustls-platform-verifier`, supporting `Disable`, `Prefer`, `Require`, `VerifyCA`, and `VerifyFull` modes with custom CA, client certificate, and client key files.
- **Windows Credential Manager Vault**: Database passwords, SSH secrets, AI API keys, and MCP bearer tokens are stored in **Windows Credential Manager** (`CredWriteW` / `CredReadW` with `zeroize` memory scrubbing)—never written as plaintext in config files or logs.
- **Safe Mode Guards**: Configure connections with `Off`, `ConfirmWrites`, or `ReadOnly` protection so accidental `UPDATE`, `DELETE`, or `DROP` statements require explicit confirmation or are blocked outright.
- **Foreign Connection Import**: Bring over saved connections and queries from other database clients via `tablepro-foreign-import`.

---

### SQL Editor & High-Volume Data Grid

- **Dialect-Aware SQL Editor**:
  - Real-time syntax highlighting per engine dialect (`PostgreSQL`, `MySQL`, `SQLite`, `MsSql`, and more) with independent UI vs. syntax palettes.
  - Context-aware autocomplete for tables, columns, schemas, SQL keywords, and clause snippets (`FROM`, `JOIN`, `INTO`, `UPDATE`, etc.).
  - Multi-cursor editing, optional **Vim mode** (`Normal`, `Insert`, `Visual`, `Command`), saved query library, and persistent query history.
  - Run the entire buffer (`Ctrl+Enter` / `F5`), run only your highlighted selection (`Ctrl+Shift+Enter`), or cancel long queries in-flight (`Shift+Esc`).
- **Interactive Virtualized Data Grid**:
  - Smooth virtualized scrolling over large datasets with a docked **Row Inspector** (`Ctrl+I`) and **Structure / Columns / Indexes / Foreign Keys / DDL** tabs.
  - **Inline Cell Editing**: Edit cells, insert rows (`Alt+Insert`), or mark rows for deletion (`Ctrl+Shift+Backspace`) in a staged batch—review changes visually before hitting **Commit** (`Ctrl+Shift+S`) or **Rollback** (`Ctrl+Shift+R`).
  - **Frozen Leading Columns**: Pin leading columns (up to 2/3 of viewport width), toggle column visibility, and drag to resize headers.
  - **Parameterized Column Filtering**: Stack filter rules using 11 dialect-safe parameterized operators (`=`, `!=`, `Contains`, `StartsWith`, `EndsWith`, `<`, `<=`, `>`, `>=`, `IS NULL`, `IS NOT NULL`).
  - **Atomic Exports**: Export query results or tables to **CSV** (`.csv`), **JSON Array** (`.json`), **JSON Lines** (`.jsonl`), or **SQL `INSERT`** (`.sql`) from the Export modal (`Ctrl+Shift+E`), written atomically via `.part` staging files (`tablepro-core` also ships TSV, XML, and constant-memory XLSX encoders).

---

### Appearance, Windows DWM Backdrops & Localization

- **14 Built-In Themes + VS Code Import**:
  - Ships with **14 curated presets**: `Dark`, `Light`, `High Contrast`, `Gaussian Glass Dark`, `Gaussian Glass Light`, `Tokyo Night`, `Dracula`, `Catppuccin Mocha`, `One Dark Pro`, `Nord`, `Monokai Pro`, `Gruvbox Dark`, `GitHub Light`, and `Catppuccin Latte`.
  - Mix and match your **UI Theme** and **Codeblock Syntax Palette** independently, or import any **VS Code `.json` color theme** (with path-escape and depth validation).
- **Native Windows 11 DWM Backdrops**: Choose between **Gaussian Blur (Frosted Glass)**, **Mica**, **Mica Alt**, and **Opaque** window materials, while keeping floating overlays (`1.0` alpha) razor-sharp.
- **Reduced Motion Accessibility**: Automatically respects Windows `SPI_GETCLIENTAREAANIMATION` so drawers and modals snap in `0 ms` when system animations are disabled.
- **6 UI & Diagnostics Languages**: Full localization across UI labels, menus, modals, tooltips, and driver diagnostic hints:
  - `en-US` (English) · `vi-VN` (Tiếng Việt) · `zh-CN` (简体中文) · `ja-JP` (日本語) · `es-ES` (Español) · `fr-FR` (Français)

---

### Local-First AI Assistant & Built-in MCP Server

- **Privacy-First AI Drawer (`Ctrl+Shift+A`)**:
  - **Schema-Only Guarantee**: Only schema context (table names, column names, types, and query language) is sent to the model. Actual row data is never transmitted.
  - **Local & BYOK Providers**: Works out of the box with local **Ollama** (`http://127.0.0.1:11434/v1`, e.g., `llama3.2`) or any **OpenAI-compatible** endpoint, plus bring-your-own-key **OpenAI** and **Anthropic** stored securely in Windows Credential Manager.
  - Streams responses in real time with syntax-highlighted SQL blocks you can copy or drop straight into your active editor.
- **Built-in Local MCP Server (`tablepro-mcp`)**:
  - Expose your active DataForge connections to external coding agents and IDEs over a loopback HTTP server (`127.0.0.1`), guarded by a Bearer access token in Windows Credential Manager and peer verification.
  - Provides **11 structured MCP tools**: `list_connections`, `connect`, `list_databases`, `list_schemas`, `list_tables`, `describe_table`, `get_table_ddl`, `browse_table`, `count_rows`, `execute_query`, and `export_data`.
  - `execute_query` respects each connection's External Clients & Safe Mode policy and prompts for explicit user approval inside DataForge before any write statement runs.

---

### CLI & Headless Workflows

`dataforge.exe` doubles as both the GUI launcher and a scriptable CLI (`tablepro-cli`):

- **App-Level Flags**:
  - `dataforge.exe --version` (`-V`) — Print the application version and exit.
  - `dataforge.exe --help` (`-h`) — Print CLI usage and options.
  - `dataforge.exe --smoke-startup` — Run a headless startup timing and `9/9` driver registration check.
- **Direct Open & Single-Instance Forwarding**:
  - Pass `.sqlite` / `.db` / `.duckdb` / `.sql` paths or connection URLs (`postgresql://...`) directly to `dataforge.exe` to open them in the active window via Named Pipe IPC (or add `-n` / `--new-window`).
- **CLI Subcommands**:
  - `open`, `query`, `connections`, `databases`, `schemas`, `tables`, `describe`, `export`, `status`, and `completion` (`bash`, `zsh`, `fish`).
  - Supports `--format` (`table`, `csv`, `tsv`, `json`, `jsonl` for `query`; `csv`, `json`, `sql` for `export`), `--read-only`, `--limit`, and `--timeout`.

```powershell
# Run a read-only query from PowerShell and output JSON Lines
.\target\release\dataforge.exe query -c Staging --read-only --format jsonl "SELECT id, email FROM users LIMIT 10"

# Inspect a table's CREATE TABLE DDL
.\target\release\dataforge.exe describe -c Staging --ddl users
```

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

## Installation & Auto-Updates

Download prebuilt Windows x64 binaries from [**GitHub Releases**](https://github.com/williamcachamwri/DataForge/releases):

- **Installer (Recommended) — `DataForge-win-Setup.exe`**:
  - Installs cleanly into `%LocalAppData%` with Start Menu and desktop shortcuts—**no UAC administrator prompt required**.
  - Powered by **Velopack** for silent, bandwidth-efficient binary delta updates. DataForge checks for updates automatically on startup (configurable in **Settings → About**) and lets you check, download, and restart to apply updates directly inside **Settings → About**.
- **Portable Archive — `DataForge-win-Portable.zip`**:
  - Extract anywhere and run `dataforge.exe` directly. Portable / unpackaged builds run cleanly in standalone mode without modifying installer state.
- **Verify Integrity**:
  - Every release publishes a `SHA256SUMS.txt` manifest so you can verify downloads in PowerShell:

```powershell
Get-FileHash .\DataForge-win-Setup.exe -Algorithm SHA256
```

---

## Keyboard Shortcuts Quick Reference

| Shortcut | Action |
| :--- | :--- |
| `Ctrl+K` / `Ctrl+Shift+K` / `Ctrl+P` | Open Command Palette |
| `Ctrl+Enter` / `F5` | Run active SQL query |
| `Ctrl+Shift+Enter` | Run selected SQL text |
| `Shift+Esc` | Cancel running query |
| `Ctrl+S` | Save query to library |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | New query tab / Close tab / Reopen closed tab |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next / Previous tab |
| `Ctrl+F` | Focus object sidebar filter |
| `Ctrl+B` | Toggle object sidebar |
| `Ctrl+I` | Toggle Row Inspector |
| `Ctrl+Shift+A` | Toggle AI Assistant drawer |
| `Ctrl+Shift+O` | Open Connections library |
| `Ctrl+,` | Open Settings |
| `Ctrl+Shift+M` | Toggle / cycle theme |
| `Ctrl+Shift+S` / `Ctrl+Shift+R` | Commit / Rollback staged grid edits |
| `Alt+Insert` / `Ctrl+Shift+Backspace` | Add row / Delete selected rows in grid |
| `Ctrl+Shift+E` | Export results or table |
| `Esc` | Dismiss active modal or overlay |

*Tip: Enable **Vim mode** in the SQL editor toolbar for `NORMAL`, `INSERT`, `VISUAL`, and `:` command navigation.*

---

## Building from Source on Windows

### Prerequisites

- **OS**: Windows 10 or Windows 11 (x64)
- **Rust Toolchain**: Stable MSVC toolchain (`x86_64-pc-windows-msvc`, Rust `2024` Edition) via [rustup](https://rustup.rs/)
- **Visual Studio C++ Build Tools & Windows SDK**: Required for Win32, Direct3D 11 (`HeroGPUI`), and bundled C drivers
- **Shell**: Windows PowerShell (use `;` instead of `&&` when chaining commands)

> [!CAUTION]
> **Always build and test with `--release`**: Never run unprofiled debug `cargo build` or `cargo test` across this workspace unless specifically debugging symbols. Debug PDBs for the full workspace and 9 database drivers in `target\debug` can grow past **140 GB**. Use `--release` (and `-j 1` on memory-constrained machines).

### Check, Build & Run

```powershell
# Make sure an existing dataforge.exe isn't locking the output binary
Get-Process dataforge -ErrorAction SilentlyContinue

# Type-check in release mode
cargo check --release -j 1 -p dataforge-windows-app

# Build the release binary
cargo build --release -j 1 -p dataforge-windows-app

# Verify cold startup & 9/9 driver registration
.\target\release\dataforge.exe --smoke-startup

# Launch DataForge
.\target\release\dataforge.exe
```

### Run Release Verification Suites

```powershell
# Core SQL filter & dialect tests
cargo test --release -j 1 -p tablepro-core filter

# Startup performance & 6-locale L10n parity tests
cargo test --release -j 1 -p dataforge-e2e --test startup_perf_e2e -- --nocapture

# 100k-row SQLite load & 10k-frame scroll E2E benchmark
cargo test --release -j 1 -p dataforge-e2e --test sqlite_e2e -- --nocapture
```

---

## Architecture & Workspace Map

```text
DataForge/
├── core/                             # Upstream TablePro database core & 9 native drivers
│   └── crates/
│       ├── core/                     # tablepro-core: query models, filters, dialects, exports
│       ├── session/                  # tablepro-session: live sessions, connection pools, transactions
│       ├── cli/                      # tablepro-cli: shell subcommands & headless runner
│       ├── mcp/                      # tablepro-mcp: token-protected localhost MCP HTTP server
│       ├── ai/                       # tablepro-ai: schema-only Ollama / OpenAI / Anthropic client
│       ├── ssh/                      # tablepro-ssh: in-process russh SSH tunneling
│       ├── net/                      # tablepro-net: rustls TLS + system cert verification
│       ├── storage/                  # tablepro-storage: saved connections, library, query history
│       ├── foreign-import/           # tablepro-foreign-import: import from other DB clients
│       ├── account/                  # tablepro-account: account & token traits
│       └── drivers/                  # postgres, mysql, sqlite, duckdb, mssql, clickhouse,
│                                     # cassandra, mongodb, redis
└── crates/
    ├── dataforge-gfx/                # Vector glyphs & engine icons
    ├── dataforge-state/              # Editor state, autocomplete, grid model, tabs, 14 themes
    ├── dataforge-services/           # Async Tokio runtime, session/AI/MCP orchestration, diagnostics
    ├── dataforge-windows-platform/   # Settings JSON, Win32 Credential Manager, DWM & motion helpers
    ├── dataforge-windows-app/        # HeroGPUI (Direct3D 11) desktop app, Velopack updater, 6 locales
    └── dataforge-e2e/                # End-to-end SQLite, DuckDB, startup & grid benchmarks
```

---

## Contributing, Security, Code of Conduct & License

- **Contributing**: See [CONTRIBUTING.md](CONTRIBUTING.md) for architecture notes, release-only build rules, HeroGPUI overlay guidelines, and 6-locale L10n conventions.
- **Changelog**: See [CHANGELOG.md](CHANGELOG.md) for release history and notable changes.
- **Code of Conduct**: See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
- **Security Policy**: See [SECURITY.md](SECURITY.md) for private vulnerability disclosure instructions and local-first privacy guarantees.
- **License**: DataForge is licensed under [LICENSE](LICENSE) (GNU Affero General Public License v3.0) and [LICENSE-MIT](LICENSE-MIT), with upstream core attribution to [TableProApp/TablePro](https://github.com/TableProApp/TablePro) (MIT).
