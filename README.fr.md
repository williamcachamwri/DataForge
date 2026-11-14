<div align="center">

<img src="crates/dataforge-windows-app/assets/dataforge_logo.png" alt="DataForge Logo" width="112" height="112" />

# DataForge

**Un atelier SQL et bases de données natif, rapide et économe en mémoire, écrit en Rust pour Windows.**

[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![GPUI](https://img.shields.io/badge/UI-HeroGPUI%20%2F%20Direct3D%2011-0078D4?style=flat-square&logo=windows&logoColor=white)](#pourquoi-dataforge)
[![Drivers](https://img.shields.io/badge/Drivers-9%20Engines-10B981?style=flat-square)](#bases-de-données-prises-en-charge)
[![Memory](https://img.shields.io/badge/RAM-~55%20Mo-F59E0B?style=flat-square)](#performances-réelles)
[![Startup](https://img.shields.io/badge/Démarrage-~10%20ms-6366F1?style=flat-square)](#performances-réelles)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue?style=flat-square)](LICENSE-MIT)

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [Español](README.es.md) · [Français](README.fr.md)

</div>

---

## Pourquoi DataForge ?

La plupart des clients de bases de données reposent sur Electron (consommant des centaines de Mo voire plus d'1 Go de RAM au repos) ou sur la JVM avec des temps de démarrage lents.

DataForge est entièrement écrit en **Rust** natif avec **HeroGPUI (Direct3D 11)** et les effets natifs de **Windows 11 (Flou gaussien / Mica)**. Il démarre en environ **10 ms**, reste autour de **55 Mo de RAM** même sur une table SQLite de **10 millions de lignes**, tout en offrant l'autocomplétion contextuelle, le mode Vim, l'édition en ligne et un assistant IA local respectueux de la confidentialité.

---

## Captures d'écran

### Éditeur SQL & requête sur 10 millions de lignes (`15 ms`)
![DataForge SQL Editor querying 10 million rows](docs/screenshots/sql-editor-10m.jpg)

### Grille de données haute densité & filtres
![DataForge Data Grid view](docs/screenshots/data-grid.jpg)

### Assistant IA conscient du schéma
![DataForge Schema-Aware AI Assistant](docs/screenshots/ai-assistant.jpg)

---

## Bases de données prises en charge

| Moteur | Crate du pilote |
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

## Performances réelles

| Benchmark | Résultat observé |
| :--- | :--- |
| **Démarrage à froid (`--smoke-startup`, 9/9 pilotes)** | `9.21 – 10.11 ms` |
| **Benchmark SQLite 100k lignes (`sqlite_100k_rows_load_and_scroll_perf`)** | `78.97 ms` seed · `24.43 ms` chargement · `0.432 ms` scroll |
| **Requête indexée sur 10M de lignes (`LIMIT 1000`)** | `5 – 15 ms` |
| **Scan complet 10M de lignes + JSON + 3 JOINs + Window Functions** | `40.01 s` |
| **Mémoire RAM sous charge de 100k / 500k / 1M lignes** | `~55 Mo` constants |

---

## Compiler depuis les sources

```powershell
cargo build --release -p dataforge-windows-app
.\target\release\dataforge.exe --smoke-startup
.\target\release\dataforge.exe
```

## Communauté, Sécurité & Licence

- **Code de conduite** : [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)
- **Politique de sécurité** : [SECURITY.md](SECURITY.md)
- **Licence** : [MIT License](LICENSE-MIT) (avec attribution au cœur [TableProApp/TablePro](https://github.com/TableProApp/TablePro)) ainsi que [LICENSE](LICENSE).
