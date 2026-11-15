<div align="center">

<img src="crates/tableminus-windows-app/assets/tableminus_logo.png" alt="TableMinus Logo" width="112" height="112" />

# TableMinus

**Atelier SQL et bases de donnÃ©es natif en Rust pour Windows.**

[![CI](https://img.shields.io/github/actions/workflow/status/williamcachamwri/TableMinus/ci.yml?branch=main&style=flat-square&label=CI&logo=github)](https://github.com/williamcachamwri/TableMinus/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/actions/workflow/status/williamcachamwri/TableMinus/release.yml?style=flat-square&label=Release&logo=github)](https://github.com/williamcachamwri/TableMinus/actions/workflows/release.yml)
[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![GPUI](https://img.shields.io/badge/UI-HeroGPUI%20%2F%20Direct3D%2011-0078D4?style=flat-square&logo=windows&logoColor=white)](#prÃ©sentation)
[![Drivers](https://img.shields.io/badge/Drivers-9%20Engines-10B981?style=flat-square)](#bases-de-donnÃ©es-prises-en-charge)
[![Memory](https://img.shields.io/badge/RAM-~55%20Mo-F59E0B?style=flat-square)](#prÃ©sentation)
[![Startup](https://img.shields.io/badge/DÃ©marrage-~10%20ms-6366F1?style=flat-square)](#prÃ©sentation)
[![Windows](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-0078D4?style=flat-square&logo=windows11&logoColor=white)](#tÃ©lÃ©chargement-et-installation)
[![License: MIT / AGPL-3.0](https://img.shields.io/badge/License-MIT%20%2F%20AGPL--3.0-blue?style=flat-square)](LICENSE-MIT)

[English](README.md) Â· [Tiáº¿ng Viá»‡t](README.vi.md) Â· [ç®€ä½“ä¸­æ–‡](README.zh-CN.md) Â· [æ—¥æœ¬èªž](README.ja.md) Â· [EspaÃ±ol](README.es.md) Â· [FranÃ§ais](README.fr.md)

</div>

---

## PrÃ©sentation

TableMinus est un atelier SQL et gestionnaire de bases de donnÃ©es natif pour Windows Ã©crit en Rust. L'interface s'appuie sur [HeroGPUI](https://github.com/williamcachamwri/TableMinus) directement au-dessus de Direct3D 11 et rÃ©utilise le cÅ“ur Rust open source de [TablePro](https://github.com/TableProApp/TablePro), sans Electron ni JVM.

Sur Windows 11 avec une compilation release, le dÃ©marrage Ã  froid prend environ `10 ms` (mesurÃ© entre `9.21` et `10.11 ms` via `--smoke-startup` avec les 9 pilotes enregistrÃ©s) et la mÃ©moire du processus reste autour de `~55 Mo` lors de requÃªtes intensives sur `100k`, `500k` et `1M` lignes. Dans la suite SQLite `tableminus-e2e`, le chargement de `100k` lignes dans la grille prend `24.43 ms` et le dÃ©filement sur `10k` images prend `0.432 ms`.

---

## Captures d'Ã©cran

### Ã‰diteur SQL, graphiques en direct et rÃ©sultats de requÃªtes

![TableMinus SQL Editor and Live Charts](docs/screenshots/sql-editor-charts.png)

### Grille de donnÃ©es virtualisÃ©e, agrÃ©gats par colonne et filtres paramÃ©trÃ©s

![TableMinus Data Grid view](docs/screenshots/data-grid.png)

### Diagramme ER interactif, relations de clÃ©s Ã©trangÃ¨res et export SVG

![TableMinus ER Diagram Workbench](docs/screenshots/er-diagram.png)

### Assistant d'importation (CSV, JSON, XLSX) avec mappage automatique des types

![TableMinus Import Wizard](docs/screenshots/import-wizard.png)

### Comparaison de schÃ©mas et de donnÃ©es avec script de synchronisation

![TableMinus Schema and Data Compare](docs/screenshots/data-compare.png)

### GÃ©nÃ©rateur de donnÃ©es fictives avec graines dÃ©terministes et clÃ©s Ã©trangÃ¨res

![TableMinus Mock Data Generator](docs/screenshots/mock-generator.png)

### Transfert de donnÃ©es multi-moteurs avec aperÃ§u DDL en direct

![TableMinus Cross-Engine Data Transfer](docs/screenshots/data-transfer.png)

### Recherche de mÃ©tadonnÃ©es (Metadata Search)

![TableMinus Metadata Search](docs/screenshots/metadata-search.png)

### Volet IA limitÃ© au schÃ©ma (`Ctrl+Shift+A`)

![TableMinus Schema-Aware AI Assistant](docs/screenshots/ai-assistant.png)

---

## Bases de donnÃ©es prises en charge

TableMinus compile 9 pilotes directement dans `tableminus.exe`, sans pilote ODBC externe ni extension Ã  installer :

| Base de donnÃ©es | Crate du pilote | Notes |
| :--- | :--- | :--- |
| PostgreSQL | `tablepro-driver-postgres` | `tokio-postgres` + `deadpool`, `COPY IN`, inspection de schÃ©ma et DDL |
| MySQL / MariaDB | `tablepro-driver-mysql` | Dialecte SQL complet et mÃ©tadonnÃ©es du catalogue |
| SQLite | `tablepro-driver-sqlite` | Ouverture de fichiers `.sqlite` / `.db` locaux par dialogue, glisser-dÃ©poser ou CLI |
| DuckDB | `tablepro-driver-duckdb` | DuckDB 2.0 embarquÃ© avec `json` et `parquet` liÃ©s |
| Microsoft SQL Server | `tablepro-driver-mssql` | TDS 8.0 pur Rust (`tiberius`) avec TLS et lots multi-rÃ©sultats |
| ClickHouse | `tablepro-driver-clickhouse` | RequÃªtes HTTP/TLS via `hyper-rustls` |
| Cassandra / ScyllaDB | `tablepro-driver-cassandra` | Pilote CQL natif (`scylla`) sur `rustls` |
| MongoDB | `tablepro-driver-mongodb` | RequÃªtes document, mappage BSON 3 et rÃ©solution DNS seed list |
| Redis | `tablepro-driver-redis` | Inspection clÃ©-valeur et exÃ©cution de commandes sur TLS/TCP asynchrone |

Les connexions gÃ¨rent le tunnel SSH intÃ©grÃ© via `russh` (`tablepro-ssh` avec mot de passe, clÃ© privÃ©e, invite de phrase secrÃ¨te et vÃ©rification de clÃ© d'hÃ´te), le TLS via `rustls` et le magasin de certificats Windows (`Disable`, `Prefer`, `Require`, `VerifyCA`, `VerifyFull`, plus CA et certificat/clÃ© client personnalisÃ©s), les politiques Safe Mode (`Off`, `ConfirmWrites`, `ReadOnly`), ainsi que l'import de connexions depuis d'autres clients via `tablepro-foreign-import`.

---

## FonctionnalitÃ©s

### Ã‰diteur SQL et Grille de donnÃ©es

L'Ã©diteur propose la coloration syntaxique par dialecte (`PostgreSQL`, `MySQL`, `SQLite`, `MsSql`, etc.), l'autocomplÃ©tion des tables, colonnes, schÃ©mas, mots-clÃ©s et clauses SQL (`FROM`, `JOIN`, `INTO`, `UPDATE`), l'Ã©dition multi-curseur, une bibliothÃ¨que de requÃªtes enregistrÃ©es, un historique persistant et un mode Vim optionnel (`NORMAL`, `INSERT`, `VISUAL`, `COMMAND`).

Dans la grille, vous pouvez examiner chaque ligne dans le Row Inspector ancrÃ© (`Ctrl+I`), basculer entre les onglets Structure, Columns, Indexes, Foreign Keys et DDL, figer les premiÃ¨res colonnes jusqu'aux 2/3 de la largeur, masquer ou redimensionner les colonnes et empiler des filtres paramÃ©trÃ©s avec 11 opÃ©rateurs (`=`, `!=`, `Contains`, `StartsWith`, `EndsWith`, `<`, `<=`, `>`, `>=`, `IS NULL`, `IS NOT NULL`). Les modifications de cellules, ajouts de lignes (`Alt+Insert`) et suppressions (`Ctrl+Shift+Backspace`) restent en attente par lot jusqu'au `Commit` (`Ctrl+Shift+S`) ou au `Rollback` (`Ctrl+Shift+R`).

Les rÃ©sultats et tables s'exportent de maniÃ¨re atomique via des fichiers temporaires `.part` en CSV (`.csv`), JSON Array (`.json`), JSON Lines (`.jsonl`) ou SQL `INSERT` (`.sql`) avec `Ctrl+Shift+E`. `tablepro-core` embarque aussi des encodeurs TSV, XML et XLSX Ã  mÃ©moire constante.

### SÃ©curitÃ© et stockage local

Les mots de passe de bases de donnÃ©es, secrets SSH, clÃ©s API IA et jetons Bearer MCP sont enregistrÃ©s dans le Gestionnaire d'informations d'identification Windows (`CredWriteW` / `CredReadW`) et effacÃ©s de la RAM avec `zeroize`, sans jamais apparaÃ®tre en clair dans les fichiers JSON ou les journaux. En Safe Mode, une connexion peut Ãªtre rÃ©glÃ©e sur `ConfirmWrites` ou `ReadOnly` pour demander confirmation ou bloquer les instructions `UPDATE`, `DELETE` et `DROP`.

### Assistant IA et serveur MCP

Le volet IA (`Ctrl+Shift+A`) se connecte Ã  Ollama en local (`http://127.0.0.1:11434/v1`), Ã  tout point de terminaison compatible OpenAI, Ã  OpenAI ou Ã  Anthropic. Seules les mÃ©tadonnÃ©es du schÃ©ma (noms de tables, noms de colonnes, types et dialecte SQL) sont transmises au modÃ¨le ; les lignes de donnÃ©es restent en local.

Pour les agents de code et IDE externes, `tablepro-mcp` dÃ©marre un serveur MCP HTTP local sur `127.0.0.1` protÃ©gÃ© par un jeton Bearer dans le Gestionnaire d'informations d'identification Windows et une vÃ©rification du processus appelant. Il expose 11 outils :

- `list_connections`, `connect`, `list_databases`, `list_schemas`, `list_tables`, `describe_table`, `get_table_ddl`, `browse_table`, `count_rows`, `execute_query` et `export_data`

`execute_query` respecte les rÃ¨gles External Clients et Safe Mode de chaque connexion et demande une confirmation explicite dans TableMinus avant d'exÃ©cuter une requÃªte d'Ã©criture.

### ThÃ¨mes, matÃ©riaux de fenÃªtre et langues

TableMinus inclut 14 thÃ¨mes intÃ©grÃ©s (`Dark`, `Light`, `High Contrast`, `Gaussian Glass Dark`, `Gaussian Glass Light`, `Tokyo Night`, `Dracula`, `Catppuccin Mocha`, `One Dark Pro`, `Nord`, `Monokai Pro`, `Gruvbox Dark`, `GitHub Light` et `Catppuccin Latte`). Le thÃ¨me de l'interface et la palette syntaxique peuvent Ãªtre choisis sÃ©parÃ©ment, et vous pouvez importer un thÃ¨me couleur VS Code au format `.json`.

Sous Windows 11, la fenÃªtre principale prend en charge les matÃ©riaux DWM `Gaussian Blur`, `Mica`, `Mica Alt` et `Opaque`, tout en gardant les fenÃªtres superposÃ©es opaques (`1.0` alpha) pour prÃ©server la lisibilitÃ©. Le mode animations rÃ©duites suit `SPI_GETCLIENTAREAANIMATION` de Windows pour dÃ©sactiver les transitions des panneaux lorsque les animations systÃ¨me sont coupÃ©es. Tous les libellÃ©s, menus, dialogues, infobulles et diagnostics des pilotes sont traduits en 6 langues : `en-US`, `vi-VN`, `zh-CN`, `ja-JP`, `es-ES` et `fr-FR`.

### Utilisation en ligne de commande

`tableminus.exe` sert Ã  la fois de lanceur graphique et d'outil CLI (`tablepro-cli`). Passer des chemins `.sqlite`, `.db`, `.duckdb`, `.sql` ou des URL de connexion (`postgresql://...`) les ouvre dans l'instance en cours via Named Pipe IPC (ajoutez `-n` / `--new-window` pour ouvrir une nouvelle fenÃªtre).

Options principales :
- `--version` (`-V`) affiche la version et quitte.
- `--help` (`-h`) affiche l'aide et les options du CLI.
- `--smoke-startup` lance un test de dÃ©marrage sans interface et vÃ©rifie l'enregistrement des 9 pilotes.

Sous-commandes : `open`, `query`, `connections`, `databases`, `schemas`, `tables`, `describe`, `export`, `status` et `completion` (`bash`, `zsh`, `fish`), avec les options `--format`, `--read-only`, `--limit` et `--timeout`.

```powershell
# ExÃ©cuter une requÃªte en lecture seule au format JSON Lines
.\target\release\tableminus.exe query -c Staging --read-only --format jsonl "SELECT id, email FROM users LIMIT 10"

# Afficher le DDL CREATE TABLE d'une table
.\target\release\tableminus.exe describe -c Staging --ddl users
```

---

## TÃ©lÃ©chargement et installation

NÃ©cessite **Windows 10 ou Windows 11 (x64)**. Les binaires compilÃ©s sont disponibles sur [GitHub Releases](https://github.com/williamcachamwri/TableMinus/releases) :

- **Installateur (`TableMinus-win-Setup.exe`)** : s'installe dans `%LocalAppData%` avec raccourcis Menu DÃ©marrer et Bureau, sans Ã©lÃ©vation UAC. Utilise Velopack pour les mises Ã  jour diffÃ©rentielles en arriÃ¨re-plan, configurables dans **Settings -> About**.
- **Archive portable (`TableMinus-win-Portable.zip`)** : Ã  extraire dans n'importe quel dossier pour lancer `tableminus.exe` directement sans modifier l'Ã©tat de l'installateur.

Chaque version publie un fichier `SHA256SUMS.txt` vÃ©rifiable dans PowerShell :

```powershell
Get-FileHash .\TableMinus-win-Setup.exe -Algorithm SHA256
```

---

## Raccourcis clavier

| Raccourci | Action |
| :--- | :--- |
| `Ctrl+K` / `Ctrl+Shift+K` / `Ctrl+P` | Ouvrir la palette de commandes |
| `Ctrl+Enter` / `F5` | ExÃ©cuter la requÃªte SQL active |
| `Ctrl+Shift+Enter` | ExÃ©cuter la sÃ©lection SQL |
| `Shift+Esc` | Annuler la requÃªte en cours |
| `Ctrl+S` | Enregistrer la requÃªte dans la bibliothÃ¨que |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | Nouvel onglet / Fermer l'onglet / Rouvrir l'onglet fermÃ© |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Onglet suivant / prÃ©cÃ©dent |
| `Ctrl+F` | Activer le filtre du panneau d'objets |
| `Ctrl+B` | Afficher ou masquer le panneau d'objets |
| `Ctrl+I` | Afficher ou masquer le Row Inspector |
| `Ctrl+Shift+A` | Afficher ou masquer le volet IA |
| `Ctrl+Shift+O` | Ouvrir la bibliothÃ¨que de connexions |
| `Ctrl+,` | Ouvrir les ParamÃ¨tres |
| `Ctrl+Shift+M` | Changer de thÃ¨me |
| `Ctrl+Shift+S` / `Ctrl+Shift+R` | Valider (`Commit`) / Annuler (`Rollback`) les modifications de grille |
| `Alt+Insert` / `Ctrl+Shift+Backspace` | Ajouter une ligne / Supprimer les lignes sÃ©lectionnÃ©es |
| `Ctrl+Shift+E` | Exporter les rÃ©sultats ou la table |
| `Esc` | Fermer la fenÃªtre modale ou superposition active |

La navigation Vim (`NORMAL`, `INSERT`, `VISUAL` et commandes `:`) s'active depuis la barre d'outils de l'Ã©diteur SQL.

---

## Compiler depuis les sources

### PrÃ©requis

- Windows 10 ou Windows 11 (x64)
- ChaÃ®ne d'outils Rust MSVC stable (`x86_64-pc-windows-msvc`, Rust `2024` Edition) via [rustup](https://rustup.rs/)
- Visual Studio C++ Build Tools et Windows SDK (pour Win32, Direct3D 11 et les pilotes C embarquÃ©s)
- Windows PowerShell (utilisez `;` au lieu de `&&` pour enchaÃ®ner les commandes)

> [!CAUTION]
> Compilez et testez toujours avec `--release`. Ã‰vitez de lancer `cargo build` ou `cargo test` en mode debug sur l'espace de travail sans besoin prÃ©cis de symboles de dÃ©bogage : les fichiers PDB dans `target\debug` pour tous les crates et les 9 pilotes dÃ©passent 140 Go. Ajoutez `-j 1` sur les machines limitÃ©es en mÃ©moire.

### Commandes de compilation et de test

```powershell
# VÃ©rifier qu'aucun processus tableminus.exe ne verrouille target\release\tableminus.exe
Get-Process tableminus -ErrorAction SilentlyContinue

# VÃ©rification des types et compilation release
cargo check --release -j 1 -p tableminus-windows-app
cargo build --release -j 1 -p tableminus-windows-app

# VÃ©rification de dÃ©marrage sans interface et lancement
.\target\release\tableminus.exe --smoke-startup
.\target\release\tableminus.exe
```

Suites de tests release :

```powershell
cargo test --release -j 1 -p tablepro-core filter
cargo test --release -j 1 -p tableminus-e2e --test startup_perf_e2e -- --nocapture
cargo test --release -j 1 -p tableminus-e2e --test sqlite_e2e -- --nocapture
```

### Structure de l'espace de travail

| Chemin | RÃ´le |
| :--- | :--- |
| `core/crates/core` | `tablepro-core` : modÃ¨les de requÃªtes, filtres, dialectes SQL, export |
| `core/crates/session` | `tablepro-session` : sessions actives, pools de connexions, transactions |
| `core/crates/cli` | `tablepro-cli` : sous-commandes CLI et exÃ©cution sans interface |
| `core/crates/mcp` | `tablepro-mcp` : serveur MCP HTTP local authentifiÃ© par jeton |
| `core/crates/ai` | `tablepro-ai` : client Ollama, OpenAI et Anthropic limitÃ© au schÃ©ma |
| `core/crates/ssh` | `tablepro-ssh` : tunnel SSH intÃ©grÃ© via `russh` |
| `core/crates/net` | `tablepro-net` : TLS `rustls` et vÃ©rification des certificats systÃ¨me |
| `core/crates/storage` | `tablepro-storage` : connexions enregistrÃ©es, bibliothÃ¨que de requÃªtes, historique |
| `core/crates/foreign-import` | `tablepro-foreign-import` : import depuis d'autres clients de bases de donnÃ©es |
| `core/crates/account` | `tablepro-account` : traits de comptes et jetons |
| `core/crates/drivers/*` | 9 pilotes (`postgres`, `mysql`, `sqlite`, `duckdb`, `mssql`, `clickhouse`, `cassandra`, `mongodb`, `redis`) |
| `crates/tableminus-gfx` | Glyphes vectoriels et icÃ´nes des moteurs |
| `crates/tableminus-state` | Ã‰tat de l'Ã©diteur, autocomplÃ©tion, modÃ¨le de grille, onglets, 14 thÃ¨mes |
| `crates/tableminus-services` | Runtime Tokio asynchrone, orchestration session/IA/MCP, diagnostics |
| `crates/tableminus-windows-platform` | ParamÃ¨tres JSON, Gestionnaire d'informations d'identification Win32, DWM, animations rÃ©duites |
| `crates/tableminus-windows-app` | Application bureau HeroGPUI (Direct3D 11), mise Ã  jour Velopack, 6 langues |
| `crates/tableminus-e2e` | Benchmarks SQLite, DuckDB, dÃ©marrage et grille |

---

## Contributions et licence

Les contributions, signalements de bogues et corrections de traductions sont les bienvenus. Consultez [CONTRIBUTING.md](CONTRIBUTING.md) pour les rÃ¨gles de compilation et GPUI, [CHANGELOG.md](CHANGELOG.md) pour les notes de version, [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) et [SECURITY.md](SECURITY.md) pour signaler une vulnÃ©rabilitÃ© en privÃ©.

TableMinus est distribuÃ© sous [LICENSE](LICENSE) (GNU Affero General Public License v3.0) et [LICENSE-MIT](LICENSE-MIT) (MIT, avec attribution du cÅ“ur amont Ã  [TableProApp/TablePro](https://github.com/TableProApp/TablePro)).
