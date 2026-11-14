<div align="center">

<img src="crates/dataforge-windows-app/assets/dataforge_logo.png" alt="DataForge Logo" width="112" height="112" />

# DataForge

**Un banco de trabajo SQL y de bases de datos nativo, rápido y ligero, escrito en Rust para Windows.**

[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![GPUI](https://img.shields.io/badge/UI-HeroGPUI%20%2F%20Direct3D%2011-0078D4?style=flat-square&logo=windows&logoColor=white)](#por-qué-dataforge)
[![Drivers](https://img.shields.io/badge/Drivers-9%20Engines-10B981?style=flat-square)](#bases-de-datos-compatibles)
[![Memory](https://img.shields.io/badge/RAM-~55%20MB-F59E0B?style=flat-square)](#rendimiento-real)
[![Startup](https://img.shields.io/badge/Inicio-~10%20ms-6366F1?style=flat-square)](#rendimiento-real)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue?style=flat-square)](LICENSE-MIT)

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [Español](README.es.md) · [Français](README.fr.md)

</div>

---

## ¿Por qué DataForge?

La mayoría de los clientes de bases de datos de escritorio funcionan sobre Electron (consumiendo cientos de megabytes o más de 1 GB de RAM en reposo) o sobre JVM con arranques lentos.

DataForge está escrito íntegramente en **Rust** nativo con **HeroGPUI (Direct3D 11)** y desenfoque nativo de **Windows 11 (Gaussian Blur / Mica)**. Arranca en unos **10 ms**, mantiene el uso de memoria alrededor de **55 MB** incluso consultando tablas SQLite de **10 millones de filas**, e incluye autocompletado consciente del esquema, modo Vim, edición en línea y un asistente de IA local que nunca envía tus datos de filas.

---

## Capturas de pantalla

### Editor SQL y consulta sobre 10 millones de filas (`15 ms`)
![DataForge SQL Editor querying 10 million rows](docs/screenshots/sql-editor-10m.jpg)

### Cuadrícula de datos de alta densidad y filtros
![DataForge Data Grid view](docs/screenshots/data-grid.jpg)

### Asistente IA consciente del esquema
![DataForge Schema-Aware AI Assistant](docs/screenshots/ai-assistant.jpg)

---

## Bases de datos compatibles

| Motor | Crate del driver |
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

## Rendimiento real

| Prueba | Resultado observado |
| :--- | :--- |
| **Inicio en frío (`--smoke-startup`, 9/9 drivers)** | `9.21 – 10.11 ms` |
| **Benchmark SQLite 100k filas (`sqlite_100k_rows_load_and_scroll_perf`)** | `78.97 ms` seed · `24.43 ms` carga · `0.432 ms` scroll |
| **Consulta indexada en 10M de filas (`LIMIT 1000`)** | `5 – 15 ms` |
| **Escaneo completo de 10M de filas + JSON + 3 JOINs + Window Functions** | `40.01 s` |
| **Memoria RAM bajo carga de 100k / 500k / 1M filas** | `~55 MB` constantes |

---

## Compilar desde el código fuente

```powershell
cargo build --release -p dataforge-windows-app
.\target\release\dataforge.exe --smoke-startup
.\target\release\dataforge.exe
```

## Comunidad, Seguridad y Licencia

- **Código de conducta**: [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)
- **Política de seguridad**: [SECURITY.md](SECURITY.md)
- **Licencia**: [MIT License](LICENSE-MIT) (con atribución al núcleo de [TableProApp/TablePro](https://github.com/TableProApp/TablePro)) y [LICENSE](LICENSE).
