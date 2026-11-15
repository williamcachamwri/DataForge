<div align="center">

<img src="crates/tableminus-windows-app/assets/tableminus_logo.png" alt="TableMinus Logo" width="112" height="112" />

# TableMinus

**Rust と Direct3D 11 で構築された Windows ネイティブ SQL・データベースワークベンチ。**

[![CI](https://img.shields.io/github/actions/workflow/status/williamcachamwri/TableMinus/ci.yml?branch=main&style=flat-square&label=CI&logo=github)](https://github.com/williamcachamwri/TableMinus/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/actions/workflow/status/williamcachamwri/TableMinus/release.yml?style=flat-square&label=Release&logo=github)](https://github.com/williamcachamwri/TableMinus/actions/workflows/release.yml)
[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![GPUI](https://img.shields.io/badge/UI-HeroGPUI%20%2F%20Direct3D%2011-0078D4?style=flat-square&logo=windows&logoColor=white)](#概要)
[![Drivers](https://img.shields.io/badge/Drivers-9%20Engines-10B981?style=flat-square)](#対応データベース)
[![Memory](https://img.shields.io/badge/RAM-~55%20MB-F59E0B?style=flat-square)](#概要)
[![Startup](https://img.shields.io/badge/コールドスタート-~10%20ms-6366F1?style=flat-square)](#概要)
[![Windows](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-0078D4?style=flat-square&logo=windows11&logoColor=white)](#ダウンロードとインストール)
[![License: MIT / AGPL-3.0](https://img.shields.io/badge/License-MIT%20%2F%20AGPL--3.0-blue?style=flat-square)](LICENSE-MIT)

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [Español](README.es.md) · [Français](README.fr.md)

</div>

---

## 概要

TableMinus は Rust で書かれた Windows 向けのネイティブ SQL・データベースワークベンチです。UI 描画には Direct3D 11 上で動く [HeroGPUI](https://github.com/williamcachamwri/TableMinus) を採用し、データベース処理にはオープンソースの [TablePro](https://github.com/TableProApp/TablePro) Rust コアを利用しているため、Electron や JVM ランタイムは不要です。

Windows 11 のリリースビルドでは、コールドスタートが約 `10 ms`（9 ドライバ登録込みの `--smoke-startup` 実測値で `9.21` から `10.11 ms`）、`100k` / `500k` / `1M` 行の負荷クエリ実行中でもプロセスのメモリ使用量は `~55 MB` 前後に収まります。`tableminus-e2e` の SQLite テストスイートでは、`100k` 行のグリッド読み込みが `24.43 ms`、`10k` フレームのスクロールが `0.432 ms` で完了します。

---

## スクリーンショット

### SQL エディタ、Vim モード、分割結果ペイン（1,000 万行のインデックス付きクエリ `15 ms`）

![TableMinus SQL Editor querying 10 million rows](docs/screenshots/sql-editor-10m.jpg)

### 仮想化データグリッド、構造インスペクタ、パラメータ化フィルタ

![TableMinus Data Grid view](docs/screenshots/data-grid.jpg)

### スキーマ情報のみを送る AI ドロワー（`Ctrl+Shift+A`）

![TableMinus Schema-Aware AI Assistant](docs/screenshots/ai-assistant.jpg)

### インタラクティブ ER 図、外部キーリレーション、SVG エクスポート

![TableMinus ER Diagram Workbench](docs/screenshots/er-diagram-workbench.jpg)

---

## 対応データベース

TableMinus は 9 種類のデータベースドライバを `tableminus.exe` に直接コンパイルして内蔵しているため、別途 ODBC ドライバやプラグインを入れる必要はありません。

| データベース | ドライバ Crate | 備考 |
| :--- | :--- | :--- |
| PostgreSQL | `tablepro-driver-postgres` | `tokio-postgres` + `deadpool`、`COPY IN`、スキーマ・DDL 表示 |
| MySQL / MariaDB | `tablepro-driver-mysql` | SQL 方言とカタログメタデータ対応 |
| SQLite | `tablepro-driver-sqlite` | ファイルを開く・ドラッグ&ドロップ・CLI から `.sqlite` / `.db` を直接開く |
| DuckDB | `tablepro-driver-duckdb` | `json` と `parquet` を組み込んだバンドル版 DuckDB 2.0 |
| Microsoft SQL Server | `tablepro-driver-mssql` | 純 Rust 製 TDS 8.0（`tiberius`）、TLS と複数結果セット対応 |
| ClickHouse | `tablepro-driver-clickhouse` | `hyper-rustls` による HTTP/TLS クエリ |
| Cassandra / ScyllaDB | `tablepro-driver-cassandra` | `rustls` 上で動作するネイティブ CQL ドライバ（`scylla`） |
| MongoDB | `tablepro-driver-mongodb` | ドキュメントクエリ、BSON 3 マッピング、DNS シードリスト解決 |
| Redis | `tablepro-driver-redis` | 非同期 TLS/TCP でのキー・バリュー閲覧とコマンド実行 |

接続設定では `russh` によるプロセス内 SSH トンネリング（`tablepro-ssh`、パスワード・秘密鍵・パスフレーズ入力・ホスト鍵検証に対応）、`rustls` と Windows 証明書ストアを使った TLS（`Disable`、`Prefer`、`Require`、`VerifyCA`、`VerifyFull`、カスタム CA とクライアント証明書/鍵）、セーフモード（`Off`、`ConfirmWrites`、`ReadOnly`）、および `tablepro-foreign-import` による他ツールからの接続情報インポートに対応しています。

---

## 主な機能

### SQL エディタとデータグリッド

エディタは方言ごとのシンタックスハイライト（`PostgreSQL`、`MySQL`、`SQLite`、`MsSql` など）、テーブル・カラム・スキーマ・キーワード・SQL 句（`FROM`、`JOIN`、`INTO`、`UPDATE`）の補完、マルチカーソル編集、保存済みクエリライブラリ、クエリ履歴、そしてオプションの Vim モード（`NORMAL`、`INSERT`、`VISUAL`、`COMMAND`）を備えています。

データグリッドではドック型の Row Inspector（`Ctrl+I`）での行確認、Structure / Columns / Indexes / Foreign Keys / DDL タブの切り替え、ビューポート幅の 2/3 までの先頭列固定、列の表示切替・リサイズ、11 種類の演算子（`=`、`!=`、`Contains`、`StartsWith`、`EndsWith`、`&lt;`、`&lt;=`、`&gt;`、`&gt;=`、`IS NULL`、`IS NOT NULL`）を使ったパラメータ化フィルタの重ね掛けが可能です。セル編集、行追加（`Alt+Insert`）、削除マーク（`Ctrl+Shift+Backspace`）はバッチとして保留され、`Commit`（`Ctrl+Shift+S`）または `Rollback`（`Ctrl+Shift+R`）で確定・取り消しできます。

クエリ結果やテーブルは `Ctrl+Shift+E` から `.part` 一時ファイル経由で CSV（`.csv`）、JSON Array（`.json`）、JSON Lines（`.jsonl`）、SQL `INSERT`（`.sql`）へアトミックに書き出せます。`tablepro-core` には TSV、XML、省メモリ動作の XLSX エンコーダも含まれています。

### セキュリティとローカル保存

データベースのパスワード、SSH 秘密鍵のパスフレーズ、AI API キー、MCP ベアラートークンは平文の JSON 設定ファイルやログには書かず、Windows 資格情報マネージャー（`CredWriteW` / `CredReadW`）に保存し、使用後のメモリは `zeroize` で消去します。セーフモードで接続を `ConfirmWrites` または `ReadOnly` に設定すれば、`UPDATE`、`DELETE`、`DROP` の実行前に確認を求めるか、実行自体をブロックできます。

### AI アシスタントと MCP サーバー

AI ドロワー（`Ctrl+Shift+A`）はローカルの Ollama（`http://127.0.0.1:11434/v1`）、OpenAI 互換エンドポイント、OpenAI、または Anthropic に接続できます。モデルへ送信されるのはスキーマ情報（テーブル名、カラム名、データ型、SQL 方言）のみで、テーブル内の行データが外部に出ることはありません。

外部のコーディングエージェントや IDE 向けには、`tablepro-mcp` が `127.0.0.1` 上でローカル HTTP MCP サーバーを起動します。Windows 資格情報マネージャーのベアラートークンと接続元プロセスの検証で保護されており、次の 11 ツールを提供します。

- `list_connections`、`connect`、`list_databases`、`list_schemas`、`list_tables`、`describe_table`、`get_table_ddl`、`browse_table`、`count_rows`、`execute_query`、`export_data`

`execute_query` は各接続の External Clients 設定およびセーフモードに従い、書き込みクエリを実行する前には TableMinus 画面上でユーザーの確認を要求します。

### テーマ、背景マテリアル、多言語対応

組み込みテーマは 14 種類（`Dark`、`Light`、`High Contrast`、`Gaussian Glass Dark`、`Gaussian Glass Light`、`Tokyo Night`、`Dracula`、`Catppuccin Mocha`、`One Dark Pro`、`Nord`、`Monokai Pro`、`Gruvbox Dark`、`GitHub Light`、`Catppuccin Latte`）あります。UI テーマとコード用シンタックスパレットは個別に組み合わせることができ、VS Code の `.json` カラーテーマのインポートにも対応しています。

Windows 11 ではメインウィンドウに DWM の `Gaussian Blur`、`Mica`、`Mica Alt`、`Opaque` を適用でき、モーダルやドロップダウンなどのオーバーレイは視認性を保つため不透明（`1.0` alpha）で描画します。視覚効果の軽減は Windows の `SPI_GETCLIENTAREAANIMATION` に連動し、OS 側でアニメーションが無効な場合はトランジションを行いません。UI ラベル、メニュー、モーダル、ツールチップ、ドライバ診断メッセージは 6 言語（`en-US`、`vi-VN`、`zh-CN`、`ja-JP`、`es-ES`、`fr-FR`）に対応しています。

### CLI の使い方

`tableminus.exe` は GUI ランチャーとスクリプト向け CLI（`tablepro-cli`）を兼ねています。`.sqlite`、`.db`、`.duckdb`、`.sql` のファイルパスや接続 URL（`postgresql://...`）を渡すと Named Pipe IPC 経由で起動中のウィンドウに開きます（別ウィンドウで開く場合は `-n` / `--new-window` を指定）。

主なトップレベルフラグ：
- `--version`（`-V`） バージョンを表示して終了します。
- `--help`（`-h`） CLI の使い方とオプションを表示します。
- `--smoke-startup` ヘッドレスで起動時間を計測し、9 ドライバすべての登録を確認します。

サブコマンド：`open`、`query`、`connections`、`databases`、`schemas`、`tables`、`describe`、`export`、`status`、`completion`（`bash` / `zsh` / `fish`）。`--format`、`--read-only`、`--limit`、`--timeout` などのオプションが使えます。

```powershell
# 読み取り専用クエリを実行して JSON Lines で出力
.\target\release\tableminus.exe query -c Staging --read-only --format jsonl "SELECT id, email FROM users LIMIT 10"

# テーブルの CREATE TABLE DDL を表示
.\target\release\tableminus.exe describe -c Staging --ddl users
```

---

## ダウンロードとインストール

対応 OS は **Windows 10 / Windows 11（x64）** です。ビルド済みバイナリは [GitHub Releases](https://github.com/williamcachamwri/TableMinus/releases) で配布しています。

- **インストーラー（`TableMinus-win-Setup.exe`）**: UAC の管理者昇格なしで `%LocalAppData%` にインストールし、スタートメニューとデスクトップにショートカットを作成します。Velopack によるバックグラウンドの差分アップデートに対応しており、設定は **Settings -> About** から変更できます。
- **ポータブル版（`TableMinus-win-Portable.zip`）**: 任意のフォルダに展開して `tableminus.exe` を直接実行でき、インストーラー側の状態を変更しません。

各リリースには `SHA256SUMS.txt` が含まれており、PowerShell からハッシュを確認できます。

```powershell
Get-FileHash .\TableMinus-win-Setup.exe -Algorithm SHA256
```

---

## キーボードショートカット

| ショートカット | 操作 |
| :--- | :--- |
| `Ctrl+K` / `Ctrl+Shift+K` / `Ctrl+P` | コマンドパレットを開く |
| `Ctrl+Enter` / `F5` | アクティブな SQL クエリを実行 |
| `Ctrl+Shift+Enter` | 選択した SQL テキストを実行 |
| `Shift+Esc` | 実行中のクエリをキャンセル |
| `Ctrl+S` | クエリをライブラリに保存 |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | 新規タブ / タブを閉じる / 閉じたタブを開き直す |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | 次のタブ / 前のタブ |
| `Ctrl+F` | サイドバーのオブジェクトフィルタにフォーカス |
| `Ctrl+B` | オブジェクトサイドバーの表示切替 |
| `Ctrl+I` | Row Inspector の表示切替 |
| `Ctrl+Shift+A` | AI アシスタントドロワーの表示切替 |
| `Ctrl+Shift+O` | 接続ライブラリを開く |
| `Ctrl+,` | 設定を開く |
| `Ctrl+Shift+M` | テーマを切り替え |
| `Ctrl+Shift+S` / `Ctrl+Shift+R` | グリッド編集を Commit / Rollback |
| `Alt+Insert` / `Ctrl+Shift+Backspace` | 行を追加 / 選択した行を削除 |
| `Ctrl+Shift+E` | クエリ結果またはテーブルをエクスポート |
| `Esc` | アクティブなモーダルまたはオーバーレイを閉じる |

SQL エディタのツールバーから Vim ナビゲーション（`NORMAL`、`INSERT`、`VISUAL`、`:` コマンド）を有効にできます。

---

## ソースからのビルド

### 動作要件

- Windows 10 または Windows 11（x64）
- [rustup](https://rustup.rs/) 経由の安定版 Rust MSVC ツールチェーン（`x86_64-pc-windows-msvc`、Rust `2024` Edition）
- Visual Studio C++ Build Tools と Windows SDK（Win32、Direct3D 11、同梱 C ドライバ用）
- Windows PowerShell（コマンド連結には `&&` ではなく `;` を使用）

> [!CAUTION]
> ビルドとテストは常に `--release` で実行してください。シンボルデバッグが必須でない限り、ワークスペース全体でデバッグ版の `cargo build` や `cargo test` を実行しないでください。全クレートと 9 ドライバの `target\debug` 内 PDB ファイルは 140 GB を超えることがあります。メモリに制約のある環境では `-j 1` を付けてください。

### ビルド・テストコマンド

```powershell
# target\release\tableminus.exe をロックしているプロセスがないか確認
Get-Process tableminus -ErrorAction SilentlyContinue

# release モードでの型チェックとビルド
cargo check --release -j 1 -p tableminus-windows-app
cargo build --release -j 1 -p tableminus-windows-app

# ヘッドレス起動チェックの実行とアプリ起動
.\target\release\tableminus.exe --smoke-startup
.\target\release\tableminus.exe
```

リリース用テストスイート：

```powershell
cargo test --release -j 1 -p tablepro-core filter
cargo test --release -j 1 -p tableminus-e2e --test startup_perf_e2e -- --nocapture
cargo test --release -j 1 -p tableminus-e2e --test sqlite_e2e -- --nocapture
```

### ワークスペース構成

| パス | 役割 |
| :--- | :--- |
| `core/crates/core` | `tablepro-core`: クエリモデル、フィルタ、SQL 方言、エクスポートエンコーダ |
| `core/crates/session` | `tablepro-session`: セッション管理、接続プール、トランザクション |
| `core/crates/cli` | `tablepro-cli`: CLI サブコマンドとヘッドレスランナー |
| `core/crates/mcp` | `tablepro-mcp`: トークン認証付きローカルホスト MCP HTTP サーバー |
| `core/crates/ai` | `tablepro-ai`: スキーマのみを扱う Ollama / OpenAI / Anthropic クライアント |
| `core/crates/ssh` | `tablepro-ssh`: `russh` によるプロセス内 SSH トンネリング |
| `core/crates/net` | `tablepro-net`: `rustls` TLS とシステム証明書検証 |
| `core/crates/storage` | `tablepro-storage`: 保存済み接続、クエリライブラリ、履歴 |
| `core/crates/foreign-import` | `tablepro-foreign-import`: 他の DB クライアントからの接続インポート |
| `core/crates/account` | `tablepro-account`: アカウントとトークンのトレイト |
| `core/crates/drivers/*` | 9 データベースドライバ（`postgres`, `mysql`, `sqlite`, `duckdb`, `mssql`, `clickhouse`, `cassandra`, `mongodb`, `redis`） |
| `crates/tableminus-gfx` | ベクターグリフとデータベースエンジンアイコン |
| `crates/tableminus-state` | エディタ状態、補完、グリッドモデル、タブ、14 組み込みテーマ |
| `crates/tableminus-services` | 非同期 Tokio ランタイム、セッション/AI/MCP 制御、多言語診断 |
| `crates/tableminus-windows-platform` | 設定 JSON、Win32 資格情報マネージャー、DWM・視覚効果軽減ヘルパー |
| `crates/tableminus-windows-app` | HeroGPUI（Direct3D 11）デスクトップアプリ、Velopack アップデータ、6 言語 |
| `crates/tableminus-e2e` | SQLite、DuckDB、起動時間、グリッド描画のベンチマーク |

---

## コントリビュートとライセンス

バグ報告、機能改善、翻訳修正などのコントリビュートを歓迎します。ビルドや GPUI の制約については [CONTRIBUTING.md](CONTRIBUTING.md)、リリース履歴は [CHANGELOG.md](CHANGELOG.md)、行動規範は [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)、脆弱性の非公開報告手順は [SECURITY.md](SECURITY.md) をご覧ください。

TableMinus は [LICENSE](LICENSE)（GNU Affero General Public License v3.0）および [LICENSE-MIT](LICENSE-MIT)（MIT、上流コアである [TableProApp/TablePro](https://github.com/TableProApp/TablePro) への帰属表示を含む）のもとでライセンスされています。
