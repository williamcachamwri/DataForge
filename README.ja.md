<div align="center">

<img src="crates/dataforge-windows-app/assets/dataforge_logo.png" alt="DataForge Logo" width="112" height="112" />

# DataForge

**Rust と Direct3D 11 で構築された超高速・省メモリな Windows ネイティブ SQL ワークベンチ。**

[![CI](https://img.shields.io/github/actions/workflow/status/williamcachamwri/DataForge/ci.yml?branch=main&style=flat-square&label=CI&logo=github)](https://github.com/williamcachamwri/DataForge/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/actions/workflow/status/williamcachamwri/DataForge/release.yml?style=flat-square&label=Release&logo=github)](https://github.com/williamcachamwri/DataForge/actions/workflows/release.yml)
[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![GPUI](https://img.shields.io/badge/UI-HeroGPUI%20%2F%20Direct3D%2011-0078D4?style=flat-square&logo=windows&logoColor=white)](#dataforge-が目指すもの)
[![Drivers](https://img.shields.io/badge/Drivers-9%20Engines-10B981?style=flat-square)](#対応データベースと接続セキュリティ)
[![Memory](https://img.shields.io/badge/RAM-~55%20MB-F59E0B?style=flat-square)](#実測ベンチマーク)
[![Startup](https://img.shields.io/badge/コールドスタート-~10%20ms-6366F1?style=flat-square)](#実測ベンチマーク)
[![Windows](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-0078D4?style=flat-square&logo=windows11&logoColor=white)](#インストールと自動アップデート)
[![License: MIT / AGPL-3.0](https://img.shields.io/badge/License-MIT%20%2F%20AGPL--3.0-blue?style=flat-square)](LICENSE-MIT)

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [Español](README.es.md) · [Français](README.fr.md)

</div>

---

## DataForge が目指すもの

従来のデスクトップ向けデータベースクライアントは、たいてい二つの妥協のどちらかを強いてきます。アイドル状態でも 1 GB 近い RAM を消費する Electron 製アプリか、手元の SQLite ファイルを開くだけで数秒待たされる JVM ベースのエディタか、そのどちらかです。

DataForge は別のアプローチを取ります。[TablePro](https://github.com/TableProApp/TablePro) の設計思想と Rust 製データベースコアをベースに、WebView を一切使わず **HeroGPUI（Direct3D 11）** 上に Windows ネイティブ UI を直接描画し、**Windows 11 DWM** のマテリアル（**ガウスぼかし / Gaussian Blur**、**Mica**、**Mica Alt**、**Opaque**）を統合しています。

その応答性は即座に体感できます。コールドスタートは約 **10 ms**、**1,000 万行** の SQLite テーブルを検索・スクロールしてもメモリ使用量は **~55 MB** 前後に収まります。それでいて、スキーマ認識オートコンプリート、Vim キーバインド、ステージング付きインライン編集、Safe Mode による書き込み保護、ローカル MCP サーバー、そして行データを外部に送らない AI SQL アシスタントまで標準で備えています。

---

## ビジュアルツアー

### 高速 SQL エディタ、Vim モード & ライブ結果ペイン（1,000 万行クエリ `15 ms`）
*ウィンドウ全体の Windows 11 ガウスぼかし、方言対応のシンタックスハイライト、スキーマ認識補完、Vim モーダル編集（`NORMAL` / `INSERT` / `VISUAL` / `COMMAND`）、分割ライブ結果ビュー。*

![DataForge SQL Editor querying 10 million rows](docs/screenshots/sql-editor-10m.jpg)

---

### 仮想化データグリッド、外部キー & パラメータ化フィルタ
*60+ FPS の仮想化テーブル描画に加え、インラインセル編集と `Commit` / `Rollback` バッチ処理、先頭列の固定、列の表示切替、マルチ演算子パラメータ化フィルタ、構造/DDL/外部キーインスペクタ、アトミックエクスポートを搭載。*

![DataForge Data Grid view](docs/screenshots/data-grid.jpg)

---

### スキーマ限定 AI アシスタント（プライバシー優先）
*作業を妨げない幅 `320px` の右サイドドロワーから、ローカルの Ollama（例: `llama3.2`）や BYOK クラウド API に接続。送信されるのはスキーマメタデータのみで、テーブル内の実データ行が外部へ出ることはありません。*

![DataForge Schema-Aware AI Assistant](docs/screenshots/ai-assistant.jpg)

---

## 機能の詳細

### 対応データベースと接続セキュリティ

DataForge は **9 種類のネイティブドライバ**を単一バイナリに直接組み込んでおり、外部 ODBC ドライバや追加ランタイムは不要です。

| エンジン | ドライバ Crate | 主な特徴 |
| :--- | :--- | :--- |
| **PostgreSQL** | `tablepro-driver-postgres` | `tokio-postgres` + `deadpool`、`COPY IN` 対応、スキーマ/DDL インスペクション |
| **MySQL / MariaDB** | `tablepro-driver-mysql` | SQL 方言とカタログメタデータを包括的にサポート |
| **SQLite** | `tablepro-driver-sqlite` | 設定不要で `.sqlite` / `.db` を直接オープン |
| **DuckDB** | `tablepro-driver-duckdb` | DuckDB 2.0 をバンドルし、`json` / `parquet` を標準サポート |
| **Microsoft SQL Server** | `tablepro-driver-mssql` | 純 Rust TDS 8.0（`tiberius`）、Windows 親和 TLS と複数結果バッチ対応 |
| **ClickHouse** | `tablepro-driver-clickhouse` | `hyper-rustls` 経由の高速 HTTP/TLS 分析クエリ |
| **Cassandra / ScyllaDB** | `tablepro-driver-cassandra` | `rustls` トランスポートを備えたネイティブ CQL ドライバ（`scylla`） |
| **MongoDB** | `tablepro-driver-mongodb` | ドキュメントクエリ、BSON 3 マッピング、DNS Seed List 解決 |
| **Redis** | `tablepro-driver-redis` | 非同期 TLS/TCP によるキー・バリュー閲覧とコマンド実行 |

セキュリティも接続層の中核に組み込まれています：

- **インプロセス SSH トンネリング**：`russh` ベースの SSH クライアント（`tablepro-ssh`）を内蔵し、パスワード認証、秘密鍵認証（パスフレーズ入力ダイアログ付き）、ホスト鍵確認モーダルを外部 `ssh.exe` なしで完結。
- **TLS と Windows システム証明書ストア**：`rustls`（`aws-lc-rs`）＋ `rustls-platform-verifier` により `Disable` / `Prefer` / `Require` / `VerifyCA` / `VerifyFull` をサポート。独自の CA やクライアント証明書も指定できます。
- **Windows 資格情報マネージャー保護**：DB パスワード、SSH 秘密鍵パスフレーズ、AI API キー、MCP トークンは **Windows 資格情報マネージャー**（`CredWriteW` / `CredReadW` ＋ `zeroize` によるメモリ消去）に保存され、設定ファイルやログへ平文出力されません。
- **Safe Mode ガード**：接続ごとに `Off` / `ConfirmWrites` / `ReadOnly` を設定し、意図しない `UPDATE` / `DELETE` / `DROP` の実行を防止します。
- **他クライアントからの接続インポート**：`tablepro-foreign-import` により既存 DB ツールの保存済み接続・クエリを取り込めます。

---

### SQL エディタ & 大容量データグリッド

- **方言対応 SQL エディタ**：
  - `PostgreSQL`、`MySQL`、`SQLite`、`MsSql` など各 DB 方言に応じたリアルタイムハイライトと、UI・構文カラーの独立カスタマイズ。
  - テーブル、カラム、スキーマ、SQL キーワード、句スニペット（`FROM`、`JOIN`、`INTO`、`UPDATE` など）のコンテキスト補完。
  - マルチカーソル、オプションの **Vim モード**（`Normal` / `Insert` / `Visual` / `Command`）、保存済みクエリライブラリ、クエリ履歴を搭載。
  - バッファ全体の実行（`Ctrl+Enter` / `F5`）、選択範囲のみの実行（`Ctrl+Shift+Enter`）、実行中クエリの中断（`Shift+Esc`）に対応。
- **インタラクティブ仮想化データグリッド**：
  - ドッキング可能な **Row Inspector**（`Ctrl+I`）と **構造 / カラム / インデックス / 外部キー / DDL** タブでレコードとスキーマを素早く確認。
  - **インライン編集とステージング**：セル編集、行追加（`Alt+Insert`）、削除マーク（`Ctrl+Shift+Backspace`）を保留バッチとして視覚的に確認し、**Commit**（`Ctrl+Shift+S`）または **Rollback**（`Ctrl+Shift+R`）できます。
  - **先頭列の固定（Freeze）**：ビューポート幅の最大 2/3 まで先頭列を固定可能。列の表示切替やドラッグリサイズにも対応。
  - **パラメータ化カラムフィルタ**：11 種類の SQL インジェクション耐性演算子（`=`、`!=`、`Contains`、`StartsWith`、`EndsWith`、`<`、`<=`、`>`、`>=`、`IS NULL`、`IS NOT NULL`）を組み合わせてフィルタリング。
  - **アトミックエクスポート**：エクスポートモーダル（`Ctrl+Shift+E`）から **CSV**（`.csv`）、**JSON Array**（`.json`）、**JSON Lines**（`.jsonl`）、**SQL `INSERT`**（`.sql`）へ `.part` ステージング経由で安全に書き出し（`tablepro-core` は TSV / XML / 省メモリ XLSX エンコーダも備えています）。

---

### 外観・Windows DWM 背景・多言語対応

- **14 種類の内蔵テーマ + VS Code テーマインポート**：
  - `Dark`、`Light`、`High Contrast`、`Gaussian Glass Dark`、`Gaussian Glass Light`、`Tokyo Night`、`Dracula`、`Catppuccin Mocha`、`One Dark Pro`、`Nord`、`Monokai Pro`、`Gruvbox Dark`、`GitHub Light`、`Catppuccin Latte` の **14 プリセット**を収録。
  - **UI テーマ**と **シンタックス配色パレット**を個別に組み合わせられるほか、**VS Code の `.json` カラーテーマ**も取り込み可能（サイズ・階層・パストラバーサル検証付き）。
- **Windows 11 ネイティブ DWM 背景**：**Gaussian Blur**、**Mica**、**Mica Alt**、**Opaque** を切り替え可能。モーダルやコマンドパレットは不透明度 `1.0` を維持し、背景の文字が透けて読みにくくなるのを防ぎます。
- **視覚効果の軽減（Reduced Motion）**：Windows の `SPI_GETCLIENTAREAANIMATION` を自動検出し、OS 側でアニメーションが無効な場合はドロワーやモーダルを `0 ms` で即座に表示します。
- **6 言語の UI & 診断メッセージ対応**：
  - `en-US` (English) · `vi-VN` (Tiếng Việt) · `zh-CN` (简体中文) · `ja-JP` (日本語) · `es-ES` (Español) · `fr-FR` (Français)

---

### ローカル優先 AI アシスタント & 内蔵 MCP サーバー

- **プライバシー重視 AI ドロワー（`Ctrl+Shift+A`）**：
  - **スキーマ限定送信**：モデルへ送信されるのはテーブル名・カラム名・型・クエリ言語のみ。テーブルの実データ行は一切送信されません。
  - **ローカル & BYOK プロバイダ**：ローカルの **Ollama**（`http://127.0.0.1:11434/v1`、例: `llama3.2`）および **OpenAI 互換エンドポイント** に標準対応し、**OpenAI** や **Anthropic**（API キーは Windows 資格情報マネージャーで保護）も利用できます。
  - ストリーミング応答内の SQL ブロックはシンタックスハイライト付きで表示され、ワンクリックでコピー可能です。
- **内蔵ローカル MCP サーバー（`tablepro-mcp`）**：
  - ループバック HTTP（`127.0.0.1`）経由で外部 IDE やコーディングエージェントへ接続を公開。Bearer トークンは Windows 資格情報マネージャーに保存されます。
  - **11 種類の構造化 MCP ツール**を提供：`list_connections`、`connect`、`list_databases`、`list_schemas`、`list_tables`、`describe_table`、`get_table_ddl`、`browse_table`、`count_rows`、`execute_query`、`export_data`。
  - `execute_query` は各接続の External Clients / Safe Mode 設定に従い、書き込みクエリ実行時には DataForge 画面上で明示的なユーザー承認を求めます。

---

### CLI & ヘッドレスワークフロー

`dataforge.exe` は GUI ランチャーとしてだけでなく、スクリプト実行可能な CLI（`tablepro-cli`）としても機能します：

- **アプリレベルフラグ**：
  - `dataforge.exe --version`（`-V`）— バージョンを表示して終了。
  - `dataforge.exe --help`（`-h`）— 使い方とオプションを表示。
  - `dataforge.exe --smoke-startup` — ヘッドレス起動時間と `9/9` ドライバ登録の検証を実行。
- **直接オープン & シングルインスタンス転送**：
  - `.sqlite` / `.db` / `.duckdb` / `.sql` ファイルや接続 URL（`postgresql://...`）を引数に渡すと、Named Pipe IPC 経由で起動中のウィンドウに開きます（`-n` / `--new-window` で新規ウィンドウ）。
- **CLI サブコマンド**：
  - `open`、`query`、`connections`、`databases`、`schemas`、`tables`、`describe`、`export`、`status`、`completion`（`bash`、`zsh`、`fish`）。
  - `--format`（`query` は `table` / `csv` / `tsv` / `json` / `jsonl`、`export` は `csv` / `json` / `sql`）、`--read-only`、`--limit`、`--timeout` をサポート。

```powershell
# PowerShell から読み取り専用クエリを実行し JSON Lines で出力
.\target\release\dataforge.exe query -c Staging --read-only --format jsonl "SELECT id, email FROM users LIMIT 10"

# テーブルの CREATE TABLE DDL を取得
.\target\release\dataforge.exe describe -c Staging --ddl users
```

---

## 実測ベンチマーク

Windows 11 上で `dataforge.exe` の Release ビルド（`--release`）を用いて計測：

| 計測項目 | 結果 |
| :--- | :--- |
| **コールドスタート（`--smoke-startup`、9/9 ドライバ）** | `9.21 – 10.11 ms` |
| **10 万行 SQLite E2E データ投入（`sqlite_100k_rows_load_and_scroll_perf`）** | `78.97 ms` |
| **10 万行グリッド読込 + 1 万フレームスクロール** | `24.43 ms` 読込 · `0.432 ms` スクロール（`0.043 µs/frame`） |
| **1,000 万行インデックス検索（`ORDER BY event_ts DESC LIMIT 1000`）** | `5 – 15 ms` |
| **1,000 万行フルスキャン + JSON 解析 + 3表 JOIN + 3 Window 関数** | `40.01 s` |
| **10 万 / 50 万 / 100 万行負荷テスト時の RAM 使用量** | 安定 `~55 MB` |

---

## インストールと自動アップデート

[**GitHub Releases**](https://github.com/williamcachamwri/DataForge/releases) から Windows x64 バイナリを入手できます：

- **インストーラー（推奨）— `DataForge-win-Setup.exe`**：
  - `%LocalAppData%` 配下にクリーンにインストールされ、スタートメニューやデスクトップショートカットを作成します（**UAC 管理者権限は不要**）。
  - **Velopack** による高速なバイナリ差分アップデートに対応。起動時の自動更新チェック（**Settings → About** で設定可能）に加え、**Settings → About** から手動チェック・ダウンロード・再起動適用が行えます。
- **ポータブル版 — `DataForge-win-Portable.zip`**：
  - 任意のフォルダに展開して `dataforge.exe` を直接実行できます（インストーラー状態を変更しないスタンドアロンモードで動作）。
- **チェックサム検証**：
  - 各リリースに `SHA256SUMS.txt` が同梱されているため、PowerShell で整合性を確認できます：

```powershell
Get-FileHash .\DataForge-win-Setup.exe -Algorithm SHA256
```

---

## キーボードショートカット一覧

| ショートカット | 操作 |
| :--- | :--- |
| `Ctrl+K` / `Ctrl+Shift+K` / `Ctrl+P` | コマンドパレットを開く |
| `Ctrl+Enter` / `F5` | アクティブな SQL クエリを実行 |
| `Ctrl+Shift+Enter` | 選択中の SQL テキストを実行 |
| `Shift+Esc` | 実行中のクエリをキャンセル |
| `Ctrl+S` | クエリをライブラリに保存 |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | 新規タブ / タブを閉じる / 閉じたタブを再度開く |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | 次のタブ / 前のタブへ移動 |
| `Ctrl+F` | サイドバーフィルタにフォーカス |
| `Ctrl+B` | オブジェクトサイドバーの表示切替 |
| `Ctrl+I` | Row Inspector の表示切替 |
| `Ctrl+Shift+A` | AI アシスタントドロワーの表示切替 |
| `Ctrl+Shift+O` | 接続ライブラリを開く |
| `Ctrl+,` | 設定を開く |
| `Ctrl+Shift+M` | テーマの切り替え |
| `Ctrl+Shift+S` / `Ctrl+Shift+R` | グリッド編集の `Commit` / `Rollback` |
| `Alt+Insert` / `Ctrl+Shift+Backspace` | 行を追加 / 選択行を削除 |
| `Ctrl+Shift+E` | 結果またはテーブルをエクスポート |
| `Esc` | 表示中のモーダルやオーバーレイを閉じる |

*ヒント：SQL エディタのツールバーから **Vim モード** を有効にすると、`NORMAL`、`INSERT`、`VISUAL`、`:` コマンド操作が使えます。*

---

## ソースからのビルド（Windows）

### 前提条件

- **OS**：Windows 10 または Windows 11（x64）
- **Rust ツールチェーン**：Stable MSVC ツールチェーン（`x86_64-pc-windows-msvc`、Rust `2024` Edition）— [rustup](https://rustup.rs/)
- **Visual Studio C++ Build Tools & Windows SDK**：Win32、Direct3D 11（`HeroGPUI`）、同梱 C ドライバのコンパイルに必要
- **シェル**：Windows PowerShell（コマンド連結には `&&` ではなく `;` を使用）

> [!CAUTION]
> **ビルドとテストは必ず `--release` で実行してください**：シンボルデバッグが明確に必要な場合を除き、ワークスペース全体で無指定のデバッグ `cargo build` や `cargo test` を実行しないでください。全ワークスペースと 9 ドライバのデバッグ PDB が `target\debug` 内で **140 GB** を超えることがあります。常に `--release`（メモリに制約がある場合は `-j 1`）を指定してください。

### チェック・ビルド・起動

```powershell
# 既存の dataforge.exe がバイナリをロックしていないか確認
Get-Process dataforge -ErrorAction SilentlyContinue

# Release モードで型チェック
cargo check --release -j 1 -p dataforge-windows-app

# Release バイナリをビルド
cargo build --release -j 1 -p dataforge-windows-app

# コールドスタート & 9/9 ドライバ登録を検証
.\target\release\dataforge.exe --smoke-startup

# DataForge を起動
.\target\release\dataforge.exe
```

### Release テストスイートの実行

```powershell
# コア SQL フィルタ & 方言ユニットテスト
cargo test --release -j 1 -p tablepro-core filter

# 起動性能 & 6 言語ローカライズ整合性テスト
cargo test --release -j 1 -p dataforge-e2e --test startup_perf_e2e -- --nocapture

# 10 万行 SQLite 読込 & 1 万フレームスクロール E2E ベンチマーク
cargo test --release -j 1 -p dataforge-e2e --test sqlite_e2e -- --nocapture
```

---

## アーキテクチャ & ワークスペース構成

```text
DataForge/
├── core/                             # 上流 TablePro コアワークスペース & 9 ネイティブドライバ
│   └── crates/
│       ├── core/                     # tablepro-core: クエリモデル、フィルタ、方言、エクスポータ
│       ├── session/                  # tablepro-session: セッション、接続プール、トランザクション
│       ├── cli/                      # tablepro-cli: シェルサブコマンド & ヘッドレス実行
│       ├── mcp/                      # tablepro-mcp: トークン保護付きローカル MCP HTTP サーバー
│       ├── ai/                       # tablepro-ai: スキーマ限定 Ollama / OpenAI / Anthropic クライアント
│       ├── ssh/                      # tablepro-ssh: russh によるインプロセス SSH トンネル
│       ├── net/                      # tablepro-net: rustls TLS + システム証明書検証
│       ├── storage/                  # tablepro-storage: 保存済み接続、ライブラリ、クエリ履歴
│       ├── foreign-import/           # tablepro-foreign-import: 他 DB クライアントからのインポート
│       ├── account/                  # tablepro-account: アカウント & トークン抽象
│       └── drivers/                  # postgres, mysql, sqlite, duckdb, mssql, clickhouse,
│                                     # cassandra, mongodb, redis
└── crates/
    ├── dataforge-gfx/                # ベクターグリフ & DB エンジンアイコン
    ├── dataforge-state/              # エディタ状態、補完、グリッドモデル、タブ、14 内蔵テーマ
    ├── dataforge-services/           # 非同期 Tokio ランタイム、セッション/AI/MCP 制御、診断メッセージ
    ├── dataforge-windows-platform/   # 設定 JSON、Windows 資格情報マネージャー、DWM & 視覚効果判定
    ├── dataforge-windows-app/        # HeroGPUI (Direct3D 11) アプリ、Velopack 更新機能、6 言語対応
    └── dataforge-e2e/                # SQLite / DuckDB / 起動性能 / グリッド E2E ベンチマーク
```

---

## コントリビュート・セキュリティ・行動規範・ライセンス

- **コントリビュート**：アーキテクチャ方針、Release ビルド規則、HeroGPUI オーバーレイ規約、6 言語 L10n ルールは [CONTRIBUTING.md](CONTRIBUTING.md) をご覧ください。
- **変更履歴**：[CHANGELOG.md](CHANGELOG.md) をご覧ください。
- **行動規範**：[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) をご覧ください。
- **セキュリティポリシー**：脆弱性の非公開報告手順およびローカル優先のプライバシー保証については [SECURITY.md](SECURITY.md) をご覧ください。
- **ライセンス**：DataForge は [LICENSE](LICENSE)（GNU Affero General Public License v3.0）および [LICENSE-MIT](LICENSE-MIT) のもとで提供されており、上流コア [TableProApp/TablePro](https://github.com/TableProApp/TablePro)（MIT）への帰属を明記しています。
