<div align="center">

<img src="crates/tableminus-windows-app/assets/tableminus_logo.png" alt="TableMinus Logo" width="112" height="112" />

# TableMinus

**åŸºäºŽ Rust ä¸Ž Direct3D 11 æž„å»ºçš„ Windows åŽŸç”Ÿ SQL ä¸Žæ•°æ®åº“å·¥ä½œå°ã€‚**

[![CI](https://img.shields.io/github/actions/workflow/status/williamcachamwri/TableMinus/ci.yml?branch=main&style=flat-square&label=CI&logo=github)](https://github.com/williamcachamwri/TableMinus/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/actions/workflow/status/williamcachamwri/TableMinus/release.yml?style=flat-square&label=Release&logo=github)](https://github.com/williamcachamwri/TableMinus/actions/workflows/release.yml)
[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![GPUI](https://img.shields.io/badge/UI-HeroGPUI%20%2F%20Direct3D%2011-0078D4?style=flat-square&logo=windows&logoColor=white)](#æ¦‚è¿°)
[![Drivers](https://img.shields.io/badge/Drivers-9%20Engines-10B981?style=flat-square)](#æ”¯æŒçš„æ•°æ®åº“)
[![Memory](https://img.shields.io/badge/RAM-~55%20MB-F59E0B?style=flat-square)](#æ¦‚è¿°)
[![Startup](https://img.shields.io/badge/å†·å¯åŠ¨-~10%20ms-6366F1?style=flat-square)](#æ¦‚è¿°)
[![Windows](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-0078D4?style=flat-square&logo=windows11&logoColor=white)](#ä¸‹è½½ä¸Žå®‰è£…)
[![License: MIT / AGPL-3.0](https://img.shields.io/badge/License-MIT%20%2F%20AGPL--3.0-blue?style=flat-square)](LICENSE-MIT)

[English](README.md) Â· [Tiáº¿ng Viá»‡t](README.vi.md) Â· [ç®€ä½“ä¸­æ–‡](README.zh-CN.md) Â· [æ—¥æœ¬èªž](README.ja.md) Â· [EspaÃ±ol](README.es.md) Â· [FranÃ§ais](README.fr.md)

</div>

---

## æ¦‚è¿°

TableMinus æ˜¯ä¸€æ¬¾ç”¨ Rust ç¼–å†™çš„ Windows åŽŸç”Ÿ SQL ä¸Žæ•°æ®åº“å·¥ä½œå°ã€‚ç•Œé¢é€šè¿‡ [HeroGPUI](https://github.com/williamcachamwri/TableMinus) ç›´æŽ¥åœ¨ Direct3D 11 ä¸Šæ¸²æŸ“ï¼Œåº•å±‚åŸºäºŽå¼€æºçš„ [TablePro](https://github.com/TableProApp/TablePro) Rust æ•°æ®åº“å†…æ ¸ï¼Œä¸ä¾èµ– Electron æˆ– JVMã€‚

åœ¨ Windows 11 çš„ release æž„å»ºä¸­ï¼Œå†·å¯åŠ¨æ—¶é—´çº¦ä¸º `10 ms`ï¼ˆé€šè¿‡ `--smoke-startup` åŠ è½½å…¨éƒ¨ 9 ä¸ªé©±åŠ¨å®žæµ‹ä¸º `9.21` åˆ° `10.11 ms`ï¼‰ï¼›åœ¨ `100k`ã€`500k` å’Œ `1M` è¡ŒåŽ‹åŠ›æŸ¥è¯¢æœŸé—´ï¼Œè¿›ç¨‹å†…å­˜å ç”¨ç¨³å®šåœ¨ `~55 MB` å·¦å³ã€‚åœ¨ `tableminus-e2e` SQLite æµ‹è¯•å¥—ä»¶ä¸­ï¼Œå‘æ•°æ®ç½‘æ ¼åŠ è½½ `100k` è¡Œè€—æ—¶ `24.43 ms`ï¼Œæ»šåŠ¨ `10k` å¸§è€—æ—¶ `0.432 ms`ã€‚

---

## ç•Œé¢æˆªå›¾

### SQL ç¼–è¾‘å™¨ã€å®žæ—¶å›¾è¡¨ä¸ŽæŸ¥è¯¢ç»“æžœ

![TableMinus SQL Editor and Live Charts](docs/screenshots/sql-editor-charts.png)

### è™šæ‹ŸåŒ–æ•°æ®ç½‘æ ¼ã€åˆ—èšåˆç»Ÿè®¡ä¸Žå‚æ•°åŒ–ç­›é€‰

![TableMinus Data Grid view](docs/screenshots/data-grid.png)

### äº¤äº’å¼ ER å…³ç³»å›¾ã€å¤–é”®è¿žçº¿ä¸Ž SVG å¯¼å‡º

![TableMinus ER Diagram Workbench](docs/screenshots/er-diagram.png)

### æ•°æ®å¯¼å…¥å‘å¯¼ï¼ˆCSVã€JSONã€XLSXï¼‰ä¸Žè‡ªåŠ¨ç±»åž‹æ˜ å°„

![TableMinus Import Wizard](docs/screenshots/import-wizard.png)

### æž¶æž„ä¸Žæ•°æ®å¯¹æ¯”ã€å·®å¼‚æ±‡æ€»ä¸ŽåŒæ­¥è„šæœ¬ç”Ÿæˆ

![TableMinus Schema and Data Compare](docs/screenshots/data-compare.png)

### æ¨¡æ‹Ÿæ•°æ®ç”Ÿæˆå™¨ï¼ˆç¡®å®šæ€§ç§å­ä¸Žå¤–é”®æ‹“æ‰‘æŽ’åºï¼‰

![TableMinus Mock Data Generator](docs/screenshots/mock-generator.png)

### è·¨å¼•æ“Žæ•°æ®ä¼ è¾“ä¸Žå®žæ—¶ DDL é¢„è§ˆ

![TableMinus Cross-Engine Data Transfer](docs/screenshots/data-transfer.png)

### å…ƒæ•°æ®å…¨å±€æœç´¢é¢æ¿ï¼ˆMetadata Searchï¼‰

![TableMinus Metadata Search](docs/screenshots/metadata-search.png)

### ä»…å‘é€ Schema çš„ AI æŠ½å±‰ï¼ˆ`Ctrl+Shift+A`ï¼‰

![TableMinus Schema-Aware AI Assistant](docs/screenshots/ai-assistant.png)

---

## æ”¯æŒçš„æ•°æ®åº“

TableMinus å°† 9 ä¸ªæ•°æ®åº“é©±åŠ¨ç›´æŽ¥ç¼–è¯‘è¿› `tableminus.exe`ï¼Œæ— éœ€é¢å¤–é…ç½® ODBC é©±åŠ¨æˆ–æ’ä»¶ï¼š

| æ•°æ®åº“ | é©±åŠ¨ Crate | è¯´æ˜Ž |
| :--- | :--- | :--- |
| PostgreSQL | `tablepro-driver-postgres` | `tokio-postgres` + `deadpool`ï¼Œæ”¯æŒ `COPY IN`ã€Schema ä¸Ž DDL æŸ¥çœ‹ |
| MySQL / MariaDB | `tablepro-driver-mysql` | å®Œæ•´ SQL æ–¹è¨€ä¸Žç›®å½•å…ƒæ•°æ®æ”¯æŒ |
| SQLite | `tablepro-driver-sqlite` | æ”¯æŒç›´æŽ¥æ‰“å¼€ã€æ‹–æ‹½æˆ–é€šè¿‡ CLI åŠ è½½ `.sqlite` / `.db` æœ¬åœ°æ–‡ä»¶ |
| DuckDB | `tablepro-driver-duckdb` | å†…ç½® DuckDB 2.0ï¼Œé™æ€é“¾æŽ¥ `json` ä¸Ž `parquet` |
| Microsoft SQL Server | `tablepro-driver-mssql` | çº¯ Rust TDS 8.0ï¼ˆ`tiberius`ï¼‰ï¼Œæ”¯æŒ TLS ä¸Žå¤šç»“æžœé›†æ‰¹å¤„ç† |
| ClickHouse | `tablepro-driver-clickhouse` | é€šè¿‡ `hyper-rustls` æ‰§è¡Œ HTTP/TLS æŸ¥è¯¢ |
| Cassandra / ScyllaDB | `tablepro-driver-cassandra` | åŸºäºŽ `rustls` çš„åŽŸç”Ÿ CQL é©±åŠ¨ï¼ˆ`scylla`ï¼‰ |
| MongoDB | `tablepro-driver-mongodb` | æ–‡æ¡£æŸ¥è¯¢ã€BSON 3 æ˜ å°„ä¸Ž DNS ç§å­åˆ—è¡¨è§£æž |
| Redis | `tablepro-driver-redis` | åŸºäºŽå¼‚æ­¥ TLS/TCP çš„é”®å€¼æŸ¥çœ‹ä¸Žå‘½ä»¤æ‰§è¡Œ |

è¿žæŽ¥å±‚å†…ç½®åŸºäºŽ `russh` çš„è¿›ç¨‹å†… SSH éš§é“ï¼ˆ`tablepro-ssh`ï¼Œæ”¯æŒå¯†ç ã€ç§é’¥ã€å£ä»¤å¼¹çª—ä¸Žä¸»æœºå¯†é’¥æ ¡éªŒï¼‰ã€åŸºäºŽ `rustls` ä¸Ž Windows è¯ä¹¦å­˜å‚¨çš„ TLSï¼ˆ`Disable`ã€`Prefer`ã€`Require`ã€`VerifyCA`ã€`VerifyFull`ï¼Œæ”¯æŒè‡ªå®šä¹‰ CA ä¸Žå®¢æˆ·ç«¯è¯ä¹¦/ç§é’¥ï¼‰ã€å®‰å…¨æ¨¡å¼ç­–ç•¥ï¼ˆ`Off`ã€`ConfirmWrites`ã€`ReadOnly`ï¼‰ï¼Œä»¥åŠé€šè¿‡ `tablepro-foreign-import` ä»Žå…¶ä»–æ•°æ®åº“å®¢æˆ·ç«¯å¯¼å…¥å·²ä¿å­˜çš„è¿žæŽ¥ã€‚

---

## åŠŸèƒ½ç‰¹æ€§

### SQL ç¼–è¾‘å™¨ä¸Žæ•°æ®ç½‘æ ¼

ç¼–è¾‘å™¨æ”¯æŒæŒ‰æ–¹è¨€åŒºåˆ†çš„è¯­æ³•é«˜äº®ï¼ˆ`PostgreSQL`ã€`MySQL`ã€`SQLite`ã€`MsSql` ç­‰ï¼‰ã€è¡¨/åˆ—/Schema/å…³é”®å­—ä¸Ž SQL å­å¥ï¼ˆ`FROM`ã€`JOIN`ã€`INTO`ã€`UPDATE`ï¼‰è‡ªåŠ¨è¡¥å…¨ã€å¤šå…‰æ ‡ç¼–è¾‘ã€å·²ä¿å­˜æŸ¥è¯¢åº“ã€æŒä¹…åŒ–æŸ¥è¯¢åŽ†å²ï¼Œä»¥åŠå¯é€‰çš„ Vim æ¨¡å¼ï¼ˆ`NORMAL`ã€`INSERT`ã€`VISUAL`ã€`COMMAND`ï¼‰ã€‚

åœ¨æ•°æ®ç½‘æ ¼ä¸­ï¼Œå¯ä»¥é€šè¿‡åœé çš„ Row Inspectorï¼ˆ`Ctrl+I`ï¼‰æŸ¥çœ‹è¡Œè¯¦æƒ…ï¼Œåˆ‡æ¢ Structureã€Columnsã€Indexesã€Foreign Keys ä¸Ž DDL æ ‡ç­¾é¡µï¼Œå›ºå®šå‰å¯¼åˆ—ï¼ˆæœ€å¤šå è§†å£å®½åº¦çš„ 2/3ï¼‰ï¼Œè°ƒæ•´æˆ–éšè—åˆ—ï¼Œå¹¶å åŠ  11 ç§å‚æ•°åŒ–è¿‡æ»¤æ“ä½œç¬¦ï¼ˆ`=`ã€`!=`ã€`Contains`ã€`StartsWith`ã€`EndsWith`ã€`&lt;`ã€`&lt;=`ã€`&gt;`ã€`&gt;=`ã€`IS NULL`ã€`IS NOT NULL`ï¼‰ã€‚å•å…ƒæ ¼ä¿®æ”¹ã€æ’å…¥è¡Œï¼ˆ`Alt+Insert`ï¼‰å’Œåˆ é™¤è¡Œï¼ˆ`Ctrl+Shift+Backspace`ï¼‰ä¼šå…ˆæš‚å­˜ä¸ºæ‰¹æ¬¡ï¼Œå¾…ç¡®è®¤åŽå†æ‰§è¡Œ `Commit`ï¼ˆ`Ctrl+Shift+S`ï¼‰æˆ– `Rollback`ï¼ˆ`Ctrl+Shift+R`ï¼‰ã€‚

é€šè¿‡ `Ctrl+Shift+E` å¯å°†æŸ¥è¯¢ç»“æžœæˆ–æ•°æ®è¡¨å¯¼å‡ºä¸º CSVï¼ˆ`.csv`ï¼‰ã€JSON Arrayï¼ˆ`.json`ï¼‰ã€JSON Linesï¼ˆ`.jsonl`ï¼‰æˆ– SQL `INSERT`ï¼ˆ`.sql`ï¼‰ï¼Œå†™å…¥è¿‡ç¨‹ç»ç”± `.part` ä¸´æ—¶æ–‡ä»¶åŽŸå­å®Œæˆã€‚`tablepro-core` åŒæ ·å†…ç½®äº† TSVã€XML ä¸Žæ’å®šå†…å­˜å ç”¨çš„ XLSX ç¼–ç å™¨ã€‚

### å®‰å…¨ä¸Žæœ¬åœ°å­˜å‚¨

æ•°æ®åº“å¯†ç ã€SSH å¯†é’¥ã€AI API å¯†é’¥ä»¥åŠ MCP Bearer Token å‡å­˜å‚¨åœ¨ Windows å‡­æ®ç®¡ç†å™¨ï¼ˆ`CredWriteW` / `CredReadW`ï¼‰ä¸­ï¼Œå¹¶åœ¨å†…å­˜ä½¿ç”¨åŽé€šè¿‡ `zeroize` æ“¦é™¤ï¼Œä¸ä¼šä»¥æ˜Žæ–‡å½¢å¼å†™å…¥ JSON é…ç½®æ–‡ä»¶æˆ–æ—¥å¿—ã€‚è¿žæŽ¥åœ¨å®‰å…¨æ¨¡å¼ä¸‹å¯è®¾ä¸º `ConfirmWrites` æˆ– `ReadOnly`ï¼Œç¡®ä¿ `UPDATE`ã€`DELETE`ã€`DROP` ç­‰è¯­å¥åœ¨æ‰§è¡Œå‰å¼¹å‡ºç¡®è®¤æˆ–ç›´æŽ¥æ‹¦æˆªã€‚

### AI åŠ©æ‰‹ä¸Ž MCP æœåŠ¡

AI ä¾§è¾¹æŠ½å±‰ï¼ˆ`Ctrl+Shift+A`ï¼‰å¯è¿žæŽ¥æœ¬åœ° Ollamaï¼ˆ`http://127.0.0.1:11434/v1`ï¼‰ã€ä»»æ„å…¼å®¹ OpenAI æŽ¥å£çš„æœåŠ¡ã€OpenAI æˆ– Anthropicã€‚å‘é€ç»™æ¨¡åž‹çš„ä»…åŒ…å« Schema å…ƒæ•°æ®ï¼ˆè¡¨åã€åˆ—åã€æ•°æ®ç±»åž‹ä¸Ž SQL æ–¹è¨€ï¼‰ï¼Œè¡¨å†…çš„å®žé™…è¡Œæ•°æ®ä¸ä¼šç¦»å¼€æœ¬æœºã€‚

é’ˆå¯¹å¤–éƒ¨ç¼–ç¨‹ä»£ç†ä¸Ž IDEï¼Œ`tablepro-mcp` åœ¨æœ¬åœ°å›žçŽ¯åœ°å€ `127.0.0.1` ä¸Šè¿è¡Œ HTTP MCP æœåŠ¡ï¼Œé€šè¿‡ Windows å‡­æ®ç®¡ç†å™¨ä¸­çš„ Bearer Token åŠå¯¹ç«¯æ ¡éªŒè¿›è¡Œä¿æŠ¤ï¼Œå¹¶æä¾› 11 ä¸ªå·¥å…·ï¼š

- `list_connections`ã€`connect`ã€`list_databases`ã€`list_schemas`ã€`list_tables`ã€`describe_table`ã€`get_table_ddl`ã€`browse_table`ã€`count_rows`ã€`execute_query`ã€`export_data`

å…¶ä¸­ `execute_query` ä¼šä¸¥æ ¼éµå¾ªå„ä¸ªè¿žæŽ¥çš„ External Clients ä¸Žå®‰å…¨æ¨¡å¼é…ç½®ï¼Œä»»ä½•å†™å…¥è¯­å¥æ‰§è¡Œå‰éƒ½å¿…é¡»åœ¨ TableMinus ç•Œé¢å†…ç”±ç”¨æˆ·ç¡®è®¤ã€‚

### ä¸»é¢˜ã€çª—å£æè´¨ä¸Žå¤šè¯­è¨€

TableMinus å†…ç½® 14 å¥—ä¸»é¢˜ï¼ˆ`Dark`ã€`Light`ã€`High Contrast`ã€`Gaussian Glass Dark`ã€`Gaussian Glass Light`ã€`Tokyo Night`ã€`Dracula`ã€`Catppuccin Mocha`ã€`One Dark Pro`ã€`Nord`ã€`Monokai Pro`ã€`Gruvbox Dark`ã€`GitHub Light`ã€`Catppuccin Latte`ï¼‰ã€‚ç•Œé¢ä¸»é¢˜ä¸Žä»£ç è¯­æ³•é…è‰²å¯ç‹¬ç«‹æ­é…ï¼Œä¹Ÿæ”¯æŒå¯¼å…¥ VS Code çš„ `.json` é…è‰²ä¸»é¢˜ã€‚

åœ¨ Windows 11 ä¸Šï¼Œä¸»çª—å£æ”¯æŒ DWM `Gaussian Blur`ã€`Mica`ã€`Mica Alt` ä¸Ž `Opaque` å››ç§èƒŒæ™¯æè´¨ï¼ŒåŒæ—¶æ‰€æœ‰æµ®åŠ¨æµ®å±‚ä¿æŒå®Œå…¨ä¸é€æ˜Žï¼ˆ`1.0` alphaï¼‰ä»¥ç¡®ä¿æ–‡å­—æ¸…æ™°å¯è¯»ã€‚å‡å¼±åŠ¨æ€æ•ˆæžœé€‰é¡¹ä¼šè¯»å–ç³»ç»Ÿçš„ `SPI_GETCLIENTAREAANIMATION` è®¾ç½®ï¼Œåœ¨ç³»ç»Ÿå…³é—­åŠ¨ç”»æ—¶è·³è¿‡æŠ½å±‰å’Œå¼¹çª—è¿‡æ¸¡ã€‚æ‰€æœ‰ç•Œé¢æ–‡æœ¬ã€èœå•ã€å¯¹è¯æ¡†ã€æç¤ºä¸Žé©±åŠ¨è¯Šæ–­ä¿¡æ¯å‡å·²æœ¬åœ°åŒ–ä¸º 6 ç§è¯­è¨€ï¼š`en-US`ã€`vi-VN`ã€`zh-CN`ã€`ja-JP`ã€`es-ES` ä¸Ž `fr-FR`ã€‚

### å‘½ä»¤è¡Œç”¨æ³•

`tableminus.exe` åŒæ—¶æ‰¿æ‹…æ¡Œé¢å¯åŠ¨å™¨ä¸Žè„šæœ¬åŒ– CLIï¼ˆ`tablepro-cli`ï¼‰çš„è§’è‰²ã€‚ç›´æŽ¥ä¼ å…¥ `.sqlite`ã€`.db`ã€`.duckdb`ã€`.sql` æ–‡ä»¶è·¯å¾„æˆ–è¿žæŽ¥ URLï¼ˆ`postgresql://...`ï¼‰æ—¶ï¼Œä¼šé€šè¿‡ Named Pipe IPC åœ¨å½“å‰è¿è¡Œçª—å£ä¸­æ‰“å¼€ï¼ˆä¼ å…¥ `-n` / `--new-window` å¯æ–°å»ºçª—å£ï¼‰ã€‚

é¡¶å±‚å‚æ•°ï¼š
- `--version`ï¼ˆ`-V`ï¼‰æ‰“å°ç‰ˆæœ¬å·å¹¶é€€å‡ºã€‚
- `--help`ï¼ˆ`-h`ï¼‰æ‰“å° CLI ç”¨æ³•ä¸Žå‚æ•°è¯´æ˜Žã€‚
- `--smoke-startup` è¿è¡Œæ— ç•Œé¢å¯åŠ¨è€—æ—¶æ£€æŸ¥å¹¶æ ¡éªŒå…¨éƒ¨ 9 ä¸ªé©±åŠ¨æ³¨å†ŒçŠ¶æ€ã€‚

å­å‘½ä»¤åŒ…æ‹¬ï¼š`open`ã€`query`ã€`connections`ã€`databases`ã€`schemas`ã€`tables`ã€`describe`ã€`export`ã€`status` ä»¥åŠ `completion`ï¼ˆæ”¯æŒ `bash`ã€`zsh`ã€`fish`ï¼‰ï¼Œé…åˆ `--format`ã€`--read-only`ã€`--limit` ä¸Ž `--timeout` å‚æ•°ä½¿ç”¨ã€‚

```powershell
# æ‰§è¡Œåªè¯»æŸ¥è¯¢å¹¶ä»¥ JSON Lines æ ¼å¼è¾“å‡º
.\target\release\tableminus.exe query -c Staging --read-only --format jsonl "SELECT id, email FROM users LIMIT 10"

# æŸ¥çœ‹æ•°æ®è¡¨çš„ CREATE TABLE DDL
.\target\release\tableminus.exe describe -c Staging --ddl users
```

---

## ä¸‹è½½ä¸Žå®‰è£…

ç³»ç»Ÿè¦æ±‚ï¼š**Windows 10 æˆ– Windows 11ï¼ˆx64ï¼‰**ã€‚é¢„ç¼–è¯‘å®‰è£…åŒ…å‘å¸ƒåœ¨ [GitHub Releases](https://github.com/williamcachamwri/TableMinus/releases)ï¼š

- **å®‰è£…ç¨‹åºï¼ˆ`TableMinus-win-Setup.exe`ï¼‰**ï¼šå®‰è£…åˆ° `%LocalAppData%` å¹¶åˆ›å»ºå¼€å§‹èœå•ä¸Žæ¡Œé¢å¿«æ·æ–¹å¼ï¼Œæ— éœ€ UAC ç®¡ç†å‘˜æƒé™ã€‚å†…ç½® Velopack åŽå°å¢žé‡æ›´æ–°ï¼Œå¯åœ¨ **Settings -> About** ä¸­é…ç½®ã€‚
- **ä¾¿æºç‰ˆï¼ˆ`TableMinus-win-Portable.zip`ï¼‰**ï¼šè§£åŽ‹åˆ°ä»»æ„ç›®å½•å³å¯ç›´æŽ¥è¿è¡Œ `tableminus.exe`ï¼Œä¸ä¼šä¿®æ”¹å®‰è£…å™¨çŠ¶æ€ã€‚

æ¯æ¬¡å‘å¸ƒå‡é™„å¸¦ `SHA256SUMS.txt`ï¼Œå¯åœ¨ PowerShell ä¸­æ ¡éªŒæ–‡ä»¶å“ˆå¸Œï¼š

```powershell
Get-FileHash .\TableMinus-win-Setup.exe -Algorithm SHA256
```

---

## å¿«æ·é”®

| å¿«æ·é”® | åŠŸèƒ½ |
| :--- | :--- |
| `Ctrl+K` / `Ctrl+Shift+K` / `Ctrl+P` | æ‰“å¼€å‘½ä»¤é¢æ¿ |
| `Ctrl+Enter` / `F5` | æ‰§è¡Œå½“å‰ SQL æŸ¥è¯¢ |
| `Ctrl+Shift+Enter` | æ‰§è¡Œé€‰ä¸­çš„ SQL æ–‡æœ¬ |
| `Shift+Esc` | å–æ¶ˆæ­£åœ¨è¿è¡Œçš„æŸ¥è¯¢ |
| `Ctrl+S` | ä¿å­˜æŸ¥è¯¢åˆ°æŸ¥è¯¢åº“ |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | æ–°å»ºæ ‡ç­¾é¡µ / å…³é—­æ ‡ç­¾é¡µ / é‡æ–°æ‰“å¼€å·²å…³é—­æ ‡ç­¾é¡µ |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | ä¸‹ä¸€ä¸ª / ä¸Šä¸€ä¸ªæ ‡ç­¾é¡µ |
| `Ctrl+F` | èšç„¦ä¾§è¾¹æ å¯¹è±¡è¿‡æ»¤æ¡† |
| `Ctrl+B` | æ˜¾ç¤º/éšè—å¯¹è±¡ä¾§è¾¹æ  |
| `Ctrl+I` | æ˜¾ç¤º/éšè— Row Inspector |
| `Ctrl+Shift+A` | æ˜¾ç¤º/éšè— AI åŠ©æ‰‹æŠ½å±‰ |
| `Ctrl+Shift+O` | æ‰“å¼€è¿žæŽ¥åº“ |
| `Ctrl+,` | æ‰“å¼€è®¾ç½® |
| `Ctrl+Shift+M` | åˆ‡æ¢ä¸»é¢˜ |
| `Ctrl+Shift+S` / `Ctrl+Shift+R` | æäº¤ï¼ˆ`Commit`ï¼‰ / å›žæ»šï¼ˆ`Rollback`ï¼‰æš‚å­˜çš„ç½‘æ ¼ä¿®æ”¹ |
| `Alt+Insert` / `Ctrl+Shift+Backspace` | æ–°å¢žè¡Œ / åˆ é™¤ç½‘æ ¼ä¸­é€‰ä¸­çš„è¡Œ |
| `Ctrl+Shift+E` | å¯¼å‡ºæŸ¥è¯¢ç»“æžœæˆ–æ•°æ®è¡¨ |
| `Esc` | å…³é—­å½“å‰å¼¹çª—æˆ–æµ®å±‚ |

å¯åœ¨ SQL ç¼–è¾‘å™¨å·¥å…·æ å¼€å¯ Vim å¯¼èˆªæ¨¡å¼ï¼ˆæ”¯æŒ `NORMAL`ã€`INSERT`ã€`VISUAL` åŠ `:` å‘½ä»¤ï¼‰ã€‚

---

## ä»Žæºç æž„å»º

### çŽ¯å¢ƒè¦æ±‚

- Windows 10 æˆ– Windows 11ï¼ˆx64ï¼‰
- é€šè¿‡ [rustup](https://rustup.rs/) å®‰è£…çš„ç¨³å®šç‰ˆ Rust MSVC å·¥å…·é“¾ï¼ˆ`x86_64-pc-windows-msvc`ï¼ŒRust `2024` Editionï¼‰
- Visual Studio C++ Build Tools ä¸Ž Windows SDKï¼ˆç”¨äºŽ Win32ã€Direct3D 11 ä¸Žå†…åµŒ C é©±åŠ¨ç¼–è¯‘ï¼‰
- Windows PowerShellï¼ˆä¸²è”å¤šæ¡å‘½ä»¤æ—¶è¯·ä½¿ç”¨ `;` è€Œéž `&&`ï¼‰

> [!CAUTION]
> è¯·å§‹ç»ˆä½¿ç”¨ `--release` è¿›è¡Œæž„å»ºå’Œæµ‹è¯•ã€‚é™¤éžä¸“é—¨è°ƒè¯•ç¬¦å·é—®é¢˜ï¼Œå¦åˆ™ä¸è¦å¯¹æ•´ä¸ªå·¥ä½œåŒºè¿è¡Œæœªé…ç½® profile çš„ debug ç‰ˆ `cargo build` æˆ– `cargo test`ï¼Œå› ä¸ºå…¨éƒ¨ crate ä¸Ž 9 ä¸ªé©±åŠ¨åœ¨ `target\debug` ä¸‹ç”Ÿæˆçš„ PDB æ–‡ä»¶ä¼šè¶…è¿‡ 140 GBã€‚å†…å­˜æœ‰é™çš„æœºå™¨è¯·é™„å¸¦ `-j 1`ã€‚

### æž„å»ºä¸Žæµ‹è¯•å‘½ä»¤

```powershell
# ç¡®è®¤æ²¡æœ‰æ­£åœ¨è¿è¡Œçš„ tableminus.exe å ç”¨ target\release\tableminus.exe
Get-Process tableminus -ErrorAction SilentlyContinue

# ä»¥ release æ¨¡å¼è¿›è¡Œç±»åž‹æ£€æŸ¥å’Œç¼–è¯‘
cargo check --release -j 1 -p tableminus-windows-app
cargo build --release -j 1 -p tableminus-windows-app

# è¿è¡Œæ— ç•Œé¢å¯åŠ¨æ£€æŸ¥å¹¶å¯åŠ¨åº”ç”¨
.\target\release\tableminus.exe --smoke-startup
.\target\release\tableminus.exe
```

Release æµ‹è¯•å¥—ä»¶ï¼š

```powershell
cargo test --release -j 1 -p tablepro-core filter
cargo test --release -j 1 -p tableminus-e2e --test startup_perf_e2e -- --nocapture
cargo test --release -j 1 -p tableminus-e2e --test sqlite_e2e -- --nocapture
```

### å·¥ä½œåŒºç»“æž„

| è·¯å¾„ | èŒè´£ |
| :--- | :--- |
| `core/crates/core` | `tablepro-core`ï¼šæŸ¥è¯¢æ¨¡åž‹ã€è¿‡æ»¤æ¡ä»¶ã€SQL æ–¹è¨€ä¸Žå¯¼å‡ºç¼–ç å™¨ |
| `core/crates/session` | `tablepro-session`ï¼šæ´»è·ƒä¼šè¯ã€è¿žæŽ¥æ± ä¸Žäº‹åŠ¡ç®¡ç† |
| `core/crates/cli` | `tablepro-cli`ï¼šCLI å­å‘½ä»¤ä¸Žæ— ç•Œé¢è¿è¡Œå™¨ |
| `core/crates/mcp` | `tablepro-mcp`ï¼šå¸¦ Token è®¤è¯çš„æœ¬åœ°å›žçŽ¯ MCP HTTP æœåŠ¡ |
| `core/crates/ai` | `tablepro-ai`ï¼šä»…ä¼  Schema çš„ Ollamaã€OpenAI ä¸Ž Anthropic å®¢æˆ·ç«¯ |
| `core/crates/ssh` | `tablepro-ssh`ï¼šåŸºäºŽ `russh` çš„è¿›ç¨‹å†… SSH éš§é“ |
| `core/crates/net` | `tablepro-net`ï¼š`rustls` TLS ä¸Žç³»ç»Ÿè¯ä¹¦æ ¡éªŒ |
| `core/crates/storage` | `tablepro-storage`ï¼šå·²ä¿å­˜è¿žæŽ¥ã€æŸ¥è¯¢åº“ä¸ŽåŽ†å²è®°å½• |
| `core/crates/foreign-import` | `tablepro-foreign-import`ï¼šä»Žå…¶ä»–æ•°æ®åº“å®¢æˆ·ç«¯å¯¼å…¥è¿žæŽ¥ |
| `core/crates/account` | `tablepro-account`ï¼šè´¦å·ä¸Žå‡­æ® trait |
| `core/crates/drivers/*` | 9 ä¸ªæ•°æ®åº“é©±åŠ¨ï¼ˆ`postgres`ã€`mysql`ã€`sqlite`ã€`duckdb`ã€`mssql`ã€`clickhouse`ã€`cassandra`ã€`mongodb`ã€`redis`ï¼‰ |
| `crates/tableminus-gfx` | çŸ¢é‡å›¾æ ‡ä¸Žæ•°æ®åº“å¼•æ“Žå›¾æ ‡ |
| `crates/tableminus-state` | ç¼–è¾‘å™¨çŠ¶æ€ã€è‡ªåŠ¨è¡¥å…¨ã€ç½‘æ ¼æ¨¡åž‹ã€æ ‡ç­¾é¡µä¸Ž 14 å¥—å†…ç½®ä¸»é¢˜ |
| `crates/tableminus-services` | å¼‚æ­¥ Tokio è¿è¡Œæ—¶ã€ä¼šè¯/AI/MCP è°ƒåº¦ä¸Žå¤šè¯­è¨€è¯Šæ–­ä¿¡æ¯ |
| `crates/tableminus-windows-platform` | è®¾ç½® JSON æŒä¹…åŒ–ã€Win32 å‡­æ®ç®¡ç†å™¨ã€DWM ä¸Žå‡å¼±åŠ¨æ€è¾…åŠ© |
| `crates/tableminus-windows-app` | HeroGPUIï¼ˆDirect3D 11ï¼‰æ¡Œé¢åº”ç”¨ã€Velopack æ›´æ–°å™¨ä¸Ž 6 ç§è¯­è¨€åŒ… |
| `crates/tableminus-e2e` | SQLiteã€DuckDBã€å¯åŠ¨è€—æ—¶ä¸Žç½‘æ ¼æ¸²æŸ“ç«¯åˆ°ç«¯åŸºå‡†æµ‹è¯• |

---

## å‚ä¸Žè´¡çŒ®ä¸Žå¼€æºåè®®

æ¬¢è¿Žæäº¤ Pull Requestã€é—®é¢˜åé¦ˆæˆ–ç¿»è¯‘ä¿®æ­£ã€‚æž„å»ºè§„èŒƒä¸Ž GPUI æ³¨æ„äº‹é¡¹è¯·å‚é˜… [CONTRIBUTING.md](CONTRIBUTING.md)ï¼Œç‰ˆæœ¬æ›´æ–°è®°å½•è§ [CHANGELOG.md](CHANGELOG.md)ï¼Œè¡Œä¸ºå‡†åˆ™è§ [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)ï¼Œå®‰å…¨æ¼æ´žè¯·æŒ‰ [SECURITY.md](SECURITY.md) æŒ‡å¼•ç§ä¸‹åé¦ˆã€‚

TableMinus é‡‡ç”¨ [LICENSE](LICENSE)ï¼ˆGNU Affero General Public License v3.0ï¼‰ä¸Ž [LICENSE-MIT](LICENSE-MIT)ï¼ˆMITï¼Œä¿ç•™å¯¹ä¸Šæ¸¸å†…æ ¸ [TableProApp/TablePro](https://github.com/TableProApp/TablePro) çš„ç½²åï¼‰åŒåè®®æŽˆæƒã€‚
