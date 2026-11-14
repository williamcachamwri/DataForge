<div align="center">

<img src="crates/dataforge-windows-app/assets/dataforge_logo.png" alt="DataForge Logo" width="112" height="112" />

# DataForge

**基于纯 Rust 与 Direct3D 11 构建的高性能 Windows 原生 SQL 与数据库工作台。**

[![CI](https://img.shields.io/github/actions/workflow/status/williamcachamwri/DataForge/ci.yml?branch=main&style=flat-square&label=CI&logo=github)](https://github.com/williamcachamwri/DataForge/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/actions/workflow/status/williamcachamwri/DataForge/release.yml?style=flat-square&label=Release&logo=github)](https://github.com/williamcachamwri/DataForge/actions/workflows/release.yml)
[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![GPUI](https://img.shields.io/badge/UI-HeroGPUI%20%2F%20Direct3D%2011-0078D4?style=flat-square&logo=windows&logoColor=white)](#为什么选择-dataforge)
[![Drivers](https://img.shields.io/badge/Drivers-9%20Engines-10B981?style=flat-square)](#支持的数据库与连接安全)
[![Memory](https://img.shields.io/badge/RAM-~55%20MB-F59E0B?style=flat-square)](#实测性能)
[![Startup](https://img.shields.io/badge/冷启动-~10%20ms-6366F1?style=flat-square)](#实测性能)
[![Windows](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-0078D4?style=flat-square&logo=windows11&logoColor=white)](#安装与自动更新)
[![License: MIT / AGPL-3.0](https://img.shields.io/badge/License-MIT%20%2F%20AGPL--3.0-blue?style=flat-square)](LICENSE-MIT)

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [Español](README.es.md) · [Français](README.fr.md)

</div>

---

## 为什么选择 DataForge

市面上的桌面数据库客户端大多让人进退两难：要么套着 Electron 壳子，开个空白标签页就能吃掉上 GB 内存；要么基于 JVM，想打开一个本地 SQLite 文件还得等上好几秒冷启动。

DataForge 选择了一条更直接的路径。它受 [TablePro](https://github.com/TableProApp/TablePro) 启发并复用其 Rust 数据库内核，彻底抛弃 WebView，转而使用 **HeroGPUI（Direct3D 11）** 直接在 GPU 上绘制 Windows 原生界面，并深度融合 **Windows 11 DWM** 窗口材质（**高斯模糊 Gaussian Blur**、**Mica**、**Mica Alt** 与 **不透明 Opaque**）。

最终呈现出的响应速度几乎是瞬时的：冷启动约 **10 ms**，即便查询和滚动 **1000 万行** SQLite 数据表，进程内存也稳定在 **~55 MB** 左右；同时你日常所需的顺手功能一应俱全——Schema 感知智能补全、可选 Vim 模式、表格内联暂存编辑、安全模式防护、内置本地 MCP 服务，以及严格不泄露行数据的 AI SQL 助手。

---

## 界面导览

### 高速 SQL 编辑器、Vim 模式与实时结果窗格（1000 万行查询 `15 ms`）
*全窗口 Windows 11 高斯模糊背景、方言感知语法高亮、Schema 智能补全、可选 Vim 模式编辑（`NORMAL` / `INSERT` / `VISUAL` / `COMMAND`），以及分栏实时结果面板。*

![DataForge SQL Editor querying 10 million rows](docs/screenshots/sql-editor-10m.jpg)

---

### 虚拟化数据网格、外键检查与参数化筛选
*60+ FPS 虚拟化表格渲染，支持单元格内联编辑与批量 `Commit` / `Rollback`、前导列冻结、列显隐与拖拽调宽、多运算符参数化过滤、表结构/DDL/外键查看，以及原子化文件导出。*

![DataForge Data Grid view](docs/screenshots/data-grid.jpg)

---

### 仅传 Schema 的隐私优先 AI 助手
*右侧 `320px` 非阻塞 AI 抽屉面板，默认对接本地 Ollama（如 `llama3.2`）或自带密钥的云端模型。只有表结构元数据会离开本机，你的业务行数据永远留在地端。*

![DataForge Schema-Aware AI Assistant](docs/screenshots/ai-assistant.jpg)

---

## 核心功能详解

### 支持的数据库与连接安全

DataForge 将 **9 种原生数据库驱动**直接编译进单个可执行文件，无需额外安装 ODBC 驱动或运行时插件：

| 数据库引擎 | 驱动 Crate | 核心特性 |
| :--- | :--- | :--- |
| **PostgreSQL** | `tablepro-driver-postgres` | 基于 `tokio-postgres` + `deadpool`，支持 `COPY IN` 与完整 Schema/DDL 检查 |
| **MySQL / MariaDB** | `tablepro-driver-mysql` | 完整 SQL 方言与目录元数据支持 |
| **SQLite** | `tablepro-driver-sqlite` | 零配置打开本地 `.sqlite` / `.db` 文件，支持 CLI 与文件直开 |
| **DuckDB** | `tablepro-driver-duckdb` | 内置 DuckDB 2.0，原生链接 `json` 与 `parquet` 能力 |
| **Microsoft SQL Server** | `tablepro-driver-mssql` | 纯 Rust TDS 8.0（`tiberius`），适配 Windows TLS 与多结果集批处理 |
| **ClickHouse** | `tablepro-driver-clickhouse` | 基于 `hyper-rustls` 的原生 HTTP/TLS 分析查询 |
| **Cassandra / ScyllaDB** | `tablepro-driver-cassandra` | 原生 CQL 驱动（`scylla`）搭配 `rustls` 加密传输 |
| **MongoDB** | `tablepro-driver-mongodb` | 文档查询、BSON 3 类型映射及 DNS Seed List 解析 |
| **Redis** | `tablepro-driver-redis` | 异步 TLS/TCP 键值浏览与命令执行 |

连接安全性贯穿整个底层设计：

- **进程内 SSH 隧道**：内置 `russh` 客户端（`tablepro-ssh`），支持密码与私钥认证、交互式私钥口令弹窗及主机指纹核验——无需外部 `ssh.exe`。
- **TLS 与 Windows 系统证书库**：基于 `rustls`（`aws-lc-rs`）与 `rustls-platform-verifier`，提供 `Disable`、`Prefer`、`Require`、`VerifyCA`、`VerifyFull` 五种模式，并可指定自定义 CA、客户端证书与私钥。
- **Windows 凭据管理器保险库**：数据库密码、SSH 密钥口令、AI API Key 与 MCP 访问令牌统一托管于 **Windows 凭据管理器**（`CredWriteW` / `CredReadW` 配合 `zeroize` 内存擦除），绝不以明文落盘或写入日志。
- **安全模式（Safe Mode）护栏**：支持按连接设置 `Off`、`ConfirmWrites` 或 `ReadOnly`，在执行 `UPDATE`、`DELETE`、`DROP` 等变更语句前强制弹窗确认或直接拦截。
- **外部客户端连接导入**：通过 `tablepro-foreign-import` 导入其他数据库客户端已保存的连接与查询。

---

### SQL 编辑器与大数据量网格

- **方言感知 SQL 编辑器**：
  - 按引擎方言实时语法高亮（`PostgreSQL`、`MySQL`、`SQLite`、`MsSql` 等），UI 主题与代码配色独立可控。
  - 上下文智能补全：表名、列名、Schema、SQL 关键字及语句片段（`FROM`、`JOIN`、`INTO`、`UPDATE` 等）。
  - 支持多光标编辑、可选 **Vim 模式**（`Normal`、`Insert`、`Visual`、`Command`）、已保存查询库与查询历史。
  - 支持运行整页 SQL（`Ctrl+Enter` / `F5`）、仅运行选中片段（`Ctrl+Shift+Enter`）或随时中止执行中的查询（`Shift+Esc`）。
- **高交互虚拟化数据网格**：
  - 流畅滚动海量结果集，配合侧边 **行检查器 Row Inspector**（`Ctrl+I`）及 **结构 / 列 / 索引 / 外键 / DDL** 视图。
  - **单元格内联编辑**：直接修改单元格、插入新行（`Alt+Insert`）或标记删除（`Ctrl+Shift+Backspace`），所有改动先暂存，确认无误后再 **Commit**（`Ctrl+Shift+S`）或 **Rollback**（`Ctrl+Shift+R`）。
  - **前导列冻结**：固定前导列（最多占视口宽度的 2/3）、切换列可见性并支持拖拽调整列宽。
  - **参数化列筛选**：支持 11 种方言安全运算符组合过滤（`=`、`!=`、`Contains`、`StartsWith`、`EndsWith`、`<`、`<=`、`>`、`>=`、`IS NULL`、`IS NOT NULL`）。
  - **原子化数据导出**：通过导出弹窗（`Ctrl+Shift+E`）导出 **CSV**（`.csv`）、**JSON 数组**（`.json`）、**JSON Lines**（`.jsonl`）或 **SQL `INSERT`**（`.sql`），先写入 `.part` 临时文件再原子重命名（`tablepro-core` 另内置 TSV、XML 与恒定内存 XLSX 编码器）。

---

### 外观主题、Windows DWM 背景与多语言

- **14 款内置主题 + VS Code 主题导入**：
  - 开箱即用 **14 套精选预设**：`Dark`、`Light`、`High Contrast`、`Gaussian Glass Dark`、`Gaussian Glass Light`、`Tokyo Night`、`Dracula`、`Catppuccin Mocha`、`One Dark Pro`、`Nord`、`Monokai Pro`、`Gruvbox Dark`、`GitHub Light` 与 `Catppuccin Latte`。
  - 支持将 **UI 界面主题** 与 **SQL 语法配色板** 自由混搭，也支持直接导入 **VS Code `.json` 配色文件**（内置大小限制与路径逃逸校验）。
- **Windows 11 原生 DWM 材质**：可在 **Gaussian Blur（毛玻璃）**、**Mica**、**Mica Alt** 与 **Opaque（纯色不透明）** 间切换，同时保持弹窗、命令面板与菜单背景 `1.0` 不透明以确保文字清晰锐利。
- **减弱动态效果适配**：自动检测 Windows `SPI_GETCLIENTAREAANIMATION` 系统设置，开启减弱动画时抽屉与弹窗即时以 `0 ms` 切换。
- **6 种完整 UI 与诊断语言**：
  - `en-US` (English) · `vi-VN` (Tiếng Việt) · `zh-CN` (简体中文) · `ja-JP` (日本語) · `es-ES` (Español) · `fr-FR` (Français)

---

### 本地优先 AI 助手与内置 MCP 服务

- **隐私优先 AI 抽屉（`Ctrl+Shift+A`）**：
  - **仅传 Schema 承诺**：仅将表名、字段名、类型与 SQL 方言发给模型，表内行数据绝不会上传。
  - **本地模型与 BYOK 云端**：默认支持本地 **Ollama**（`http://127.0.0.1:11434/v1`，如 `llama3.2`）及任意 **OpenAI 兼容接口**，也支持 **OpenAI** 与 **Anthropic**（API 密钥安全存于 Windows 凭据管理器）。
  - 实时流式输出带语法高亮的 SQL 代码块，可一键复制使用。
- **内置本地 MCP 服务（`tablepro-mcp`）**：
  - 通过回环地址（`127.0.0.1`）向 IDE 或编程 Agent 暴露已打开的数据库连接，由 Windows 凭据管理器托管 Bearer Token 并校验本地调用方。
  - 提供 **11 个结构化 MCP 工具**：`list_connections`、`connect`、`list_databases`、`list_schemas`、`list_tables`、`describe_table`、`get_table_ddl`、`browse_table`、`count_rows`、`execute_query` 与 `export_data`。
  - 其中 `execute_query` 严格遵守连接的 External Clients 与 Safe Mode 策略，任何写入操作均需用户在 DataForge 弹窗中明确批准。

---

### CLI 与无头工作流

`dataforge.exe` 既是桌面端入口，也是可直接用于脚本的 CLI 工具（`tablepro-cli`）：

- **应用级参数**：
  - `dataforge.exe --version`（`-V`）— 打印版本号并退出。
  - `dataforge.exe --help`（`-h`）— 打印帮助信息。
  - `dataforge.exe --smoke-startup` — 执行无头冷启动耗时与 `9/9` 驱动注册探针测试。
- **直开文件 / URL 与单实例转发**：
  - 直接传入 `.sqlite` / `.db` / `.duckdb` / `.sql` 文件或连接串（如 `postgresql://...`），即可通过 Named Pipe IPC 在已有窗口中打开（或加 `-n` / `--new-window` 新开窗口）。
- **CLI 子命令**：
  - `open`、`query`、`connections`、`databases`、`schemas`、`tables`、`describe`、`export`、`status` 与 `completion`（支持 `bash`、`zsh`、`fish`）。
  - 支持 `--format`（`query` 支持 `table`、`csv`、`tsv`、`json`、`jsonl`；`export` 支持 `csv`、`json`、`sql`）、`--read-only`、`--limit` 与 `--timeout`。

```powershell
# 在 PowerShell 中以只读方式执行查询并输出 JSON Lines
.\target\release\dataforge.exe query -c Staging --read-only --format jsonl "SELECT id, email FROM users LIMIT 10"

# 导出表的 CREATE TABLE DDL
.\target\release\dataforge.exe describe -c Staging --ddl users
```

---

## 实测性能

在 Windows 11 上使用 `dataforge.exe` 的 Release 构建（`--release`）测得：

| 测试项 | 实测结果 |
| :--- | :--- |
| **冷启动（`--smoke-startup`，9/9 驱动）** | `9.21 – 10.11 ms` |
| **10 万行 SQLite E2E 数据写入（`sqlite_100k_rows_load_and_scroll_perf`）** | `78.97 ms` |
| **10 万行网格加载 + 1 万帧滚动** | `24.43 ms` 加载 · `0.432 ms` 滚动（`0.043 µs/帧`） |
| **1000 万行索引查询（`ORDER BY event_ts DESC LIMIT 1000`）** | `5 – 15 ms` |
| **1000 万行全表 JSON 解析 + 三表 JOIN + 3 个窗口函数** | `40.01 s` |
| **10 万 / 50 万 / 100 万行压力查询期间内存占用** | 稳定 `~55 MB` |

---

## 安装与自动更新

前往 [**GitHub Releases**](https://github.com/williamcachamwri/DataForge/releases) 下载 Windows x64 预编译包：

- **安装版（推荐）— `DataForge-win-Setup.exe`**：
  - 安装至当前用户的 `%LocalAppData%` 目录并创建开始菜单与桌面快捷方式，**无需 UAC 管理员权限**。
  - 内置 **Velopack** 增量更新引擎：启动时自动检查更新（可在 **Settings → About** 中开关），也可在 **Settings → About** 中手动检查、下载并一键重启完成升级。
- **绿色便携版 — `DataForge-win-Portable.zip`**：
  - 解压至任意目录即可直接运行 `dataforge.exe`，以独立模式（Standalone）工作，不写入安装器注册信息。
- **校验下载完整性**：
  - 每个版本均随附 `SHA256SUMS.txt`，可在 PowerShell 中核对哈希值：

```powershell
Get-FileHash .\DataForge-win-Setup.exe -Algorithm SHA256
```

---

## 快捷键速查表

| 快捷键 | 功能 |
| :--- | :--- |
| `Ctrl+K` / `Ctrl+Shift+K` / `Ctrl+P` | 打开命令面板 |
| `Ctrl+Enter` / `F5` | 执行当前 SQL |
| `Ctrl+Shift+Enter` | 仅执行选中的 SQL 文本 |
| `Shift+Esc` | 中止正在运行的查询 |
| `Ctrl+S` | 保存当前查询到查询库 |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | 新建标签页 / 关闭标签页 / 重新打开已关闭标签页 |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | 切换到下一个 / 上一个标签页 |
| `Ctrl+F` | 聚焦左侧对象树过滤框 |
| `Ctrl+B` | 显示/隐藏左侧对象栏 |
| `Ctrl+I` | 显示/隐藏行检查器（Row Inspector） |
| `Ctrl+Shift+A` | 显示/隐藏 AI 助手抽屉 |
| `Ctrl+Shift+O` | 打开连接库 |
| `Ctrl+,` | 打开设置 |
| `Ctrl+Shift+M` | 切换明暗/主题 |
| `Ctrl+Shift+S` / `Ctrl+Shift+R` | 提交（`Commit`）/ 回滚（`Rollback`）网格暂存修改 |
| `Alt+Insert` / `Ctrl+Shift+Backspace` | 新增行 / 删除选中行 |
| `Ctrl+Shift+E` | 导出查询结果或数据表 |
| `Esc` | 关闭当前弹窗或浮层 |

*提示：在 SQL 编辑器工具栏开启 **Vim 模式** 后，即可使用 `NORMAL`、`INSERT`、`VISUAL` 与 `:` 命令模式进行键盘操作。*

---

## 从源码构建（Windows）

### 环境要求

- **操作系统**：Windows 10 或 Windows 11（x64）
- **Rust 工具链**：Stable MSVC 工具链（`x86_64-pc-windows-msvc`，Rust `2024` Edition），通过 [rustup](https://rustup.rs/) 安装
- **Visual Studio C++ Build Tools & Windows SDK**：编译 Win32、Direct3D 11（`HeroGPUI`）及内置 C 驱动所需
- **终端环境**：Windows PowerShell（链式执行命令请使用 `;` 而非 `&&`）

> [!CAUTION]
> **请始终使用 `--release` 构建与测试**：除非明确需要排查底层符号，切勿对整个工作区运行未配置的 Debug `cargo build` 或 `cargo test`。全量工作区与 9 个驱动在 `target\debug` 下生成的 Debug PDB 文件可超过 **140 GB**。请始终加上 `--release`（内存受限时请加 `-j 1`）。

### 检查、编译与运行

```powershell
# 编译前确认没有正在运行的 dataforge.exe 占用目标文件
Get-Process dataforge -ErrorAction SilentlyContinue

# Release 模式类型检查
cargo check --release -j 1 -p dataforge-windows-app

# 编译 Release 可执行文件
cargo build --release -j 1 -p dataforge-windows-app

# 验证冷启动与 9/9 驱动注册
.\target\release\dataforge.exe --smoke-startup

# 启动 DataForge
.\target\release\dataforge.exe
```

### 运行 Release 测试套件

```powershell
# 核心 SQL 过滤与方言单元测试
cargo test --release -j 1 -p tablepro-core filter

# 启动性能与 6 语言本地化一致性测试
cargo test --release -j 1 -p dataforge-e2e --test startup_perf_e2e -- --nocapture

# 10 万行 SQLite 加载与万帧滚动 E2E 基准测试
cargo test --release -j 1 -p dataforge-e2e --test sqlite_e2e -- --nocapture
```

---

## 架构与工作区目录

```text
DataForge/
├── core/                             # 上游 TablePro 数据库内核与 9 个原生驱动
│   └── crates/
│       ├── core/                     # tablepro-core：查询模型、过滤、方言、导出编码器
│       ├── session/                  # tablepro-session：会话管理、连接池、事务状态
│       ├── cli/                      # tablepro-cli：CLI 子命令与无头执行器
│       ├── mcp/                      # tablepro-mcp：Token 鉴权的本地环回 MCP HTTP 服务
│       ├── ai/                       # tablepro-ai：仅传 Schema 的 Ollama / OpenAI / Anthropic 客户端
│       ├── ssh/                      # tablepro-ssh：基于 russh 的进程内 SSH 隧道
│       ├── net/                      # tablepro-net：rustls TLS 与系统证书校验
│       ├── storage/                  # tablepro-storage：连接库、收藏、查询历史持久化
│       ├── foreign-import/           # tablepro-foreign-import：第三方客户端连接导入
│       ├── account/                  # tablepro-account：账户与令牌抽象
│       └── drivers/                  # postgres, mysql, sqlite, duckdb, mssql, clickhouse,
│                                     # cassandra, mongodb, redis
└── crates/
    ├── dataforge-gfx/                # 矢量字形与数据库引擎图标
    ├── dataforge-state/              # 编辑器状态、自动补全、网格模型、标签页与 14 套内置主题
    ├── dataforge-services/           # 异步 Tokio 运行时、会话/AI/MCP 编排与多语言诊断
    ├── dataforge-windows-platform/   # 设置 JSON、Windows 凭据管理器、DWM 与减弱动画检测
    ├── dataforge-windows-app/        # HeroGPUI (Direct3D 11) 桌面界面、Velopack 更新器与 6 语言包
    └── dataforge-e2e/                # SQLite、DuckDB、冷启动与网格滚动端到端基准测试
```

---

## 贡献、安全、行为准则与许可证

- **参与贡献**：请参阅 [CONTRIBUTING.md](CONTRIBUTING.md) 了解架构说明、Release 构建规范、HeroGPUI 浮层约束及 6 语言本地化要求。
- **更新日志**：请参阅 [CHANGELOG.md](CHANGELOG.md)。
- **行为准则**：请参阅 [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)。
- **安全策略**：请参阅 [SECURITY.md](SECURITY.md) 了解漏洞私密披露流程与本地隐私保障。
- **开源许可证**：DataForge 采用 [LICENSE](LICENSE)（GNU Affero General Public License v3.0）与 [LICENSE-MIT](LICENSE-MIT) 授权，并向上游核心项目 [TableProApp/TablePro](https://github.com/TableProApp/TablePro)（MIT）致谢。
