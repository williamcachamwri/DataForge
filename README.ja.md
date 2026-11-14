<div align="center">

<img src="crates/dataforge-windows-app/assets/dataforge_logo.png" alt="DataForge Logo" width="112" height="112" />

# DataForge

**Rust と Direct3D 11 で構築された超高速・省メモリな Windows ネイティブ SQL ワークベンチ。**

[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![GPUI](https://img.shields.io/badge/UI-HeroGPUI%20%2F%20Direct3D%2011-0078D4?style=flat-square&logo=windows&logoColor=white)](#dataforge-の特徴)
[![Drivers](https://img.shields.io/badge/Drivers-9%20Engines-10B981?style=flat-square)](#対応データベース)
[![Memory](https://img.shields.io/badge/RAM-~55%20MB-F59E0B?style=flat-square)](#実測ベンチマーク)
[![Startup](https://img.shields.io/badge/コールドスタート-~10%20ms-6366F1?style=flat-square)](#実測ベンチマーク)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue?style=flat-square)](LICENSE-MIT)

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [Español](README.es.md) · [Français](README.fr.md)

</div>

---

## DataForge の特徴

従来のデータベースクライアントは、Electron 製で 1 GB 近くのメモリを消費したり、JVM ベースで起動に数秒かかったりすることが珍しくありません。

DataForge はドライバ層から GPU レンダリング UI（**HeroGPUI / Direct3D 11** + **Windows 11 ガウスぼかし / Mica**）まですべて **Rust** でネイティブ実装されています。コールドスタートは約 **10 ms**、1,000 万行の SQLite テーブルをクエリしてもメモリ使用量は **~55 MB** に抑えられます。

---

## スクリーンショット

### SQL エディタ & 1,000 万行クエリ (`15 ms`)
![DataForge SQL Editor querying 10 million rows](docs/screenshots/sql-editor-10m.jpg)

### 高密度データグリッド & フィルタリング
![DataForge Data Grid view](docs/screenshots/data-grid.jpg)

### スキーマ認識 AI アシスタント（ローカル優先・プライバシー保護）
![DataForge Schema-Aware AI Assistant](docs/screenshots/ai-assistant.jpg)

---

## 対応データベース

| エンジン | ドライバ Crate |
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

## 実測ベンチマーク

| 計測項目 | 結果 |
| :--- | :--- |
| **コールドスタート (`--smoke-startup`, 9/9 ドライバ)** | `9.21 – 10.11 ms` |
| **10 万行 SQLite E2E (`sqlite_100k_rows_load_and_scroll_perf`)** | `78.97 ms` 投入 · `24.43 ms` 読込 · `0.432 ms` スクロール |
| **1,000 万行インデックス検索 (`LIMIT 1000`)** | `5 – 15 ms` |
| **1,000 万行フルスキャン + `json_extract` + 3表 JOIN + Window 関数** | `40.01 s` |
| **10 万 / 50 万 / 100 万行負荷時の RAM 使用量** | 安定 `~55 MB` |

---

## ビルド方法

```powershell
cargo build --release -p dataforge-windows-app
.\target\release\dataforge.exe --smoke-startup
.\target\release\dataforge.exe
```

## コミュニティ・セキュリティ・ライセンス

- **行動規範**: [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)
- **セキュリティポリシー**: [SECURITY.md](SECURITY.md)
- **ライセンス**: [MIT License](LICENSE-MIT)（上流コア [TableProApp/TablePro](https://github.com/TableProApp/TablePro) への帰属表記を含む）および [LICENSE](LICENSE)。
