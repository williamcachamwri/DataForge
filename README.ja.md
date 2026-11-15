<div align="center">

<img src="crates/tableminus-windows-app/assets/tableminus_logo.png" alt="TableMinus Logo" width="112" height="112" />

# TableMinus

**Rust ã¨ Direct3D 11 ã§æ§‹ç¯‰ã•ã‚ŒãŸ Windows ãƒã‚¤ãƒ†ã‚£ãƒ– SQLãƒ»ãƒ‡ãƒ¼ã‚¿ãƒ™ãƒ¼ã‚¹ãƒ¯ãƒ¼ã‚¯ãƒ™ãƒ³ãƒã€‚**

[![CI](https://img.shields.io/github/actions/workflow/status/williamcachamwri/TableMinus/ci.yml?branch=main&style=flat-square&label=CI&logo=github)](https://github.com/williamcachamwri/TableMinus/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/actions/workflow/status/williamcachamwri/TableMinus/release.yml?style=flat-square&label=Release&logo=github)](https://github.com/williamcachamwri/TableMinus/actions/workflows/release.yml)
[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![GPUI](https://img.shields.io/badge/UI-HeroGPUI%20%2F%20Direct3D%2011-0078D4?style=flat-square&logo=windows&logoColor=white)](#æ¦‚è¦)
[![Drivers](https://img.shields.io/badge/Drivers-9%20Engines-10B981?style=flat-square)](#å¯¾å¿œãƒ‡ãƒ¼ã‚¿ãƒ™ãƒ¼ã‚¹)
[![Memory](https://img.shields.io/badge/RAM-~55%20MB-F59E0B?style=flat-square)](#æ¦‚è¦)
[![Startup](https://img.shields.io/badge/ã‚³ãƒ¼ãƒ«ãƒ‰ã‚¹ã‚¿ãƒ¼ãƒˆ-~10%20ms-6366F1?style=flat-square)](#æ¦‚è¦)
[![Windows](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-0078D4?style=flat-square&logo=windows11&logoColor=white)](#ãƒ€ã‚¦ãƒ³ãƒ­ãƒ¼ãƒ‰ã¨ã‚¤ãƒ³ã‚¹ãƒˆãƒ¼ãƒ«)
[![License: MIT / AGPL-3.0](https://img.shields.io/badge/License-MIT%20%2F%20AGPL--3.0-blue?style=flat-square)](LICENSE-MIT)

[English](README.md) Â· [Tiáº¿ng Viá»‡t](README.vi.md) Â· [ç®€ä½“ä¸­æ–‡](README.zh-CN.md) Â· [æ—¥æœ¬èªž](README.ja.md) Â· [EspaÃ±ol](README.es.md) Â· [FranÃ§ais](README.fr.md)

</div>

---

## æ¦‚è¦

TableMinus ã¯ Rust ã§æ›¸ã‹ã‚ŒãŸ Windows å‘ã‘ã®ãƒã‚¤ãƒ†ã‚£ãƒ– SQLãƒ»ãƒ‡ãƒ¼ã‚¿ãƒ™ãƒ¼ã‚¹ãƒ¯ãƒ¼ã‚¯ãƒ™ãƒ³ãƒã§ã™ã€‚UI æç”»ã«ã¯ Direct3D 11 ä¸Šã§å‹•ã [HeroGPUI](https://github.com/williamcachamwri/TableMinus) ã‚’æŽ¡ç”¨ã—ã€ãƒ‡ãƒ¼ã‚¿ãƒ™ãƒ¼ã‚¹å‡¦ç†ã«ã¯ã‚ªãƒ¼ãƒ—ãƒ³ã‚½ãƒ¼ã‚¹ã® [TablePro](https://github.com/TableProApp/TablePro) Rust ã‚³ã‚¢ã‚’åˆ©ç”¨ã—ã¦ã„ã‚‹ãŸã‚ã€Electron ã‚„ JVM ãƒ©ãƒ³ã‚¿ã‚¤ãƒ ã¯ä¸è¦ã§ã™ã€‚

Windows 11 ã®ãƒªãƒªãƒ¼ã‚¹ãƒ“ãƒ«ãƒ‰ã§ã¯ã€ã‚³ãƒ¼ãƒ«ãƒ‰ã‚¹ã‚¿ãƒ¼ãƒˆãŒç´„ `10 ms`ï¼ˆ9 ãƒ‰ãƒ©ã‚¤ãƒç™»éŒ²è¾¼ã¿ã® `--smoke-startup` å®Ÿæ¸¬å€¤ã§ `9.21` ã‹ã‚‰ `10.11 ms`ï¼‰ã€`100k` / `500k` / `1M` è¡Œã®è² è·ã‚¯ã‚¨ãƒªå®Ÿè¡Œä¸­ã§ã‚‚ãƒ—ãƒ­ã‚»ã‚¹ã®ãƒ¡ãƒ¢ãƒªä½¿ç”¨é‡ã¯ `~55 MB` å‰å¾Œã«åŽã¾ã‚Šã¾ã™ã€‚`tableminus-e2e` ã® SQLite ãƒ†ã‚¹ãƒˆã‚¹ã‚¤ãƒ¼ãƒˆã§ã¯ã€`100k` è¡Œã®ã‚°ãƒªãƒƒãƒ‰èª­ã¿è¾¼ã¿ãŒ `24.43 ms`ã€`10k` ãƒ•ãƒ¬ãƒ¼ãƒ ã®ã‚¹ã‚¯ãƒ­ãƒ¼ãƒ«ãŒ `0.432 ms` ã§å®Œäº†ã—ã¾ã™ã€‚

---

## ã‚¹ã‚¯ãƒªãƒ¼ãƒ³ã‚·ãƒ§ãƒƒãƒˆ

### SQL ã‚¨ãƒ‡ã‚£ã‚¿ã€ãƒ©ã‚¤ãƒ–ãƒãƒ£ãƒ¼ãƒˆã€ã‚¯ã‚¨ãƒªçµæžœ

![TableMinus SQL Editor and Live Charts](docs/screenshots/sql-editor-charts.png)

### ä»®æƒ³åŒ–ãƒ‡ãƒ¼ã‚¿ã‚°ãƒªãƒƒãƒ‰ã€åˆ—é›†è¨ˆã€ãƒ‘ãƒ©ãƒ¡ãƒ¼ã‚¿åŒ–ãƒ•ã‚£ãƒ«ã‚¿

![TableMinus Data Grid view](docs/screenshots/data-grid.png)

### ã‚¤ãƒ³ã‚¿ãƒ©ã‚¯ãƒ†ã‚£ãƒ– ER å›³ã€å¤–éƒ¨ã‚­ãƒ¼ãƒªãƒ¬ãƒ¼ã‚·ãƒ§ãƒ³ã€SVG ã‚¨ã‚¯ã‚¹ãƒãƒ¼ãƒˆ

![TableMinus ER Diagram Workbench](docs/screenshots/er-diagram.png)

### ã‚¤ãƒ³ãƒãƒ¼ãƒˆã‚¦ã‚£ã‚¶ãƒ¼ãƒ‰ï¼ˆCSV / JSON / XLSXï¼‰ã¨è‡ªå‹•åž‹ãƒžãƒƒãƒ”ãƒ³ã‚°

![TableMinus Import Wizard](docs/screenshots/import-wizard.png)

### ã‚¹ã‚­ãƒ¼ãƒžãƒ»ãƒ‡ãƒ¼ã‚¿æ¯”è¼ƒã¨åŒæœŸã‚¹ã‚¯ãƒªãƒ—ãƒˆç”Ÿæˆ

![TableMinus Schema and Data Compare](docs/screenshots/data-compare.png)

### ãƒ¢ãƒƒã‚¯ãƒ‡ãƒ¼ã‚¿ç”Ÿæˆï¼ˆæ±ºå®šçš„ã‚·ãƒ¼ãƒ‰ã¨å¤–éƒ¨ã‚­ãƒ¼é †åºå¯¾å¿œï¼‰

![TableMinus Mock Data Generator](docs/screenshots/mock-generator.png)

### ã‚¯ãƒ­ã‚¹ã‚¨ãƒ³ã‚¸ãƒ³ãƒ‡ãƒ¼ã‚¿è»¢é€ã¨ãƒ©ã‚¤ãƒ– DDL ãƒ—ãƒ¬ãƒ“ãƒ¥ãƒ¼

![TableMinus Cross-Engine Data Transfer](docs/screenshots/data-transfer.png)

### ãƒ¡ã‚¿ãƒ‡ãƒ¼ã‚¿æ¤œç´¢ãƒ‘ãƒ¬ãƒƒãƒˆï¼ˆMetadata Searchï¼‰

![TableMinus Metadata Search](docs/screenshots/metadata-search.png)

### ã‚¹ã‚­ãƒ¼ãƒžæƒ…å ±ã®ã¿ã‚’é€ã‚‹ AI ãƒ‰ãƒ­ãƒ¯ãƒ¼ï¼ˆ`Ctrl+Shift+A`ï¼‰

![TableMinus Schema-Aware AI Assistant](docs/screenshots/ai-assistant.png)

---

## å¯¾å¿œãƒ‡ãƒ¼ã‚¿ãƒ™ãƒ¼ã‚¹

TableMinus ã¯ 9 ç¨®é¡žã®ãƒ‡ãƒ¼ã‚¿ãƒ™ãƒ¼ã‚¹ãƒ‰ãƒ©ã‚¤ãƒã‚’ `tableminus.exe` ã«ç›´æŽ¥ã‚³ãƒ³ãƒ‘ã‚¤ãƒ«ã—ã¦å†…è”µã—ã¦ã„ã‚‹ãŸã‚ã€åˆ¥é€” ODBC ãƒ‰ãƒ©ã‚¤ãƒã‚„ãƒ—ãƒ©ã‚°ã‚¤ãƒ³ã‚’å…¥ã‚Œã‚‹å¿…è¦ã¯ã‚ã‚Šã¾ã›ã‚“ã€‚

| ãƒ‡ãƒ¼ã‚¿ãƒ™ãƒ¼ã‚¹ | ãƒ‰ãƒ©ã‚¤ãƒ Crate | å‚™è€ƒ |
| :--- | :--- | :--- |
| PostgreSQL | `tablepro-driver-postgres` | `tokio-postgres` + `deadpool`ã€`COPY IN`ã€ã‚¹ã‚­ãƒ¼ãƒžãƒ»DDL è¡¨ç¤º |
| MySQL / MariaDB | `tablepro-driver-mysql` | SQL æ–¹è¨€ã¨ã‚«ã‚¿ãƒ­ã‚°ãƒ¡ã‚¿ãƒ‡ãƒ¼ã‚¿å¯¾å¿œ |
| SQLite | `tablepro-driver-sqlite` | ãƒ•ã‚¡ã‚¤ãƒ«ã‚’é–‹ããƒ»ãƒ‰ãƒ©ãƒƒã‚°&ãƒ‰ãƒ­ãƒƒãƒ—ãƒ»CLI ã‹ã‚‰ `.sqlite` / `.db` ã‚’ç›´æŽ¥é–‹ã |
| DuckDB | `tablepro-driver-duckdb` | `json` ã¨ `parquet` ã‚’çµ„ã¿è¾¼ã‚“ã ãƒãƒ³ãƒ‰ãƒ«ç‰ˆ DuckDB 2.0 |
| Microsoft SQL Server | `tablepro-driver-mssql` | ç´” Rust è£½ TDS 8.0ï¼ˆ`tiberius`ï¼‰ã€TLS ã¨è¤‡æ•°çµæžœã‚»ãƒƒãƒˆå¯¾å¿œ |
| ClickHouse | `tablepro-driver-clickhouse` | `hyper-rustls` ã«ã‚ˆã‚‹ HTTP/TLS ã‚¯ã‚¨ãƒª |
| Cassandra / ScyllaDB | `tablepro-driver-cassandra` | `rustls` ä¸Šã§å‹•ä½œã™ã‚‹ãƒã‚¤ãƒ†ã‚£ãƒ– CQL ãƒ‰ãƒ©ã‚¤ãƒï¼ˆ`scylla`ï¼‰ |
| MongoDB | `tablepro-driver-mongodb` | ãƒ‰ã‚­ãƒ¥ãƒ¡ãƒ³ãƒˆã‚¯ã‚¨ãƒªã€BSON 3 ãƒžãƒƒãƒ”ãƒ³ã‚°ã€DNS ã‚·ãƒ¼ãƒ‰ãƒªã‚¹ãƒˆè§£æ±º |
| Redis | `tablepro-driver-redis` | éžåŒæœŸ TLS/TCP ã§ã®ã‚­ãƒ¼ãƒ»ãƒãƒªãƒ¥ãƒ¼é–²è¦§ã¨ã‚³ãƒžãƒ³ãƒ‰å®Ÿè¡Œ |

æŽ¥ç¶šè¨­å®šã§ã¯ `russh` ã«ã‚ˆã‚‹ãƒ—ãƒ­ã‚»ã‚¹å†… SSH ãƒˆãƒ³ãƒãƒªãƒ³ã‚°ï¼ˆ`tablepro-ssh`ã€ãƒ‘ã‚¹ãƒ¯ãƒ¼ãƒ‰ãƒ»ç§˜å¯†éµãƒ»ãƒ‘ã‚¹ãƒ•ãƒ¬ãƒ¼ã‚ºå…¥åŠ›ãƒ»ãƒ›ã‚¹ãƒˆéµæ¤œè¨¼ã«å¯¾å¿œï¼‰ã€`rustls` ã¨ Windows è¨¼æ˜Žæ›¸ã‚¹ãƒˆã‚¢ã‚’ä½¿ã£ãŸ TLSï¼ˆ`Disable`ã€`Prefer`ã€`Require`ã€`VerifyCA`ã€`VerifyFull`ã€ã‚«ã‚¹ã‚¿ãƒ  CA ã¨ã‚¯ãƒ©ã‚¤ã‚¢ãƒ³ãƒˆè¨¼æ˜Žæ›¸/éµï¼‰ã€ã‚»ãƒ¼ãƒ•ãƒ¢ãƒ¼ãƒ‰ï¼ˆ`Off`ã€`ConfirmWrites`ã€`ReadOnly`ï¼‰ã€ãŠã‚ˆã³ `tablepro-foreign-import` ã«ã‚ˆã‚‹ä»–ãƒ„ãƒ¼ãƒ«ã‹ã‚‰ã®æŽ¥ç¶šæƒ…å ±ã‚¤ãƒ³ãƒãƒ¼ãƒˆã«å¯¾å¿œã—ã¦ã„ã¾ã™ã€‚

---

## ä¸»ãªæ©Ÿèƒ½

### SQL ã‚¨ãƒ‡ã‚£ã‚¿ã¨ãƒ‡ãƒ¼ã‚¿ã‚°ãƒªãƒƒãƒ‰

ã‚¨ãƒ‡ã‚£ã‚¿ã¯æ–¹è¨€ã”ã¨ã®ã‚·ãƒ³ã‚¿ãƒƒã‚¯ã‚¹ãƒã‚¤ãƒ©ã‚¤ãƒˆï¼ˆ`PostgreSQL`ã€`MySQL`ã€`SQLite`ã€`MsSql` ãªã©ï¼‰ã€ãƒ†ãƒ¼ãƒ–ãƒ«ãƒ»ã‚«ãƒ©ãƒ ãƒ»ã‚¹ã‚­ãƒ¼ãƒžãƒ»ã‚­ãƒ¼ãƒ¯ãƒ¼ãƒ‰ãƒ»SQL å¥ï¼ˆ`FROM`ã€`JOIN`ã€`INTO`ã€`UPDATE`ï¼‰ã®è£œå®Œã€ãƒžãƒ«ãƒã‚«ãƒ¼ã‚½ãƒ«ç·¨é›†ã€ä¿å­˜æ¸ˆã¿ã‚¯ã‚¨ãƒªãƒ©ã‚¤ãƒ–ãƒ©ãƒªã€ã‚¯ã‚¨ãƒªå±¥æ­´ã€ãã—ã¦ã‚ªãƒ—ã‚·ãƒ§ãƒ³ã® Vim ãƒ¢ãƒ¼ãƒ‰ï¼ˆ`NORMAL`ã€`INSERT`ã€`VISUAL`ã€`COMMAND`ï¼‰ã‚’å‚™ãˆã¦ã„ã¾ã™ã€‚

ãƒ‡ãƒ¼ã‚¿ã‚°ãƒªãƒƒãƒ‰ã§ã¯ãƒ‰ãƒƒã‚¯åž‹ã® Row Inspectorï¼ˆ`Ctrl+I`ï¼‰ã§ã®è¡Œç¢ºèªã€Structure / Columns / Indexes / Foreign Keys / DDL ã‚¿ãƒ–ã®åˆ‡ã‚Šæ›¿ãˆã€ãƒ“ãƒ¥ãƒ¼ãƒãƒ¼ãƒˆå¹…ã® 2/3 ã¾ã§ã®å…ˆé ­åˆ—å›ºå®šã€åˆ—ã®è¡¨ç¤ºåˆ‡æ›¿ãƒ»ãƒªã‚µã‚¤ã‚ºã€11 ç¨®é¡žã®æ¼”ç®—å­ï¼ˆ`=`ã€`!=`ã€`Contains`ã€`StartsWith`ã€`EndsWith`ã€`&lt;`ã€`&lt;=`ã€`&gt;`ã€`&gt;=`ã€`IS NULL`ã€`IS NOT NULL`ï¼‰ã‚’ä½¿ã£ãŸãƒ‘ãƒ©ãƒ¡ãƒ¼ã‚¿åŒ–ãƒ•ã‚£ãƒ«ã‚¿ã®é‡ã­æŽ›ã‘ãŒå¯èƒ½ã§ã™ã€‚ã‚»ãƒ«ç·¨é›†ã€è¡Œè¿½åŠ ï¼ˆ`Alt+Insert`ï¼‰ã€å‰Šé™¤ãƒžãƒ¼ã‚¯ï¼ˆ`Ctrl+Shift+Backspace`ï¼‰ã¯ãƒãƒƒãƒã¨ã—ã¦ä¿ç•™ã•ã‚Œã€`Commit`ï¼ˆ`Ctrl+Shift+S`ï¼‰ã¾ãŸã¯ `Rollback`ï¼ˆ`Ctrl+Shift+R`ï¼‰ã§ç¢ºå®šãƒ»å–ã‚Šæ¶ˆã—ã§ãã¾ã™ã€‚

ã‚¯ã‚¨ãƒªçµæžœã‚„ãƒ†ãƒ¼ãƒ–ãƒ«ã¯ `Ctrl+Shift+E` ã‹ã‚‰ `.part` ä¸€æ™‚ãƒ•ã‚¡ã‚¤ãƒ«çµŒç”±ã§ CSVï¼ˆ`.csv`ï¼‰ã€JSON Arrayï¼ˆ`.json`ï¼‰ã€JSON Linesï¼ˆ`.jsonl`ï¼‰ã€SQL `INSERT`ï¼ˆ`.sql`ï¼‰ã¸ã‚¢ãƒˆãƒŸãƒƒã‚¯ã«æ›¸ãå‡ºã›ã¾ã™ã€‚`tablepro-core` ã«ã¯ TSVã€XMLã€çœãƒ¡ãƒ¢ãƒªå‹•ä½œã® XLSX ã‚¨ãƒ³ã‚³ãƒ¼ãƒ€ã‚‚å«ã¾ã‚Œã¦ã„ã¾ã™ã€‚

### ã‚»ã‚­ãƒ¥ãƒªãƒ†ã‚£ã¨ãƒ­ãƒ¼ã‚«ãƒ«ä¿å­˜

ãƒ‡ãƒ¼ã‚¿ãƒ™ãƒ¼ã‚¹ã®ãƒ‘ã‚¹ãƒ¯ãƒ¼ãƒ‰ã€SSH ç§˜å¯†éµã®ãƒ‘ã‚¹ãƒ•ãƒ¬ãƒ¼ã‚ºã€AI API ã‚­ãƒ¼ã€MCP ãƒ™ã‚¢ãƒ©ãƒ¼ãƒˆãƒ¼ã‚¯ãƒ³ã¯å¹³æ–‡ã® JSON è¨­å®šãƒ•ã‚¡ã‚¤ãƒ«ã‚„ãƒ­ã‚°ã«ã¯æ›¸ã‹ãšã€Windows è³‡æ ¼æƒ…å ±ãƒžãƒãƒ¼ã‚¸ãƒ£ãƒ¼ï¼ˆ`CredWriteW` / `CredReadW`ï¼‰ã«ä¿å­˜ã—ã€ä½¿ç”¨å¾Œã®ãƒ¡ãƒ¢ãƒªã¯ `zeroize` ã§æ¶ˆåŽ»ã—ã¾ã™ã€‚ã‚»ãƒ¼ãƒ•ãƒ¢ãƒ¼ãƒ‰ã§æŽ¥ç¶šã‚’ `ConfirmWrites` ã¾ãŸã¯ `ReadOnly` ã«è¨­å®šã™ã‚Œã°ã€`UPDATE`ã€`DELETE`ã€`DROP` ã®å®Ÿè¡Œå‰ã«ç¢ºèªã‚’æ±‚ã‚ã‚‹ã‹ã€å®Ÿè¡Œè‡ªä½“ã‚’ãƒ–ãƒ­ãƒƒã‚¯ã§ãã¾ã™ã€‚

### AI ã‚¢ã‚·ã‚¹ã‚¿ãƒ³ãƒˆã¨ MCP ã‚µãƒ¼ãƒãƒ¼

AI ãƒ‰ãƒ­ãƒ¯ãƒ¼ï¼ˆ`Ctrl+Shift+A`ï¼‰ã¯ãƒ­ãƒ¼ã‚«ãƒ«ã® Ollamaï¼ˆ`http://127.0.0.1:11434/v1`ï¼‰ã€OpenAI äº’æ›ã‚¨ãƒ³ãƒ‰ãƒã‚¤ãƒ³ãƒˆã€OpenAIã€ã¾ãŸã¯ Anthropic ã«æŽ¥ç¶šã§ãã¾ã™ã€‚ãƒ¢ãƒ‡ãƒ«ã¸é€ä¿¡ã•ã‚Œã‚‹ã®ã¯ã‚¹ã‚­ãƒ¼ãƒžæƒ…å ±ï¼ˆãƒ†ãƒ¼ãƒ–ãƒ«åã€ã‚«ãƒ©ãƒ åã€ãƒ‡ãƒ¼ã‚¿åž‹ã€SQL æ–¹è¨€ï¼‰ã®ã¿ã§ã€ãƒ†ãƒ¼ãƒ–ãƒ«å†…ã®è¡Œãƒ‡ãƒ¼ã‚¿ãŒå¤–éƒ¨ã«å‡ºã‚‹ã“ã¨ã¯ã‚ã‚Šã¾ã›ã‚“ã€‚

å¤–éƒ¨ã®ã‚³ãƒ¼ãƒ‡ã‚£ãƒ³ã‚°ã‚¨ãƒ¼ã‚¸ã‚§ãƒ³ãƒˆã‚„ IDE å‘ã‘ã«ã¯ã€`tablepro-mcp` ãŒ `127.0.0.1` ä¸Šã§ãƒ­ãƒ¼ã‚«ãƒ« HTTP MCP ã‚µãƒ¼ãƒãƒ¼ã‚’èµ·å‹•ã—ã¾ã™ã€‚Windows è³‡æ ¼æƒ…å ±ãƒžãƒãƒ¼ã‚¸ãƒ£ãƒ¼ã®ãƒ™ã‚¢ãƒ©ãƒ¼ãƒˆãƒ¼ã‚¯ãƒ³ã¨æŽ¥ç¶šå…ƒãƒ—ãƒ­ã‚»ã‚¹ã®æ¤œè¨¼ã§ä¿è­·ã•ã‚Œã¦ãŠã‚Šã€æ¬¡ã® 11 ãƒ„ãƒ¼ãƒ«ã‚’æä¾›ã—ã¾ã™ã€‚

- `list_connections`ã€`connect`ã€`list_databases`ã€`list_schemas`ã€`list_tables`ã€`describe_table`ã€`get_table_ddl`ã€`browse_table`ã€`count_rows`ã€`execute_query`ã€`export_data`

`execute_query` ã¯å„æŽ¥ç¶šã® External Clients è¨­å®šãŠã‚ˆã³ã‚»ãƒ¼ãƒ•ãƒ¢ãƒ¼ãƒ‰ã«å¾“ã„ã€æ›¸ãè¾¼ã¿ã‚¯ã‚¨ãƒªã‚’å®Ÿè¡Œã™ã‚‹å‰ã«ã¯ TableMinus ç”»é¢ä¸Šã§ãƒ¦ãƒ¼ã‚¶ãƒ¼ã®ç¢ºèªã‚’è¦æ±‚ã—ã¾ã™ã€‚

### ãƒ†ãƒ¼ãƒžã€èƒŒæ™¯ãƒžãƒ†ãƒªã‚¢ãƒ«ã€å¤šè¨€èªžå¯¾å¿œ

çµ„ã¿è¾¼ã¿ãƒ†ãƒ¼ãƒžã¯ 14 ç¨®é¡žï¼ˆ`Dark`ã€`Light`ã€`High Contrast`ã€`Gaussian Glass Dark`ã€`Gaussian Glass Light`ã€`Tokyo Night`ã€`Dracula`ã€`Catppuccin Mocha`ã€`One Dark Pro`ã€`Nord`ã€`Monokai Pro`ã€`Gruvbox Dark`ã€`GitHub Light`ã€`Catppuccin Latte`ï¼‰ã‚ã‚Šã¾ã™ã€‚UI ãƒ†ãƒ¼ãƒžã¨ã‚³ãƒ¼ãƒ‰ç”¨ã‚·ãƒ³ã‚¿ãƒƒã‚¯ã‚¹ãƒ‘ãƒ¬ãƒƒãƒˆã¯å€‹åˆ¥ã«çµ„ã¿åˆã‚ã›ã‚‹ã“ã¨ãŒã§ãã€VS Code ã® `.json` ã‚«ãƒ©ãƒ¼ãƒ†ãƒ¼ãƒžã®ã‚¤ãƒ³ãƒãƒ¼ãƒˆã«ã‚‚å¯¾å¿œã—ã¦ã„ã¾ã™ã€‚

Windows 11 ã§ã¯ãƒ¡ã‚¤ãƒ³ã‚¦ã‚£ãƒ³ãƒ‰ã‚¦ã« DWM ã® `Gaussian Blur`ã€`Mica`ã€`Mica Alt`ã€`Opaque` ã‚’é©ç”¨ã§ãã€ãƒ¢ãƒ¼ãƒ€ãƒ«ã‚„ãƒ‰ãƒ­ãƒƒãƒ—ãƒ€ã‚¦ãƒ³ãªã©ã®ã‚ªãƒ¼ãƒãƒ¼ãƒ¬ã‚¤ã¯è¦–èªæ€§ã‚’ä¿ã¤ãŸã‚ä¸é€æ˜Žï¼ˆ`1.0` alphaï¼‰ã§æç”»ã—ã¾ã™ã€‚è¦–è¦šåŠ¹æžœã®è»½æ¸›ã¯ Windows ã® `SPI_GETCLIENTAREAANIMATION` ã«é€£å‹•ã—ã€OS å´ã§ã‚¢ãƒ‹ãƒ¡ãƒ¼ã‚·ãƒ§ãƒ³ãŒç„¡åŠ¹ãªå ´åˆã¯ãƒˆãƒ©ãƒ³ã‚¸ã‚·ãƒ§ãƒ³ã‚’è¡Œã„ã¾ã›ã‚“ã€‚UI ãƒ©ãƒ™ãƒ«ã€ãƒ¡ãƒ‹ãƒ¥ãƒ¼ã€ãƒ¢ãƒ¼ãƒ€ãƒ«ã€ãƒ„ãƒ¼ãƒ«ãƒãƒƒãƒ—ã€ãƒ‰ãƒ©ã‚¤ãƒè¨ºæ–­ãƒ¡ãƒƒã‚»ãƒ¼ã‚¸ã¯ 6 è¨€èªžï¼ˆ`en-US`ã€`vi-VN`ã€`zh-CN`ã€`ja-JP`ã€`es-ES`ã€`fr-FR`ï¼‰ã«å¯¾å¿œã—ã¦ã„ã¾ã™ã€‚

### CLI ã®ä½¿ã„æ–¹

`tableminus.exe` ã¯ GUI ãƒ©ãƒ³ãƒãƒ£ãƒ¼ã¨ã‚¹ã‚¯ãƒªãƒ—ãƒˆå‘ã‘ CLIï¼ˆ`tablepro-cli`ï¼‰ã‚’å…¼ã­ã¦ã„ã¾ã™ã€‚`.sqlite`ã€`.db`ã€`.duckdb`ã€`.sql` ã®ãƒ•ã‚¡ã‚¤ãƒ«ãƒ‘ã‚¹ã‚„æŽ¥ç¶š URLï¼ˆ`postgresql://...`ï¼‰ã‚’æ¸¡ã™ã¨ Named Pipe IPC çµŒç”±ã§èµ·å‹•ä¸­ã®ã‚¦ã‚£ãƒ³ãƒ‰ã‚¦ã«é–‹ãã¾ã™ï¼ˆåˆ¥ã‚¦ã‚£ãƒ³ãƒ‰ã‚¦ã§é–‹ãå ´åˆã¯ `-n` / `--new-window` ã‚’æŒ‡å®šï¼‰ã€‚

ä¸»ãªãƒˆãƒƒãƒ—ãƒ¬ãƒ™ãƒ«ãƒ•ãƒ©ã‚°ï¼š
- `--version`ï¼ˆ`-V`ï¼‰ ãƒãƒ¼ã‚¸ãƒ§ãƒ³ã‚’è¡¨ç¤ºã—ã¦çµ‚äº†ã—ã¾ã™ã€‚
- `--help`ï¼ˆ`-h`ï¼‰ CLI ã®ä½¿ã„æ–¹ã¨ã‚ªãƒ—ã‚·ãƒ§ãƒ³ã‚’è¡¨ç¤ºã—ã¾ã™ã€‚
- `--smoke-startup` ãƒ˜ãƒƒãƒ‰ãƒ¬ã‚¹ã§èµ·å‹•æ™‚é–“ã‚’è¨ˆæ¸¬ã—ã€9 ãƒ‰ãƒ©ã‚¤ãƒã™ã¹ã¦ã®ç™»éŒ²ã‚’ç¢ºèªã—ã¾ã™ã€‚

ã‚µãƒ–ã‚³ãƒžãƒ³ãƒ‰ï¼š`open`ã€`query`ã€`connections`ã€`databases`ã€`schemas`ã€`tables`ã€`describe`ã€`export`ã€`status`ã€`completion`ï¼ˆ`bash` / `zsh` / `fish`ï¼‰ã€‚`--format`ã€`--read-only`ã€`--limit`ã€`--timeout` ãªã©ã®ã‚ªãƒ—ã‚·ãƒ§ãƒ³ãŒä½¿ãˆã¾ã™ã€‚

```powershell
# èª­ã¿å–ã‚Šå°‚ç”¨ã‚¯ã‚¨ãƒªã‚’å®Ÿè¡Œã—ã¦ JSON Lines ã§å‡ºåŠ›
.\target\release\tableminus.exe query -c Staging --read-only --format jsonl "SELECT id, email FROM users LIMIT 10"

# ãƒ†ãƒ¼ãƒ–ãƒ«ã® CREATE TABLE DDL ã‚’è¡¨ç¤º
.\target\release\tableminus.exe describe -c Staging --ddl users
```

---

## ãƒ€ã‚¦ãƒ³ãƒ­ãƒ¼ãƒ‰ã¨ã‚¤ãƒ³ã‚¹ãƒˆãƒ¼ãƒ«

å¯¾å¿œ OS ã¯ **Windows 10 / Windows 11ï¼ˆx64ï¼‰** ã§ã™ã€‚ãƒ“ãƒ«ãƒ‰æ¸ˆã¿ãƒã‚¤ãƒŠãƒªã¯ [GitHub Releases](https://github.com/williamcachamwri/TableMinus/releases) ã§é…å¸ƒã—ã¦ã„ã¾ã™ã€‚

- **ã‚¤ãƒ³ã‚¹ãƒˆãƒ¼ãƒ©ãƒ¼ï¼ˆ`TableMinus-win-Setup.exe`ï¼‰**: UAC ã®ç®¡ç†è€…æ˜‡æ ¼ãªã—ã§ `%LocalAppData%` ã«ã‚¤ãƒ³ã‚¹ãƒˆãƒ¼ãƒ«ã—ã€ã‚¹ã‚¿ãƒ¼ãƒˆãƒ¡ãƒ‹ãƒ¥ãƒ¼ã¨ãƒ‡ã‚¹ã‚¯ãƒˆãƒƒãƒ—ã«ã‚·ãƒ§ãƒ¼ãƒˆã‚«ãƒƒãƒˆã‚’ä½œæˆã—ã¾ã™ã€‚Velopack ã«ã‚ˆã‚‹ãƒãƒƒã‚¯ã‚°ãƒ©ã‚¦ãƒ³ãƒ‰ã®å·®åˆ†ã‚¢ãƒƒãƒ—ãƒ‡ãƒ¼ãƒˆã«å¯¾å¿œã—ã¦ãŠã‚Šã€è¨­å®šã¯ **Settings -> About** ã‹ã‚‰å¤‰æ›´ã§ãã¾ã™ã€‚
- **ãƒãƒ¼ã‚¿ãƒ–ãƒ«ç‰ˆï¼ˆ`TableMinus-win-Portable.zip`ï¼‰**: ä»»æ„ã®ãƒ•ã‚©ãƒ«ãƒ€ã«å±•é–‹ã—ã¦ `tableminus.exe` ã‚’ç›´æŽ¥å®Ÿè¡Œã§ãã€ã‚¤ãƒ³ã‚¹ãƒˆãƒ¼ãƒ©ãƒ¼å´ã®çŠ¶æ…‹ã‚’å¤‰æ›´ã—ã¾ã›ã‚“ã€‚

å„ãƒªãƒªãƒ¼ã‚¹ã«ã¯ `SHA256SUMS.txt` ãŒå«ã¾ã‚Œã¦ãŠã‚Šã€PowerShell ã‹ã‚‰ãƒãƒƒã‚·ãƒ¥ã‚’ç¢ºèªã§ãã¾ã™ã€‚

```powershell
Get-FileHash .\TableMinus-win-Setup.exe -Algorithm SHA256
```

---

## ã‚­ãƒ¼ãƒœãƒ¼ãƒ‰ã‚·ãƒ§ãƒ¼ãƒˆã‚«ãƒƒãƒˆ

| ã‚·ãƒ§ãƒ¼ãƒˆã‚«ãƒƒãƒˆ | æ“ä½œ |
| :--- | :--- |
| `Ctrl+K` / `Ctrl+Shift+K` / `Ctrl+P` | ã‚³ãƒžãƒ³ãƒ‰ãƒ‘ãƒ¬ãƒƒãƒˆã‚’é–‹ã |
| `Ctrl+Enter` / `F5` | ã‚¢ã‚¯ãƒ†ã‚£ãƒ–ãª SQL ã‚¯ã‚¨ãƒªã‚’å®Ÿè¡Œ |
| `Ctrl+Shift+Enter` | é¸æŠžã—ãŸ SQL ãƒ†ã‚­ã‚¹ãƒˆã‚’å®Ÿè¡Œ |
| `Shift+Esc` | å®Ÿè¡Œä¸­ã®ã‚¯ã‚¨ãƒªã‚’ã‚­ãƒ£ãƒ³ã‚»ãƒ« |
| `Ctrl+S` | ã‚¯ã‚¨ãƒªã‚’ãƒ©ã‚¤ãƒ–ãƒ©ãƒªã«ä¿å­˜ |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | æ–°è¦ã‚¿ãƒ– / ã‚¿ãƒ–ã‚’é–‰ã˜ã‚‹ / é–‰ã˜ãŸã‚¿ãƒ–ã‚’é–‹ãç›´ã™ |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | æ¬¡ã®ã‚¿ãƒ– / å‰ã®ã‚¿ãƒ– |
| `Ctrl+F` | ã‚µã‚¤ãƒ‰ãƒãƒ¼ã®ã‚ªãƒ–ã‚¸ã‚§ã‚¯ãƒˆãƒ•ã‚£ãƒ«ã‚¿ã«ãƒ•ã‚©ãƒ¼ã‚«ã‚¹ |
| `Ctrl+B` | ã‚ªãƒ–ã‚¸ã‚§ã‚¯ãƒˆã‚µã‚¤ãƒ‰ãƒãƒ¼ã®è¡¨ç¤ºåˆ‡æ›¿ |
| `Ctrl+I` | Row Inspector ã®è¡¨ç¤ºåˆ‡æ›¿ |
| `Ctrl+Shift+A` | AI ã‚¢ã‚·ã‚¹ã‚¿ãƒ³ãƒˆãƒ‰ãƒ­ãƒ¯ãƒ¼ã®è¡¨ç¤ºåˆ‡æ›¿ |
| `Ctrl+Shift+O` | æŽ¥ç¶šãƒ©ã‚¤ãƒ–ãƒ©ãƒªã‚’é–‹ã |
| `Ctrl+,` | è¨­å®šã‚’é–‹ã |
| `Ctrl+Shift+M` | ãƒ†ãƒ¼ãƒžã‚’åˆ‡ã‚Šæ›¿ãˆ |
| `Ctrl+Shift+S` / `Ctrl+Shift+R` | ã‚°ãƒªãƒƒãƒ‰ç·¨é›†ã‚’ Commit / Rollback |
| `Alt+Insert` / `Ctrl+Shift+Backspace` | è¡Œã‚’è¿½åŠ  / é¸æŠžã—ãŸè¡Œã‚’å‰Šé™¤ |
| `Ctrl+Shift+E` | ã‚¯ã‚¨ãƒªçµæžœã¾ãŸã¯ãƒ†ãƒ¼ãƒ–ãƒ«ã‚’ã‚¨ã‚¯ã‚¹ãƒãƒ¼ãƒˆ |
| `Esc` | ã‚¢ã‚¯ãƒ†ã‚£ãƒ–ãªãƒ¢ãƒ¼ãƒ€ãƒ«ã¾ãŸã¯ã‚ªãƒ¼ãƒãƒ¼ãƒ¬ã‚¤ã‚’é–‰ã˜ã‚‹ |

SQL ã‚¨ãƒ‡ã‚£ã‚¿ã®ãƒ„ãƒ¼ãƒ«ãƒãƒ¼ã‹ã‚‰ Vim ãƒŠãƒ“ã‚²ãƒ¼ã‚·ãƒ§ãƒ³ï¼ˆ`NORMAL`ã€`INSERT`ã€`VISUAL`ã€`:` ã‚³ãƒžãƒ³ãƒ‰ï¼‰ã‚’æœ‰åŠ¹ã«ã§ãã¾ã™ã€‚

---

## ã‚½ãƒ¼ã‚¹ã‹ã‚‰ã®ãƒ“ãƒ«ãƒ‰

### å‹•ä½œè¦ä»¶

- Windows 10 ã¾ãŸã¯ Windows 11ï¼ˆx64ï¼‰
- [rustup](https://rustup.rs/) çµŒç”±ã®å®‰å®šç‰ˆ Rust MSVC ãƒ„ãƒ¼ãƒ«ãƒã‚§ãƒ¼ãƒ³ï¼ˆ`x86_64-pc-windows-msvc`ã€Rust `2024` Editionï¼‰
- Visual Studio C++ Build Tools ã¨ Windows SDKï¼ˆWin32ã€Direct3D 11ã€åŒæ¢± C ãƒ‰ãƒ©ã‚¤ãƒç”¨ï¼‰
- Windows PowerShellï¼ˆã‚³ãƒžãƒ³ãƒ‰é€£çµã«ã¯ `&&` ã§ã¯ãªã `;` ã‚’ä½¿ç”¨ï¼‰

> [!CAUTION]
> ãƒ“ãƒ«ãƒ‰ã¨ãƒ†ã‚¹ãƒˆã¯å¸¸ã« `--release` ã§å®Ÿè¡Œã—ã¦ãã ã•ã„ã€‚ã‚·ãƒ³ãƒœãƒ«ãƒ‡ãƒãƒƒã‚°ãŒå¿…é ˆã§ãªã„é™ã‚Šã€ãƒ¯ãƒ¼ã‚¯ã‚¹ãƒšãƒ¼ã‚¹å…¨ä½“ã§ãƒ‡ãƒãƒƒã‚°ç‰ˆã® `cargo build` ã‚„ `cargo test` ã‚’å®Ÿè¡Œã—ãªã„ã§ãã ã•ã„ã€‚å…¨ã‚¯ãƒ¬ãƒ¼ãƒˆã¨ 9 ãƒ‰ãƒ©ã‚¤ãƒã® `target\debug` å†… PDB ãƒ•ã‚¡ã‚¤ãƒ«ã¯ 140 GB ã‚’è¶…ãˆã‚‹ã“ã¨ãŒã‚ã‚Šã¾ã™ã€‚ãƒ¡ãƒ¢ãƒªã«åˆ¶ç´„ã®ã‚ã‚‹ç’°å¢ƒã§ã¯ `-j 1` ã‚’ä»˜ã‘ã¦ãã ã•ã„ã€‚

### ãƒ“ãƒ«ãƒ‰ãƒ»ãƒ†ã‚¹ãƒˆã‚³ãƒžãƒ³ãƒ‰

```powershell
# target\release\tableminus.exe ã‚’ãƒ­ãƒƒã‚¯ã—ã¦ã„ã‚‹ãƒ—ãƒ­ã‚»ã‚¹ãŒãªã„ã‹ç¢ºèª
Get-Process tableminus -ErrorAction SilentlyContinue

# release ãƒ¢ãƒ¼ãƒ‰ã§ã®åž‹ãƒã‚§ãƒƒã‚¯ã¨ãƒ“ãƒ«ãƒ‰
cargo check --release -j 1 -p tableminus-windows-app
cargo build --release -j 1 -p tableminus-windows-app

# ãƒ˜ãƒƒãƒ‰ãƒ¬ã‚¹èµ·å‹•ãƒã‚§ãƒƒã‚¯ã®å®Ÿè¡Œã¨ã‚¢ãƒ—ãƒªèµ·å‹•
.\target\release\tableminus.exe --smoke-startup
.\target\release\tableminus.exe
```

ãƒªãƒªãƒ¼ã‚¹ç”¨ãƒ†ã‚¹ãƒˆã‚¹ã‚¤ãƒ¼ãƒˆï¼š

```powershell
cargo test --release -j 1 -p tablepro-core filter
cargo test --release -j 1 -p tableminus-e2e --test startup_perf_e2e -- --nocapture
cargo test --release -j 1 -p tableminus-e2e --test sqlite_e2e -- --nocapture
```

### ãƒ¯ãƒ¼ã‚¯ã‚¹ãƒšãƒ¼ã‚¹æ§‹æˆ

| ãƒ‘ã‚¹ | å½¹å‰² |
| :--- | :--- |
| `core/crates/core` | `tablepro-core`: ã‚¯ã‚¨ãƒªãƒ¢ãƒ‡ãƒ«ã€ãƒ•ã‚£ãƒ«ã‚¿ã€SQL æ–¹è¨€ã€ã‚¨ã‚¯ã‚¹ãƒãƒ¼ãƒˆã‚¨ãƒ³ã‚³ãƒ¼ãƒ€ |
| `core/crates/session` | `tablepro-session`: ã‚»ãƒƒã‚·ãƒ§ãƒ³ç®¡ç†ã€æŽ¥ç¶šãƒ—ãƒ¼ãƒ«ã€ãƒˆãƒ©ãƒ³ã‚¶ã‚¯ã‚·ãƒ§ãƒ³ |
| `core/crates/cli` | `tablepro-cli`: CLI ã‚µãƒ–ã‚³ãƒžãƒ³ãƒ‰ã¨ãƒ˜ãƒƒãƒ‰ãƒ¬ã‚¹ãƒ©ãƒ³ãƒŠãƒ¼ |
| `core/crates/mcp` | `tablepro-mcp`: ãƒˆãƒ¼ã‚¯ãƒ³èªè¨¼ä»˜ããƒ­ãƒ¼ã‚«ãƒ«ãƒ›ã‚¹ãƒˆ MCP HTTP ã‚µãƒ¼ãƒãƒ¼ |
| `core/crates/ai` | `tablepro-ai`: ã‚¹ã‚­ãƒ¼ãƒžã®ã¿ã‚’æ‰±ã† Ollama / OpenAI / Anthropic ã‚¯ãƒ©ã‚¤ã‚¢ãƒ³ãƒˆ |
| `core/crates/ssh` | `tablepro-ssh`: `russh` ã«ã‚ˆã‚‹ãƒ—ãƒ­ã‚»ã‚¹å†… SSH ãƒˆãƒ³ãƒãƒªãƒ³ã‚° |
| `core/crates/net` | `tablepro-net`: `rustls` TLS ã¨ã‚·ã‚¹ãƒ†ãƒ è¨¼æ˜Žæ›¸æ¤œè¨¼ |
| `core/crates/storage` | `tablepro-storage`: ä¿å­˜æ¸ˆã¿æŽ¥ç¶šã€ã‚¯ã‚¨ãƒªãƒ©ã‚¤ãƒ–ãƒ©ãƒªã€å±¥æ­´ |
| `core/crates/foreign-import` | `tablepro-foreign-import`: ä»–ã® DB ã‚¯ãƒ©ã‚¤ã‚¢ãƒ³ãƒˆã‹ã‚‰ã®æŽ¥ç¶šã‚¤ãƒ³ãƒãƒ¼ãƒˆ |
| `core/crates/account` | `tablepro-account`: ã‚¢ã‚«ã‚¦ãƒ³ãƒˆã¨ãƒˆãƒ¼ã‚¯ãƒ³ã®ãƒˆãƒ¬ã‚¤ãƒˆ |
| `core/crates/drivers/*` | 9 ãƒ‡ãƒ¼ã‚¿ãƒ™ãƒ¼ã‚¹ãƒ‰ãƒ©ã‚¤ãƒï¼ˆ`postgres`, `mysql`, `sqlite`, `duckdb`, `mssql`, `clickhouse`, `cassandra`, `mongodb`, `redis`ï¼‰ |
| `crates/tableminus-gfx` | ãƒ™ã‚¯ã‚¿ãƒ¼ã‚°ãƒªãƒ•ã¨ãƒ‡ãƒ¼ã‚¿ãƒ™ãƒ¼ã‚¹ã‚¨ãƒ³ã‚¸ãƒ³ã‚¢ã‚¤ã‚³ãƒ³ |
| `crates/tableminus-state` | ã‚¨ãƒ‡ã‚£ã‚¿çŠ¶æ…‹ã€è£œå®Œã€ã‚°ãƒªãƒƒãƒ‰ãƒ¢ãƒ‡ãƒ«ã€ã‚¿ãƒ–ã€14 çµ„ã¿è¾¼ã¿ãƒ†ãƒ¼ãƒž |
| `crates/tableminus-services` | éžåŒæœŸ Tokio ãƒ©ãƒ³ã‚¿ã‚¤ãƒ ã€ã‚»ãƒƒã‚·ãƒ§ãƒ³/AI/MCP åˆ¶å¾¡ã€å¤šè¨€èªžè¨ºæ–­ |
| `crates/tableminus-windows-platform` | è¨­å®š JSONã€Win32 è³‡æ ¼æƒ…å ±ãƒžãƒãƒ¼ã‚¸ãƒ£ãƒ¼ã€DWMãƒ»è¦–è¦šåŠ¹æžœè»½æ¸›ãƒ˜ãƒ«ãƒ‘ãƒ¼ |
| `crates/tableminus-windows-app` | HeroGPUIï¼ˆDirect3D 11ï¼‰ãƒ‡ã‚¹ã‚¯ãƒˆãƒƒãƒ—ã‚¢ãƒ—ãƒªã€Velopack ã‚¢ãƒƒãƒ—ãƒ‡ãƒ¼ã‚¿ã€6 è¨€èªž |
| `crates/tableminus-e2e` | SQLiteã€DuckDBã€èµ·å‹•æ™‚é–“ã€ã‚°ãƒªãƒƒãƒ‰æç”»ã®ãƒ™ãƒ³ãƒãƒžãƒ¼ã‚¯ |

---

## ã‚³ãƒ³ãƒˆãƒªãƒ“ãƒ¥ãƒ¼ãƒˆã¨ãƒ©ã‚¤ã‚»ãƒ³ã‚¹

ãƒã‚°å ±å‘Šã€æ©Ÿèƒ½æ”¹å–„ã€ç¿»è¨³ä¿®æ­£ãªã©ã®ã‚³ãƒ³ãƒˆãƒªãƒ“ãƒ¥ãƒ¼ãƒˆã‚’æ­“è¿Žã—ã¾ã™ã€‚ãƒ“ãƒ«ãƒ‰ã‚„ GPUI ã®åˆ¶ç´„ã«ã¤ã„ã¦ã¯ [CONTRIBUTING.md](CONTRIBUTING.md)ã€ãƒªãƒªãƒ¼ã‚¹å±¥æ­´ã¯ [CHANGELOG.md](CHANGELOG.md)ã€è¡Œå‹•è¦ç¯„ã¯ [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)ã€è„†å¼±æ€§ã®éžå…¬é–‹å ±å‘Šæ‰‹é †ã¯ [SECURITY.md](SECURITY.md) ã‚’ã”è¦§ãã ã•ã„ã€‚

TableMinus ã¯ [LICENSE](LICENSE)ï¼ˆGNU Affero General Public License v3.0ï¼‰ãŠã‚ˆã³ [LICENSE-MIT](LICENSE-MIT)ï¼ˆMITã€ä¸Šæµã‚³ã‚¢ã§ã‚ã‚‹ [TableProApp/TablePro](https://github.com/TableProApp/TablePro) ã¸ã®å¸°å±žè¡¨ç¤ºã‚’å«ã‚€ï¼‰ã®ã‚‚ã¨ã§ãƒ©ã‚¤ã‚»ãƒ³ã‚¹ã•ã‚Œã¦ã„ã¾ã™ã€‚
