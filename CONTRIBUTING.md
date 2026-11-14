# Contributing to DataForge

Thank you for your interest in contributing to **DataForge**! Whether you are fixing a bug, adding support for a SQL dialect quirk, refining the Direct3D 11 UI, or improving translations, contributions of all kinds are welcome.

By participating in this repository, you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md). Please take a moment to review it before opening issues or pull requests.

---

## Prerequisites

DataForge is a native Windows desktop application built in Rust (`2024` Edition). Before building from source, make sure you have:

- **Operating System**: Windows 10 or Windows 11 (x64)
- **Rust Toolchain**: Stable MSVC toolchain (`x86_64-pc-windows-msvc`) via [rustup](https://rustup.rs/)
- **Visual Studio C++ Build Tools & Windows SDK**: Required for compiling native Win32, Direct3D 11 (`HeroGPUI`), and database driver dependencies
- **Shell**: Windows PowerShell (note: use `;` instead of `&&` when chaining commands in PowerShell)

---

## Workspace Architecture

DataForge is organized as a Cargo workspace combining upstream database core crates with a native Windows UI stack:

```text
DataForge/
├── core/                           # Upstream TablePro core workspace & 9 database drivers
│   └── crates/
│       ├── core/                   # tablepro-core: query models, filters, dialect helpers, schema types
│       ├── session/                # tablepro-session: active database sessions, SSH tunnels, connection pools
│       ├── cli/                    # tablepro-cli: command-line runner & headless utilities
│       ├── mcp/                    # tablepro-mcp: Model Context Protocol (MCP) server for schema tooling
│       ├── ai/                     # tablepro-ai: privacy-first schema-aware SQL assistant
│       └── drivers/                # 9 database drivers:
│           ├── postgres/           # tablepro-driver-postgres (PostgreSQL)
│           ├── mysql/              # tablepro-driver-mysql (MySQL / MariaDB)
│           ├── sqlite/             # tablepro-driver-sqlite (SQLite)
│           ├── duckdb/             # tablepro-driver-duckdb (DuckDB)
│           ├── mssql/              # tablepro-driver-mssql (Microsoft SQL Server)
│           ├── clickhouse/         # tablepro-driver-clickhouse (ClickHouse)
│           ├── cassandra/          # tablepro-driver-cassandra (Cassandra / ScyllaDB)
│           ├── mongodb/            # tablepro-driver-mongodb (MongoDB)
│           └── redis/              # tablepro-driver-redis (Redis)
└── crates/
    ├── dataforge-gfx/              # Vector glyphs and database engine icons
    ├── dataforge-state/            # Editor state, SQL grammar/autocomplete, tab state, 14+ UI/syntax themes
    ├── dataforge-services/         # Async Tokio runtime, session orchestration, and localized diagnostics
    ├── dataforge-windows-platform/ # Settings persistence, Windows DWM backdrop (GaussianBlur, Mica, MicaAlt, Opaque), reduced-motion detection
    ├── dataforge-windows-app/      # HeroGPUI (Direct3D 11) desktop application (dataforge.exe)
    └── dataforge-e2e/              # End-to-end SQLite, startup, and grid performance benchmarks
```

---

## Build, Test & Verification Rules

> [!CAUTION]
> **Avoid Debug PDB Bloat (`>140 GB`)**: Never run unprofiled debug `cargo build` or `cargo test` unless explicitly troubleshooting a symbol-specific issue. Unoptimized debug builds across the full workspace and 9 drivers generate massive PDB files in `target\debug` that can exceed **140 GB**. Always pass `--release` (and `-j 1` on memory-constrained machines).

### 1. Check for a Running Binary Before Building

Windows locks the executable while it is running. Before compiling `dataforge-windows-app`, verify that no existing `dataforge.exe` instance is holding a lock on `target\release\dataforge.exe`:

```powershell
Get-Process dataforge -ErrorAction SilentlyContinue
```

### 2. Check & Build (`--release`)

```powershell
# Type-check the desktop application in release mode
cargo check --release -j 1 -p dataforge-windows-app

# Build the release binary
cargo build --release -j 1 -p dataforge-windows-app
```

### 3. Smoke Test & Verification Suites

Every change affecting application startup, drivers, query filters, or localization should be verified with the release test commands below:

```powershell
# Verify cold startup (~10 ms) & registration of all 9 database drivers
.\target\release\dataforge.exe --smoke-startup

# Run core SQL filter & dialect unit tests
cargo test --release -j 1 -p tablepro-core filter

# Run startup performance & localization E2E assertions
cargo test --release -j 1 -p dataforge-e2e --test startup_perf_e2e -- --nocapture

# Run the 100k-row SQLite load & 10k-frame scroll benchmark
cargo test --release -j 1 -p dataforge-e2e --test sqlite_e2e -- --nocapture
```

---

## UI & HeroGPUI Guidelines

DataForge renders its interface through `gpui-pre-windows 0.3.11` over Direct3D 11 paired with Windows 11 DWM backdrops (`GaussianBlur`, `Mica`, `MicaAlt`, `Opaque`). When modifying UI components in `crates/dataforge-windows-app` or themes in `crates/dataforge-state`, keep these constraints in mind:

1. **Backdrop Blur vs. Overlays**:
   - Windows DWM `WindowBackgroundAppearance::Blurred` blurs desktop pixels behind the top-level window—GPUI does not provide an in-app `backdrop-filter: blur()` shader for floating elements drawn over in-app text.
   - Always keep `theme.colors.overlay.background` fully opaque (`1.0` alpha) so modals, dropdowns, context menus, the command palette, and toasts remain crisp and legible without background text bleeding through.
2. **Single-Layer Translucent Surfaces**:
   - Never stack multiple `.bg(...)` fills on nested editor or panel containers when window backdrop translucency is active.
   - Paint `code_bounds` and `gutter_bounds` once without overlapping translucent fills so the DWM blur/Mica tint stays uniform.
3. **Scrollable Containers**:
   - Wrap scrollable `TreeView` or list views inside a `div().id(...).flex_1().min_h_0().overflow_y_scroll()` container (ensure `StatefulInteractiveElement` is in scope). Omitting `.min_h_0()` inside flex columns prevents proper scroll clamping.

---

## Localization (L10n) Guidelines

DataForge ships with first-class support for **6 locales** out of the box:

- `en-US` (English)
- `vi-VN` (Tiếng Việt)
- `zh-CN` (简体中文)
- `ja-JP` (日本語)
- `es-ES` (Español)
- `fr-FR` (Français)

When adding or changing user-facing strings or diagnostic messages:

1. Update **all 6 locales** in `crates/dataforge-windows-app/src/i18n.rs` for UI labels, menus, modals, tooltips, and status bar text.
2. Update **all 6 locales** in `crates/dataforge-services/src/diagnostics.rs` when adding or modifying connection/query diagnostic messages and remediation hints.
3. Run the startup and localization test suite to verify there are no missing keys or fallback regressions:

```powershell
cargo test --release -j 1 -p dataforge-e2e --test startup_perf_e2e -- --nocapture
```

---

## Pull Request & Commit Conventions

We keep the Git history clean, bisectable, and easy to review:

- **Atomic Per-File Commits**: Stage and commit changes in focused, atomic commits (one logical file or tightly coupled unit per commit where practical).
- **Conventional Commits in English**: Format commit messages using [Conventional Commits](https://www.conventionalcommits.org/):
  - `feat(editor): add schema-qualified CTE autocomplete`
  - `fix(grid): preserve scroll offset when toggling column visibility`
  - `docs(contributing): document release-only build rules and L10n workflow`
  - `test(e2e): add locale coverage check in startup_perf_e2e`
- **Pull Request Checklist**:
  - `cargo check --release -j 1 -p dataforge-windows-app` compiles cleanly without new warnings.
  - `.\target\release\dataforge.exe --smoke-startup` succeeds with `9/9` drivers registered.
  - Any UI changes respect opaque overlays (`1.0` alpha) and single-layer translucent surfaces under `GaussianBlur` and `Mica`.
  - Any new user-facing text is translated across all 6 locales (`en-US`, `vi-VN`, `zh-CN`, `ja-JP`, `es-ES`, `fr-FR`).

---

## Release & Versioning Workflow

DataForge follows [Semantic Versioning](https://semver.org/) and keeps all `crates/dataforge-*/Cargo.toml` crate versions synchronized.

### 1. Maintain `CHANGELOG.md`

- Document notable changes under `## [Unreleased]` in [CHANGELOG.md](CHANGELOG.md) using [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) categories (`Added`, `Changed`, `Fixed`, etc.).
- Use Conventional Commits (`feat`, `fix`, `perf`, `i18n`, `refactor`, `docs`, `ci`, `build`, `chore`) so [cliff.toml](cliff.toml) and [.github/release.yml](.github/release.yml) categorize release notes automatically.

### 2. Bump the Version

Preview a release bump without modifying tracked files:

```powershell
.\scripts\bump-version.ps1 -Bump patch -DryRun
```

Apply a bump (`patch`, `minor`, `major`, or an explicit `-Version X.Y.Z`), update `Cargo.lock` and `CHANGELOG.md`, and create the release commit and annotated `vX.Y.Z` tag:

```powershell
.\scripts\bump-version.ps1 -Bump patch -Tag
```

> [!NOTE]
> `scripts/bump-version.ps1` uses `git-cliff` when installed locally and falls back to built-in Conventional Commit grouping when `git-cliff` is absent.

### 3. Publish via Tag Push

Push the release commit and tag (or pass `-Push` directly to `bump-version.ps1`) to trigger [.github/workflows/release.yml](.github/workflows/release.yml):

```powershell
git push origin main
git push origin vX.Y.Z
```

The release workflow validates that the `vX.Y.Z` tag matches `crates/dataforge-windows-app/Cargo.toml`, runs release tests and `--smoke-startup`, packages `DataForge-win-Setup.exe`, `DataForge-win-Portable.zip`, and Velopack delta/full `.nupkg` assets, generates `SHA256SUMS.txt` and `git-cliff` notes, and publishes the GitHub Release. Maintainers can also run a non-publishing dry run from GitHub Actions (`workflow_dispatch` with `dry_run: true`).

---

## Security & Licensing

- **Reporting Security Vulnerabilities**: Please **do not** report security vulnerabilities in public GitHub issues. Follow the private disclosure instructions in [SECURITY.md](SECURITY.md).
- **License**: By contributing to DataForge, you agree that your contributions will be licensed under the repository's licenses—see [LICENSE-MIT](LICENSE-MIT) (with upstream core attribution to [TableProApp/TablePro](https://github.com/TableProApp/TablePro)) and [LICENSE](LICENSE) (GNU Affero General Public License v3.0).
