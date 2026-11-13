# DataForge

**DataForge** is a native Windows database management workbench built in Rust on top of Win32, Direct2D, and DirectWrite, powered by the [TablePro](https://github.com/TableProApp/TablePro) core engine (`core/`).

## License & Attribution

- **License**: GNU Affero General Public License v3.0 or later ([`AGPL-3.0-or-later`](LICENSE)).
- **Core Engine Attribution**: DataForge vendors and builds directly upon the TablePro Rust workspace in `core/crates/` (`tablepro-core`, `tablepro-session`, `tablepro-storage`, `tablepro-ssh`, `tablepro-net`, `tablepro-ai`, `tablepro-mcp`, `tablepro-cli`, `tablepro-foreign-import`, `tablepro-account`, and the nine `tablepro-driver-*` crates).
- **Windows Shell & UI**: All Windows-native crates under `crates/` (`dataforge-windows-platform`, `dataforge-state`, `dataforge-gfx`, `dataforge-services`, `dataforge-windows-app`, `dataforge-e2e`) are original DataForge code licensed under `AGPL-3.0-or-later`.
