<div align="center">

<img src="crates/tableminus-windows-app/assets/tableminus_logo.png" alt="TableMinus Logo" width="112" height="112" />

# TableMinus

**TrÃ¬nh quáº£n lÃ½ cÆ¡ sá»Ÿ dá»¯ liá»‡u vÃ  SQL workbench thuáº§n Rust cho Windows.**

[![CI](https://img.shields.io/github/actions/workflow/status/williamcachamwri/TableMinus/ci.yml?branch=main&style=flat-square&label=CI&logo=github)](https://github.com/williamcachamwri/TableMinus/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/actions/workflow/status/williamcachamwri/TableMinus/release.yml?style=flat-square&label=Release&logo=github)](https://github.com/williamcachamwri/TableMinus/actions/workflows/release.yml)
[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![GPUI](https://img.shields.io/badge/UI-HeroGPUI%20%2F%20Direct3D%2011-0078D4?style=flat-square&logo=windows&logoColor=white)](#tá»•ng-quan)
[![Drivers](https://img.shields.io/badge/Drivers-9%20Engines-10B981?style=flat-square)](#há»‡-quáº£n-trá»‹-csdl-Ä‘Æ°á»£c-há»—-trá»£)
[![Memory](https://img.shields.io/badge/RAM-~55%20MB%20khi%20táº£i-F59E0B?style=flat-square)](#tá»•ng-quan)
[![Startup](https://img.shields.io/badge/Khá»Ÿi%20Ä‘á»™ng-~10%20ms-6366F1?style=flat-square)](#tá»•ng-quan)
[![Windows](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-0078D4?style=flat-square&logo=windows11&logoColor=white)](#táº£i-vá»-vÃ -cÃ i-Ä‘áº·t)
[![License: MIT / AGPL-3.0](https://img.shields.io/badge/License-MIT%20%2F%20AGPL--3.0-blue?style=flat-square)](LICENSE-MIT)

[English](README.md) Â· [Tiáº¿ng Viá»‡t](README.vi.md) Â· [ç®€ä½“ä¸­æ–‡](README.zh-CN.md) Â· [æ—¥æœ¬èªž](README.ja.md) Â· [EspaÃ±ol](README.es.md) Â· [FranÃ§ais](README.fr.md)

</div>

---

## Tá»•ng quan

TableMinus lÃ  á»©ng dá»¥ng SQL vÃ  quáº£n lÃ½ cÆ¡ sá»Ÿ dá»¯ liá»‡u viáº¿t báº±ng Rust dÃ nh riÃªng cho Windows. Giao diá»‡n dá»±ng báº±ng [HeroGPUI](https://github.com/williamcachamwri/TableMinus) trÃªn ná»n Direct3D 11 vÃ  sá»­ dá»¥ng lÃµi Rust mÃ£ nguá»“n má»Ÿ cá»§a [TablePro](https://github.com/TableProApp/TablePro), khÃ´ng cháº¡y qua Electron hay JVM.

TrÃªn báº£n dá»±ng release á»Ÿ Windows 11, thá»i gian khá»Ÿi Ä‘á»™ng láº¡nh Ä‘o Ä‘Æ°á»£c quanh má»©c `10 ms` (tá»« `9.21` Ä‘áº¿n `10.11 ms` qua `--smoke-startup` khi náº¡p Ä‘á»§ 9 driver), cÃ²n bá»™ nhá»› tiáº¿n trÃ¬nh giá»¯ á»Ÿ khoáº£ng `~55 MB` trong cÃ¡c bÃ i kiá»ƒm tra táº£i `100k`, `500k` vÃ  `1M` dÃ²ng. Trong bá»™ test SQLite cá»§a `tableminus-e2e`, viá»‡c náº¡p `100k` dÃ²ng vÃ o báº£ng lÆ°á»›i máº¥t `24.43 ms` vÃ  cuá»™n `10k` khung hÃ¬nh máº¥t `0.432 ms`.

---

## áº¢nh chá»¥p mÃ n hÃ¬nh

### TrÃ¬nh soáº¡n tháº£o SQL, biá»ƒu Ä‘á»“ trá»±c tiáº¿p vÃ  káº¿t quáº£ truy váº¥n

![TableMinus SQL Editor and Live Charts](docs/screenshots/sql-editor-charts.png)

### LÆ°á»›i dá»¯ liá»‡u áº£o hÃ³a, tá»•ng há»£p cá»™t vÃ  bá»™ lá»c tham sá»‘ hÃ³a

![TableMinus Data Grid view](docs/screenshots/data-grid.png)

### SÆ¡ Ä‘á»“ ER tÆ°Æ¡ng tÃ¡c, quan há»‡ khÃ³a ngoáº¡i vÃ  xuáº¥t SVG

![TableMinus ER Diagram Workbench](docs/screenshots/er-diagram.png)

### TrÃ¬nh hÆ°á»›ng dáº«n nháº­p dá»¯ liá»‡u (CSV, JSON, XLSX) tá»± Ä‘á»™ng Ã¡nh xáº¡ kiá»ƒu

![TableMinus Import Wizard](docs/screenshots/import-wizard.png)

### So sÃ¡nh Schema & Dá»¯ liá»‡u kÃ¨m tÃ³m táº¯t khÃ¡c biá»‡t vÃ  táº¡o script Ä‘á»“ng bá»™

![TableMinus Schema and Data Compare](docs/screenshots/data-compare.png)

### TrÃ¬nh táº¡o dá»¯ liá»‡u giáº£ láº­p vá»›i seed cá»‘ Ä‘á»‹nh vÃ  thá»© tá»± khÃ³a ngoáº¡i

![TableMinus Mock Data Generator](docs/screenshots/mock-generator.png)

### Truyá»n dá»¯ liá»‡u liÃªn cÆ¡ sá»Ÿ dá»¯ liá»‡u kÃ¨m xem trÆ°á»›c DDL trá»±c tiáº¿p

![TableMinus Cross-Engine Data Transfer](docs/screenshots/data-transfer.png)

### TÃ¬m kiáº¿m siÃªu dá»¯ liá»‡u (`Ctrl+P` / Metadata Search)

![TableMinus Metadata Search](docs/screenshots/metadata-search.png)

### NgÄƒn trá»£ lÃ½ AI chá»‰ Ä‘á»c schema (`Ctrl+Shift+A`)

![TableMinus Schema-Aware AI Assistant](docs/screenshots/ai-assistant.png)

---

## Há»‡ quáº£n trá»‹ CSDL Ä‘Æ°á»£c há»— trá»£

TableMinus biÃªn dá»‹ch sáºµn 9 driver vÃ o tháº³ng tá»‡p `tableminus.exe`, khÃ´ng cáº§n cÃ i thÃªm ODBC hay plugin ngoÃ i:

| Há»‡ quáº£n trá»‹ | Crate driver | Ghi chÃº |
| :--- | :--- | :--- |
| PostgreSQL | `tablepro-driver-postgres` | `tokio-postgres` + `deadpool`, há»— trá»£ `COPY IN`, xem schema vÃ  DDL |
| MySQL / MariaDB | `tablepro-driver-mysql` | Há»— trá»£ Ä‘áº§y Ä‘á»§ dialect SQL vÃ  metadata danh má»¥c |
| SQLite | `tablepro-driver-sqlite` | Má»Ÿ trá»±c tiáº¿p tá»‡p `.sqlite` / `.db` qua giao diá»‡n, kÃ©o tháº£ hoáº·c CLI |
| DuckDB | `tablepro-driver-duckdb` | ÄÃ³ng gÃ³i sáºµn DuckDB 2.0 kÃ¨m `json` vÃ  `parquet` |
| Microsoft SQL Server | `tablepro-driver-mssql` | TDS 8.0 thuáº§n Rust (`tiberius`) há»— trá»£ TLS vÃ  nhiá»u táº­p káº¿t quáº£ |
| ClickHouse | `tablepro-driver-clickhouse` | Truy váº¥n qua HTTP/TLS báº±ng `hyper-rustls` |
| Cassandra / ScyllaDB | `tablepro-driver-cassandra` | Driver CQL thuáº§n (`scylla`) cháº¡y trÃªn `rustls` |
| MongoDB | `tablepro-driver-mongodb` | Truy váº¥n document, Ã¡nh xáº¡ BSON 3 vÃ  phÃ¢n giáº£i DNS seed list |
| Redis | `tablepro-driver-redis` | Xem key-value vÃ  cháº¡y lá»‡nh qua TLS/TCP báº¥t Ä‘á»“ng bá»™ |

Káº¿t ná»‘i há»— trá»£ SSH tunnel tÃ­ch há»£p sáºµn qua `russh` (`tablepro-ssh` vá»›i máº­t kháº©u, khÃ³a riÃªng, há»™p thoáº¡i nháº­p passphrase vÃ  xÃ¡c minh host key), TLS qua `rustls` vÃ  kho chá»©ng chá»‰ Windows (`Disable`, `Prefer`, `Require`, `VerifyCA`, `VerifyFull`, há»— trá»£ CA riÃªng vÃ  client cert/key), cháº¿ Ä‘á»™ Safe Mode (`Off`, `ConfirmWrites`, `ReadOnly`), cÃ¹ng kháº£ nÄƒng nháº­p káº¿t ná»‘i Ä‘Ã£ lÆ°u tá»« cÃ¡c á»©ng dá»¥ng khÃ¡c qua `tablepro-foreign-import`.

---

## TÃ­nh nÄƒng

### TrÃ¬nh soáº¡n tháº£o SQL vÃ  LÆ°á»›i dá»¯ liá»‡u

TrÃ¬nh soáº¡n tháº£o há»— trá»£ tÃ´ mÃ u cÃº phÃ¡p theo tá»«ng dialect (`PostgreSQL`, `MySQL`, `SQLite`, `MsSql` vÃ  cÃ¡c há»‡ khÃ¡c), tá»± Ä‘á»™ng gá»£i Ã½ báº£ng, cá»™t, schema, tá»« khÃ³a vÃ  má»‡nh Ä‘á» SQL (`FROM`, `JOIN`, `INTO`, `UPDATE`), chá»‰nh sá»­a Ä‘a con trá», thÆ° viá»‡n truy váº¥n Ä‘Ã£ lÆ°u, lá»‹ch sá»­ truy váº¥n vÃ  tÃ¹y chá»n cháº¿ Ä‘á»™ Vim (`NORMAL`, `INSERT`, `VISUAL`, `COMMAND`).

Trong lÆ°á»›i dá»¯ liá»‡u, báº¡n cÃ³ thá»ƒ xem chi tiáº¿t báº£n ghi qua Row Inspector (`Ctrl+I`), chuyá»ƒn giá»¯a cÃ¡c tab Structure, Columns, Indexes, Foreign Keys vÃ  DDL, ghim cÃ¡c cá»™t Ä‘áº§u tá»‘i Ä‘a 2/3 chiá»u rá»™ng khung nhÃ¬n, áº©n/hiá»‡n hoáº·c kÃ©o giÃ£n cá»™t, vÃ  ghÃ©p nhiá»u Ä‘iá»u kiá»‡n lá»c tham sá»‘ hÃ³a vá»›i 11 toÃ¡n tá»­ (`=`, `!=`, `Contains`, `StartsWith`, `EndsWith`, `<`, `<=`, `>`, `>=`, `IS NULL`, `IS NOT NULL`). CÃ¡c thao tÃ¡c sá»­a Ã´, thÃªm dÃ²ng (`Alt+Insert`) hay Ä‘Ã¡nh dáº¥u xÃ³a dÃ²ng (`Ctrl+Shift+Backspace`) Ä‘Æ°á»£c gom theo lÃ´ Ä‘á»ƒ kiá»ƒm tra trÆ°á»›c khi `Commit` (`Ctrl+Shift+S`) hoáº·c `Rollback` (`Ctrl+Shift+R`).

Káº¿t quáº£ truy váº¥n vÃ  báº£ng Ä‘Æ°á»£c xuáº¥t qua tá»‡p táº¡m `.part` Ä‘á»ƒ Ä‘áº£m báº£o ghi nguyÃªn tá»­ sang CSV (`.csv`), JSON Array (`.json`), JSON Lines (`.jsonl`) hoáº·c cÃ¢u lá»‡nh SQL `INSERT` (`.sql`) báº±ng tá»• há»£p `Ctrl+Shift+E`. NgoÃ i ra `tablepro-core` cÃ²n cÃ³ bá»™ mÃ£ hÃ³a TSV, XML vÃ  XLSX tiáº¿t kiá»‡m bá»™ nhá»›.

### Báº£o máº­t vÃ  LÆ°u trá»¯ cá»¥c bá»™

Máº­t kháº©u cÆ¡ sá»Ÿ dá»¯ liá»‡u, khÃ³a SSH, API key AI vÃ  Bearer token cá»§a MCP Ä‘Æ°á»£c lÆ°u trong Windows Credential Manager (`CredWriteW` / `CredReadW`) vÃ  xÃ³a khá»i RAM báº±ng `zeroize` thay vÃ¬ ghi dáº¡ng vÄƒn báº£n rÃµ vÃ o tá»‡p cáº¥u hÃ¬nh hay nháº­t kÃ½. á»ž cháº¿ Ä‘á»™ Safe Mode, káº¿t ná»‘i cÃ³ thá»ƒ Ä‘áº·t á»Ÿ má»©c `ConfirmWrites` hoáº·c `ReadOnly` Ä‘á»ƒ cÃ¡c lá»‡nh `UPDATE`, `DELETE`, `DROP` pháº£i Ä‘Æ°á»£c xÃ¡c nháº­n hoáº·c bá»‹ cháº·n trÆ°á»›c khi cháº¡y.

### Trá»£ lÃ½ AI vÃ  MÃ¡y chá»§ MCP

NgÄƒn AI (`Ctrl+Shift+A`) káº¿t ná»‘i vá»›i Ollama ná»™i bá»™ (`http://127.0.0.1:11434/v1`), cÃ¡c endpoint tÆ°Æ¡ng thÃ­ch OpenAI, OpenAI hoáº·c Anthropic. Chá»‰ thÃ´ng tin schema (tÃªn báº£ng, tÃªn cá»™t, kiá»ƒu dá»¯ liá»‡u vÃ  dialect SQL) Ä‘Æ°á»£c gá»­i tá»›i mÃ´ hÃ¬nh; dá»¯ liá»‡u tá»«ng dÃ²ng trong báº£ng luÃ´n náº±m láº¡i trÃªn mÃ¡y.

Vá»›i cÃ¡c cÃ´ng cá»¥ láº­p trÃ¬nh hoáº·c IDE bÃªn ngoÃ i, `tablepro-mcp` cháº¡y má»™t mÃ¡y chá»§ MCP HTTP ná»™i bá»™ trÃªn `127.0.0.1`, xÃ¡c thá»±c báº±ng Bearer token trong Windows Credential Manager vÃ  kiá»ƒm tra tiáº¿n trÃ¬nh gá»i. MÃ¡y chá»§ cung cáº¥p 11 cÃ´ng cá»¥:

- `list_connections`, `connect`, `list_databases`, `list_schemas`, `list_tables`, `describe_table`, `get_table_ddl`, `browse_table`, `count_rows`, `execute_query`, `export_data`

CÃ´ng cá»¥ `execute_query` tuÃ¢n thá»§ cáº¥u hÃ¬nh External Clients vÃ  Safe Mode cá»§a tá»«ng káº¿t ná»‘i, Ä‘á»“ng thá»i yÃªu cáº§u ngÆ°á»i dÃ¹ng báº¥m xÃ¡c nháº­n ngay trong TableMinus trÆ°á»›c khi cháº¡y báº¥t ká»³ lá»‡nh ghi nÃ o.

### Giao diá»‡n, Váº­t liá»‡u ná»n vÃ  NgÃ´n ngá»¯

TableMinus cÃ³ sáºµn 14 bá»™ giao diá»‡n (`Dark`, `Light`, `High Contrast`, `Gaussian Glass Dark`, `Gaussian Glass Light`, `Tokyo Night`, `Dracula`, `Catppuccin Mocha`, `One Dark Pro`, `Nord`, `Monokai Pro`, `Gruvbox Dark`, `GitHub Light`, `Catppuccin Latte`). Báº¡n cÃ³ thá»ƒ chá»n tÃ¡ch biá»‡t giao diá»‡n khung á»©ng dá»¥ng vÃ  báº£ng mÃ u mÃ£ nguá»“n, hoáº·c nháº­p tá»‡p theme `.json` tá»« VS Code.

TrÃªn Windows 11, cá»­a sá»• chÃ­nh há»— trá»£ cÃ¡c váº­t liá»‡u ná»n DWM gá»“m `Gaussian Blur`, `Mica`, `Mica Alt` vÃ  `Opaque`, trong khi cÃ¡c lá»›p ná»•i (modal, dropdown, toast) luÃ´n giá»¯ ná»n Ä‘á»¥c (`1.0` alpha) Ä‘á»ƒ chá»¯ khÃ´ng bá»‹ nhÃ²e. TÃ¹y chá»n Reduced Motion tá»± Ä‘á»“ng bá»™ theo `SPI_GETCLIENTAREAANIMATION` cá»§a Windows Ä‘á»ƒ bá» qua hiá»‡u á»©ng chuyá»ƒn Ä‘á»™ng khi há»‡ thá»‘ng táº¯t hoáº¡t áº£nh. ToÃ n bá»™ nhÃ£n giao diá»‡n, menu, há»™p thoáº¡i vÃ  thÃ´ng bÃ¡o lá»—i tá»« driver há»— trá»£ 6 ngÃ´n ngá»¯: `en-US`, `vi-VN`, `zh-CN`, `ja-JP`, `es-ES` vÃ  `fr-FR`.

### Sá»­ dá»¥ng qua CLI

`tableminus.exe` vá»«a khá»Ÿi cháº¡y giao diá»‡n Ä‘á»“ há»a vá»«a Ä‘Ã³ng vai trÃ² cÃ´ng cá»¥ dÃ²ng lá»‡nh (`tablepro-cli`). Khi truyá»n Ä‘Æ°á»ng dáº«n `.sqlite`, `.db`, `.duckdb`, `.sql` hoáº·c chuá»—i káº¿t ná»‘i (`postgresql://...`), tá»‡p sáº½ má»Ÿ ngay trong cá»­a sá»• Ä‘ang cháº¡y qua Named Pipe IPC (thÃªm `-n` / `--new-window` náº¿u muá»‘n má»Ÿ cá»­a sá»• má»›i).

CÃ¡c cá» cáº¥p á»©ng dá»¥ng:
- `--version` (`-V`) in phiÃªn báº£n rá»“i thoÃ¡t.
- `--help` (`-h`) in hÆ°á»›ng dáº«n sá»­ dá»¥ng CLI.
- `--smoke-startup` cháº¡y kiá»ƒm tra thá»i gian khá»Ÿi Ä‘á»™ng khÃ´ng giao diá»‡n vÃ  xÃ¡c nháº­n Ä‘á»§ 9 driver.

CÃ¡c lá»‡nh con: `open`, `query`, `connections`, `databases`, `schemas`, `tables`, `describe`, `export`, `status` vÃ  `completion` (`bash`, `zsh`, `fish`), Ä‘i kÃ¨m cÃ¡c tÃ¹y chá»n `--format`, `--read-only`, `--limit` vÃ  `--timeout`.

```powershell
# Cháº¡y truy váº¥n chá»‰ Ä‘á»c vÃ  xuáº¥t káº¿t quáº£ dáº¡ng JSON Lines
.\target\release\tableminus.exe query -c Staging --read-only --format jsonl "SELECT id, email FROM users LIMIT 10"

# Xem cÃ¢u lá»‡nh CREATE TABLE DDL cá»§a má»™t báº£ng
.\target\release\tableminus.exe describe -c Staging --ddl users
```

---

## Táº£i vá» vÃ  CÃ i Ä‘áº·t

YÃªu cáº§u **Windows 10 hoáº·c Windows 11 (x64)**. CÃ¡c báº£n dá»±ng sáºµn Ä‘Æ°á»£c phÃ¡t hÃ nh táº¡i [GitHub Releases](https://github.com/williamcachamwri/TableMinus/releases):

- **Bá»™ cÃ i Ä‘áº·t (`TableMinus-win-Setup.exe`)**: CÃ i vÃ o `%LocalAppData%` kÃ¨m lá»‘i táº¯t Start Menu vÃ  mÃ n hÃ¬nh ná»n mÃ  khÃ´ng cáº§n quyá»n quáº£n trá»‹ UAC. Sá»­ dá»¥ng Velopack Ä‘á»ƒ cáº­p nháº­t pháº§n sai khÃ¡c (delta) cháº¡y ná»n, tÃ¹y chá»‰nh trong **Settings -> About**.
- **Báº£n Portable (`TableMinus-win-Portable.zip`)**: Giáº£i nÃ©n ra thÆ° má»¥c báº¥t ká»³ vÃ  cháº¡y tháº³ng `tableminus.exe` mÃ  khÃ´ng áº£nh hÆ°á»Ÿng tráº¡ng thÃ¡i bá»™ cÃ i.

Má»—i báº£n phÃ¡t hÃ nh Ä‘á»u kÃ¨m tá»‡p `SHA256SUMS.txt` Ä‘á»ƒ Ä‘á»‘i chiáº¿u báº±ng PowerShell:

```powershell
Get-FileHash .\TableMinus-win-Setup.exe -Algorithm SHA256
```

---

## PhÃ­m táº¯t

| PhÃ­m táº¯t | Thao tÃ¡c |
| :--- | :--- |
| `Ctrl+K` / `Ctrl+Shift+K` / `Ctrl+P` | Má»Ÿ Báº£ng lá»‡nh (Command Palette) |
| `Ctrl+Enter` / `F5` | Cháº¡y truy váº¥n SQL hiá»‡n táº¡i |
| `Ctrl+Shift+Enter` | Cháº¡y Ä‘oáº¡n SQL Ä‘ang bÃ´i Ä‘en |
| `Shift+Esc` | Há»§y truy váº¥n Ä‘ang cháº¡y |
| `Ctrl+S` | LÆ°u truy váº¥n vÃ o thÆ° viá»‡n |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | Tab truy váº¥n má»›i / ÄÃ³ng tab / Má»Ÿ láº¡i tab vá»«a Ä‘Ã³ng |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Chuyá»ƒn sang tab káº¿ tiáº¿p / trÆ°á»›c Ä‘Ã³ |
| `Ctrl+F` | Trá» vÃ o Ã´ lá»c thanh Ä‘á»‘i tÆ°á»£ng |
| `Ctrl+B` | áº¨n/hiá»‡n thanh bÃªn Ä‘á»‘i tÆ°á»£ng |
| `Ctrl+I` | áº¨n/hiá»‡n Row Inspector |
| `Ctrl+Shift+A` | áº¨n/hiá»‡n ngÄƒn Trá»£ lÃ½ AI |
| `Ctrl+Shift+O` | Má»Ÿ thÆ° viá»‡n Káº¿t ná»‘i |
| `Ctrl+,` | Má»Ÿ CÃ i Ä‘áº·t |
| `Ctrl+Shift+M` | Äá»•i giao diá»‡n |
| `Ctrl+Shift+S` / `Ctrl+Shift+R` | LÆ°u (`Commit`) / HoÃ n tÃ¡c (`Rollback`) thay Ä‘á»•i trÃªn lÆ°á»›i |
| `Alt+Insert` / `Ctrl+Shift+Backspace` | ThÃªm dÃ²ng / XÃ³a cÃ¡c dÃ²ng Ä‘ang chá»n trÃªn lÆ°á»›i |
| `Ctrl+Shift+E` | Xuáº¥t káº¿t quáº£ hoáº·c báº£ng |
| `Esc` | ÄÃ³ng há»™p thoáº¡i hoáº·c lá»›p phá»§ hiá»‡n táº¡i |

Cháº¿ Ä‘á»™ Vim (`NORMAL`, `INSERT`, `VISUAL` vÃ  lá»‡nh `:`) cÃ³ thá»ƒ báº­t trá»±c tiáº¿p trÃªn thanh cÃ´ng cá»¥ cá»§a trÃ¬nh soáº¡n tháº£o SQL.

---

## Tá»± biÃªn dá»‹ch tá»« mÃ£ nguá»“n

### YÃªu cáº§u mÃ´i trÆ°á»ng

- Windows 10 hoáº·c Windows 11 (x64)
- Bá»™ cÃ´ng cá»¥ Rust MSVC á»•n Ä‘á»‹nh (`x86_64-pc-windows-msvc`, Rust `2024` Edition) qua [rustup](https://rustup.rs/)
- Visual Studio C++ Build Tools vÃ  Windows SDK (cho Win32, Direct3D 11 vÃ  cÃ¡c driver C Ä‘i kÃ¨m)
- Windows PowerShell (dÃ¹ng `;` thay cho `&&` khi ná»‘i nhiá»u lá»‡nh)

> [!CAUTION]
> LuÃ´n biÃªn dá»‹ch vÃ  cháº¡y test vá»›i `--release`. TrÃ¡nh cháº¡y `cargo build` hay `cargo test` á»Ÿ cháº¿ Ä‘á»™ debug thÃ´ng thÆ°á»ng náº¿u khÃ´ng thá»±c sá»± cáº§n gá»¡ lá»—i symbol, vÃ¬ cÃ¡c tá»‡p PDB trong `target\debug` cho toÃ n bá»™ workspace vÃ  9 driver cÃ³ thá»ƒ vÆ°á»£t quÃ¡ 140 GB. DÃ¹ng thÃªm `-j 1` trÃªn mÃ¡y Ã­t RAM.

### Lá»‡nh biÃªn dá»‹ch vÃ  kiá»ƒm tra

```powershell
# Kiá»ƒm tra xem cÃ³ tiáº¿n trÃ¬nh tableminus.exe nÃ o Ä‘ang khÃ³a tá»‡p Ä‘áº§u ra khÃ´ng
Get-Process tableminus -ErrorAction SilentlyContinue

# Kiá»ƒm tra kiá»ƒu vÃ  biÃªn dá»‹ch báº£n release
cargo check --release -j 1 -p tableminus-windows-app
cargo build --release -j 1 -p tableminus-windows-app

# Cháº¡y kiá»ƒm tra khá»Ÿi Ä‘á»™ng vÃ  má»Ÿ á»©ng dá»¥ng
.\target\release\tableminus.exe --smoke-startup
.\target\release\tableminus.exe
```

CÃ¡c bá»™ kiá»ƒm thá»­ release:

```powershell
cargo test --release -j 1 -p tablepro-core filter
cargo test --release -j 1 -p tableminus-e2e --test startup_perf_e2e -- --nocapture
cargo test --release -j 1 -p tableminus-e2e --test sqlite_e2e -- --nocapture
```

### Cáº¥u trÃºc workspace

| ÄÆ°á»ng dáº«n | Vai trÃ² |
| :--- | :--- |
| `core/crates/core` | `tablepro-core`: mÃ´ hÃ¬nh truy váº¥n, bá»™ lá»c, dialect SQL, bá»™ xuáº¥t tá»‡p |
| `core/crates/session` | `tablepro-session`: phiÃªn káº¿t ná»‘i, connection pool, transaction |
| `core/crates/cli` | `tablepro-cli`: lá»‡nh con CLI vÃ  bá»™ cháº¡y khÃ´ng giao diá»‡n |
| `core/crates/mcp` | `tablepro-mcp`: mÃ¡y chá»§ MCP HTTP ná»™i bá»™ xÃ¡c thá»±c báº±ng token |
| `core/crates/ai` | `tablepro-ai`: client Ollama, OpenAI vÃ  Anthropic chá»‰ gá»­i schema |
| `core/crates/ssh` | `tablepro-ssh`: SSH tunnel tÃ­ch há»£p qua `russh` |
| `core/crates/net` | `tablepro-net`: TLS `rustls` vÃ  xÃ¡c minh chá»©ng chá»‰ há»‡ thá»‘ng |
| `core/crates/storage` | `tablepro-storage`: káº¿t ná»‘i Ä‘Ã£ lÆ°u, thÆ° viá»‡n truy váº¥n, lá»‹ch sá»­ |
| `core/crates/foreign-import` | `tablepro-foreign-import`: nháº­p káº¿t ná»‘i tá»« cÃ¡c trÃ¬nh quáº£n lÃ½ DB khÃ¡c |
| `core/crates/account` | `tablepro-account`: trait tÃ i khoáº£n vÃ  token |
| `core/crates/drivers/*` | 9 driver CSDL (`postgres`, `mysql`, `sqlite`, `duckdb`, `mssql`, `clickhouse`, `cassandra`, `mongodb`, `redis`) |
| `crates/tableminus-gfx` | Biá»ƒu tÆ°á»£ng vector vÃ  icon há»‡ quáº£n trá»‹ CSDL |
| `crates/tableminus-state` | Tráº¡ng thÃ¡i editor, autocomplete, mÃ´ hÃ¬nh lÆ°á»›i, tab, 14 bá»™ giao diá»‡n |
| `crates/tableminus-services` | Runtime Tokio báº¥t Ä‘á»“ng bá»™, Ä‘iá»u phá»‘i session/AI/MCP, cháº©n Ä‘oÃ¡n lá»—i |
| `crates/tableminus-windows-platform` | Cáº¥u hÃ¬nh JSON, Windows Credential Manager, DWM vÃ  Reduced Motion |
| `crates/tableminus-windows-app` | á»¨ng dá»¥ng HeroGPUI (Direct3D 11), bá»™ cáº­p nháº­t Velopack, 6 ngÃ´n ngá»¯ |
| `crates/tableminus-e2e` | Bá»™ Ä‘o hiá»‡u nÄƒng SQLite, DuckDB, khá»Ÿi Ä‘á»™ng vÃ  lÆ°á»›i dá»¯ liá»‡u |

---

## ÄÃ³ng gÃ³p vÃ  Giáº¥y phÃ©p

Má»i Ä‘Ã³ng gÃ³p mÃ£ nguá»“n, bÃ¡o lá»—i hoáº·c sá»­a báº£n dá»‹ch Ä‘á»u Ä‘Æ°á»£c hoan nghÃªnh. Xem [CONTRIBUTING.md](CONTRIBUTING.md) Ä‘á»ƒ náº¯m quy táº¯c biÃªn dá»‹ch vÃ  GPUI, [CHANGELOG.md](CHANGELOG.md) cho lá»‹ch sá»­ phiÃªn báº£n, [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md), vÃ  [SECURITY.md](SECURITY.md) khi cáº§n bÃ¡o cÃ¡o lá»— há»•ng báº£o máº­t riÃªng tÆ°.

TableMinus Ä‘Æ°á»£c phÃ¡t hÃ nh theo [LICENSE](LICENSE) (GNU Affero General Public License v3.0) vÃ  [LICENSE-MIT](LICENSE-MIT) (MIT, kÃ¨m ghi nháº­n pháº§n lÃµi tá»« [TableProApp/TablePro](https://github.com/TableProApp/TablePro)).
