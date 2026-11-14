<div align="center">

<img src="crates/dataforge-windows-app/assets/dataforge_logo.png" alt="DataForge Logo" width="112" height="112" />

# DataForge

**基于纯 Rust 与 Direct3D 11 构建的高性能 Windows 原生 SQL 与数据库工作台。**

[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![GPUI](https://img.shields.io/badge/UI-HeroGPUI%20%2F%20Direct3D%2011-0078D4?style=flat-square&logo=windows&logoColor=white)](#为什么选择-dataforge)
[![Drivers](https://img.shields.io/badge/Drivers-9%20Engines-10B981?style=flat-square)](#支持的数据库)
[![Memory](https://img.shields.io/badge/RAM-~55%20MB-F59E0B?style=flat-square)](#实测性能)
[![Startup](https://img.shields.io/badge/冷启动-~10%20ms-6366F1?style=flat-square)](#实测性能)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue?style=flat-square)](LICENSE-MIT)

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [Español](README.es.md) · [Français](README.fr.md)

</div>

---

## 为什么选择 DataForge？

大多数桌面端数据库工具要么基于 Electron（空载就占用数百 MB 甚至 1 GB 以上内存），要么基于 JVM（冷启动缓慢）。

DataForge 从底层驱动到 GPU 渲染界面完全使用 **Rust** 原生编写，采用 **HeroGPUI (Direct3D 11)** 与 **Windows 11 高斯模糊 / Mica** 毛玻璃背景。冷启动仅约 **10 ms**，即使在查询 **1000 万行** SQLite 数据表时内存占用也稳定在 **~55 MB**，同时提供 Schema 智能补全、Vim 模式、表格内联编辑、安全模式保护以及本地优先的 AI SQL 助手。

---

## 界面预览

### SQL 编辑器与千万行查询 (`15 ms`)
![DataForge SQL Editor querying 10 million rows](docs/screenshots/sql-editor-10m.jpg)

### 高密度数据表格与过滤
![DataForge Data Grid view](docs/screenshots/data-grid.jpg)

### 隐私优先的 Schema 感知 AI 助手
![DataForge Schema-Aware AI Assistant](docs/screenshots/ai-assistant.jpg)

---

## 支持的数据库

| 数据库引擎 | 驱动 Crate |
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

## 实测性能

| 测试项 | 实测结果 |
| :--- | :--- |
| **冷启动 (`--smoke-startup`, 9/9 驱动)** | `9.21 – 10.11 ms` |
| **10 万行 SQLite E2E (`sqlite_100k_rows_load_and_scroll_perf`)** | `78.97 ms` 写入 · `24.43 ms` 加载 · `0.432 ms` 万帧滚动 |
| **1000 万行索引查询 (`ORDER BY event_ts DESC LIMIT 1000`)** | `5 – 15 ms` |
| **1000 万行全表 JSON 解析 + 三表 JOIN + 窗口函数** | `40.01 s` |
| **10 万 / 50 万 / 100 万行压力测试内存占用** | 稳定 `~55 MB` |

---

## 从源码构建

```powershell
cargo build --release -p dataforge-windows-app
.\target\release\dataforge.exe --smoke-startup
.\target\release\dataforge.exe
```

## 社区、安全与许可证

- **行为准则**：[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)
- **安全策略**：[SECURITY.md](SECURITY.md)
- **许可证**：[MIT License](LICENSE-MIT)（致谢上游核心项目 [TableProApp/TablePro](https://github.com/TableProApp/TablePro)）及 [LICENSE](LICENSE)。
