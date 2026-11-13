# DataForge

**DataForge** is a native Windows database management workbench built in Rust on top of Win32, Direct2D, and DirectWrite, powered by the [TablePro](https://github.com/TableProApp/TablePro) core engine (`core/`).

## License & Attribution

- **License**: GNU Affero General Public License v3.0 or later ([`AGPL-3.0-or-later`](LICENSE)).
- **Core Engine Attribution**: DataForge vendors and builds directly upon the TablePro Rust workspace in `core/crates/` (`tablepro-core`, `tablepro-session`, `tablepro-storage`, `tablepro-ssh`, `tablepro-net`, `tablepro-ai`, `tablepro-mcp`, `tablepro-cli`, `tablepro-foreign-import`, `tablepro-account`, and the nine `tablepro-driver-*` crates).
- **Windows Shell & UI**: All Windows-native crates under `crates/` (`dataforge-windows-platform`, `dataforge-state`, `dataforge-gfx`, `dataforge-services`, `dataforge-windows-app`, `dataforge-e2e`) are original DataForge code licensed under `AGPL-3.0-or-later`.

## Supported Database Engines (11)

DataForge provides adaptive vector glyphs (`EngineKind::ALL` in `dataforge-gfx`) and driver connectivity across 11 database engines backed by the 9 `tablepro-driver-*` crates:

| Engine | Driver Crate | Category | Data Model | Endpoint |
| --- | --- | --- | --- | --- |
| PostgreSQL | `tablepro-driver-postgres` | Relational | Relational | Network / SSH |
| MySQL | `tablepro-driver-mysql` | Relational | Relational | Network / SSH |
| MariaDB | `tablepro-driver-mysql` | Relational | Relational | Network / SSH |
| SQLite | `tablepro-driver-sqlite` | Relational | Relational | File |
| DuckDB | `tablepro-driver-duckdb` | Analytical | Relational | File |
| SQL Server | `tablepro-driver-mssql` | Relational | Relational | Network / SSPI |
| ClickHouse | `tablepro-driver-clickhouse` | Analytical | Relational | Network / TLS |
| Cassandra | `tablepro-driver-cassandra` | Wide-Column | Relational | Network / TLS |
| ScyllaDB | `tablepro-driver-cassandra` | Wide-Column | Relational | Network / TLS |
| MongoDB | `tablepro-driver-mongodb` | Document | Document | Network / SRV |
| Redis | `tablepro-driver-redis` | Key-Value | Key-Value | Network / TLS |

## Windows Direct2D / DirectWrite Architecture

DataForge is a pure-Rust Win32 desktop application without WebView or Electron overhead:

- **Platform (`dataforge-windows-platform`)**: Windows Credential Manager (`CredReadW`/`CredWriteW`), Named Pipe single-instance IPC, Registry theme/DPI/machine-ID detection, `KnownFolders` storage paths, and foreign connection import discovery.
- **State (`dataforge-state`)**: Headless, deterministic state machines (`ConnectionWindowPhaseMachine`, `DataGridModel`, `EditorModel`, `TabStripModel`, `LibraryModel`, `ThemeResolver`).
- **Graphics (`dataforge-gfx`)**: Direct3D 11 + DXGI swap chain + Direct2D (`ID2D1DeviceContext`) + DirectWrite text layout, virtualized `DataGridRenderer`, SQL-tokenized `SqlEditorRenderer`, and adaptive 16x16 SVG-to-`ID2D1PathGeometry` engine glyphs.
- **Services (`dataforge-services`)**: `AppRuntime`, `DriverRegistry`, `SessionManager`, atomic CSV/JSON/SQL export, `tablepro-foreign-import` bridge, local MCP host, BYOK/Ollama AI assistant, and `tracing`/crash-report diagnostics.
- **Application (`dataforge-windows-app`)**: Per-Monitor DPI v2 Win32 window shell, UI Automation (`WM_GETOBJECT`) baseline, modal/prompt flows, and multi-pane workbench views.

See [ARCHITECTURE.md](ARCHITECTURE.md) and [`docs/`](docs/) for detailed technical documentation.
