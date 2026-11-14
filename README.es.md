<div align="center">

<img src="crates/dataforge-windows-app/assets/dataforge_logo.png" alt="DataForge Logo" width="112" height="112" />

# DataForge

**Un banco de trabajo SQL y de bases de datos nativo, rápido y ligero, escrito en Rust para Windows.**

[![CI](https://img.shields.io/github/actions/workflow/status/williamcachamwri/DataForge/ci.yml?branch=main&style=flat-square&label=CI&logo=github)](https://github.com/williamcachamwri/DataForge/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/actions/workflow/status/williamcachamwri/DataForge/release.yml?style=flat-square&label=Release&logo=github)](https://github.com/williamcachamwri/DataForge/actions/workflows/release.yml)
[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![GPUI](https://img.shields.io/badge/UI-HeroGPUI%20%2F%20Direct3D%2011-0078D4?style=flat-square&logo=windows&logoColor=white)](#por-qué-dataforge-existe)
[![Drivers](https://img.shields.io/badge/Drivers-9%20Engines-10B981?style=flat-square)](#motores-compatibles-y-seguridad-de-conexión)
[![Memory](https://img.shields.io/badge/RAM-~55%20MB-F59E0B?style=flat-square)](#rendimiento-real)
[![Startup](https://img.shields.io/badge/Inicio-~10%20ms-6366F1?style=flat-square)](#rendimiento-real)
[![Windows](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-0078D4?style=flat-square&logo=windows11&logoColor=white)](#instalación-y-actualizaciones-automáticas)
[![License: MIT / AGPL-3.0](https://img.shields.io/badge/License-MIT%20%2F%20AGPL--3.0-blue?style=flat-square)](LICENSE-MIT)

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [Español](README.es.md) · [Français](README.fr.md)

</div>

---

## Por qué DataForge existe

La mayoría de los clientes de bases de datos para escritorio te obligan a elegir entre dos concesiones incómodas: una aplicación Electron que consume un gigabyte de RAM sin hacer nada sobre una pestaña vacía, o una herramienta JVM que tarda cinco segundos en despertar antes de poder inspeccionar un simple archivo SQLite local.

DataForge toma otro camino. Inspirado en [TablePro](https://github.com/TableProApp/TablePro) y construido sobre su núcleo de bases de datos en Rust, DataForge sustituye los webviews multiplataforma por una interfaz nativa de Windows renderizada directamente sobre **Direct3D 11** mediante **HeroGPUI**, integrada con materiales de fondo **Windows 11 DWM** (**Gaussian Blur**, **Mica**, **Mica Alt** y **Opaque**).

La diferencia se nota al instante: el arranque en frío ronda los **10 ms**, el consumo de memoria se mantiene cerca de **~55 MB** incluso al consultar y desplazarse por tablas SQLite de **10 millones de filas**, y sigues teniendo todas las herramientas ergonómicas del día a día: autocompletado consciente del esquema, modo Vim opcional, edición en línea por lotes, guardias de Modo Seguro, servidor MCP local integrado y un asistente SQL con IA centrado en la privacidad.

---

## Recorrido visual

### Editor SQL de alta velocidad, modo Vim y resultados en vivo (`15 ms` en 10M filas)
*Desenfoque gaussiano de Windows 11 en toda la ventana, resaltado de sintaxis adaptado al dialecto, autocompletado consciente del esquema, edición modal Vim opcional (`NORMAL` / `INSERT` / `VISUAL` / `COMMAND`) y panel dividido de resultados en tiempo real.*

![DataForge SQL Editor querying 10 million rows](docs/screenshots/sql-editor-10m.jpg)

---

### Cuadrícula virtualizada, claves foráneas y filtros parametrizados
*Renderizado virtualizado a 60+ FPS con edición de celdas en línea, confirmación o reversión (`Commit` / `Rollback`) por lotes, columnas iniciales fijadas, control de visibilidad de columnas, filtros parametrizados multioperador, inspector de estructura/DDL/claves foráneas y exportación atómica.*

![DataForge Data Grid view](docs/screenshots/data-grid.jpg)

---

### Asistente IA solo de esquema (Privacidad primero)
*Panel lateral derecho no bloqueante de `320px` conectado a modelos locales de Ollama (p. ej. `llama3.2`) o endpoints en la nube con tu propia clave (BYOK). Solo los metadatos del esquema salen de tu equipo; las filas de tus tablas nunca abandonan tu máquina.*

![DataForge Schema-Aware AI Assistant](docs/screenshots/ai-assistant.jpg)

---

## Análisis detallado de funciones

### Motores compatibles y seguridad de conexión

DataForge compila **9 drivers nativos de bases de datos** directamente en el ejecutable—sin buscar controladores ODBC externos ni plugins en tiempo de ejecución:

| Motor | Crate del driver | Puntos destacados |
| :--- | :--- | :--- |
| **PostgreSQL** | `tablepro-driver-postgres` | `tokio-postgres` + `deadpool`, soporte de `COPY IN`, inspección de esquema/DDL |
| **MySQL / MariaDB** | `tablepro-driver-mysql` | Soporte completo de dialecto SQL y metadatos de catálogo |
| **SQLite** | `tablepro-driver-sqlite` | Archivos locales `.sqlite` / `.db` sin configuración, apertura directa desde CLI |
| **DuckDB** | `tablepro-driver-duckdb` | DuckDB 2.0 integrado con soporte nativo para `json` y `parquet` |
| **Microsoft SQL Server** | `tablepro-driver-mssql` | TDS 8.0 en Rust puro (`tiberius`) con TLS compatible con Windows y lotes múltiples |
| **ClickHouse** | `tablepro-driver-clickhouse` | Consultas analíticas nativas HTTP/TLS mediante `hyper-rustls` |
| **Cassandra / ScyllaDB** | `tablepro-driver-cassandra` | Driver CQL nativo (`scylla`) con transporte `rustls` |
| **MongoDB** | `tablepro-driver-mongodb` | Consultas documentales, mapeo BSON 3 y resolución DNS seed list |
| **Redis** | `tablepro-driver-redis` | Inspección clave-valor y ejecución de comandos sobre TLS/TCP asíncrono |

La seguridad forma parte de la capa de conexión desde el principio:

- **Túnel SSH en proceso**: Cliente SSH `russh` integrado (`tablepro-ssh`) con autenticación por contraseña o clave privada, solicitud interactiva de frase de paso y diálogo de verificación de clave de host—sin depender de `ssh.exe` externo.
- **TLS y almacén de certificados de Windows**: Impulsado por `rustls` (`aws-lc-rs`) y `rustls-platform-verifier`, con modos `Disable`, `Prefer`, `Require`, `VerifyCA` y `VerifyFull`, además de archivos CA, certificado de cliente y clave de cliente personalizados.
- **Bóveda en el Administrador de credenciales de Windows**: Contraseñas de bases de datos, secretos SSH, claves de API de IA y tokens MCP se guardan en **Windows Credential Manager** (`CredWriteW` / `CredReadW` con limpieza de memoria `zeroize`)—nunca en texto plano en archivos de configuración ni registros.
- **Guardias de Modo Seguro (Safe Mode)**: Configura conexiones con protección `Off`, `ConfirmWrites` o `ReadOnly` para exigir confirmación explícita o bloquear sentencias `UPDATE`, `DELETE` o `DROP`.
- **Importación de conexiones externas**: Migra conexiones y consultas guardadas desde otros clientes mediante `tablepro-foreign-import`.

---

### Editor SQL y cuadrícula de alto volumen

- **Editor SQL consciente del dialecto**:
  - Resaltado de sintaxis en tiempo real por dialecto (`PostgreSQL`, `MySQL`, `SQLite`, `MsSql`, etc.) con paletas independientes para la interfaz y el código.
  - Autocompletado contextual para tablas, columnas, esquemas, palabras clave SQL y fragmentos (`FROM`, `JOIN`, `INTO`, `UPDATE`, etc.).
  - Edición multicursor, **modo Vim** opcional (`Normal`, `Insert`, `Visual`, `Command`), biblioteca de consultas guardadas e historial persistente.
  - Ejecuta todo el editor (`Ctrl+Enter` / `F5`), solo el texto seleccionado (`Ctrl+Shift+Enter`) o cancela una consulta en curso (`Shift+Esc`).
- **Cuadrícula virtualizada interactiva**:
  - Desplazamiento fluido sobre grandes conjuntos de datos con **Row Inspector** acoplado (`Ctrl+I`) y pestañas de **Estructura / Columnas / Índices / Claves foráneas / DDL**.
  - **Edición en línea**: Edita celdas, inserta filas (`Alt+Insert`) o marca filas para eliminar (`Ctrl+Shift+Backspace`) en un lote provisional—revisa los cambios antes de hacer **Commit** (`Ctrl+Shift+S`) o **Rollback** (`Ctrl+Shift+R`).
  - **Columnas iniciales fijadas**: Fija columnas iniciales (hasta 2/3 del ancho del visor), alterna la visibilidad de columnas y redimensiona cabeceras arrastrando.
  - **Filtros parametrizados por columna**: Combina reglas con 11 operadores seguros según dialecto (`=`, `!=`, `Contains`, `StartsWith`, `EndsWith`, `<`, `<=`, `>`, `>=`, `IS NULL`, `IS NOT NULL`).
  - **Exportación atómica**: Exporta resultados o tablas a **CSV** (`.csv`), **JSON Array** (`.json`), **JSON Lines** (`.jsonl`) o **SQL `INSERT`** (`.sql`) desde el diálogo Exportar (`Ctrl+Shift+E`), escribiendo mediante archivos temporales `.part` (`tablepro-core` también incluye codificadores TSV, XML y XLSX).

---

### Apariencia, fondos Windows DWM e internacionalización

- **14 temas integrados + importación de VS Code**:
  - Incluye **14 preajustes**: `Dark`, `Light`, `High Contrast`, `Gaussian Glass Dark`, `Gaussian Glass Light`, `Tokyo Night`, `Dracula`, `Catppuccin Mocha`, `One Dark Pro`, `Nord`, `Monokai Pro`, `Gruvbox Dark`, `GitHub Light` y `Catppuccin Latte`.
  - Combina libremente tu **Tema de interfaz** y tu **Paleta de sintaxis**, o importa cualquier **tema `.json` de VS Code** (con validación de tamaño, profundidad y escape de rutas).
- **Fondos nativos Windows 11 DWM**: Elige entre **Gaussian Blur**, **Mica**, **Mica Alt** y **Opaque**, manteniendo las capas flotantes con opacidad `1.0` para conservar una lectura nítida.
- **Accesibilidad con movimiento reducido**: Detecta automáticamente `SPI_GETCLIENTAREAANIMATION` en Windows para que paneles y modales respondan en `0 ms` cuando las animaciones del sistema están desactivadas.
- **6 idiomas en interfaz y diagnósticos**:
  - `en-US` (English) · `vi-VN` (Tiếng Việt) · `zh-CN` (简体中文) · `ja-JP` (日本語) · `es-ES` (Español) · `fr-FR` (Français)

---

### Asistente IA local-first y servidor MCP integrado

- **Panel de IA privado (`Ctrl+Shift+A`)**:
  - **Garantía de solo esquema**: Únicamente se envía el contexto del esquema (nombres de tablas, columnas, tipos y lenguaje de consulta). Los datos de las filas jamás se transmiten.
  - **Proveedores locales y BYOK**: Funciona directamente con **Ollama** local (`http://127.0.0.1:11434/v1`, p. ej. `llama3.2`) o cualquier endpoint **compatible con OpenAI**, además de **OpenAI** y **Anthropic** con claves guardadas en Windows Credential Manager.
  - Respuestas en streaming en tiempo real con bloques SQL resaltados listos para copiar.
- **Servidor MCP local integrado (`tablepro-mcp`)**:
  - Expone tus conexiones abiertas en DataForge a IDEs o agentes de programación mediante un servidor HTTP loopback (`127.0.0.1`), protegido por un token Bearer en Windows Credential Manager.
  - Ofrece **11 herramientas MCP estructuradas**: `list_connections`, `connect`, `list_databases`, `list_schemas`, `list_tables`, `describe_table`, `get_table_ddl`, `browse_table`, `count_rows`, `execute_query` y `export_data`.
  - `execute_query` respeta las políticas de External Clients y Modo Seguro y solicita aprobación explícita dentro de DataForge antes de ejecutar cualquier escritura.

---

### CLI y flujos sin interfaz

`dataforge.exe` funciona tanto como lanzador gráfico como CLI automatizable (`tablepro-cli`):

- **Opciones de aplicación**:
  - `dataforge.exe --version` (`-V`) — Imprime la versión y sale.
  - `dataforge.exe --help` (`-h`) — Muestra la ayuda de uso.
  - `dataforge.exe --smoke-startup` — Ejecuta una comprobación de arranque sin interfaz y registro de los `9/9` drivers.
- **Apertura directa y reenvío a instancia única**:
  - Pasa rutas `.sqlite` / `.db` / `.duckdb` / `.sql` o URLs de conexión (`postgresql://...`) directamente a `dataforge.exe` para abrirlas en la ventana activa mediante Named Pipe IPC (o añade `-n` / `--new-window`).
- **Subcomandos CLI**:
  - `open`, `query`, `connections`, `databases`, `schemas`, `tables`, `describe`, `export`, `status` y `completion` (`bash`, `zsh`, `fish`).
  - Admite `--format` (`table`, `csv`, `tsv`, `json`, `jsonl` para `query`; `csv`, `json`, `sql` para `export`), `--read-only`, `--limit` y `--timeout`.

```powershell
# Ejecutar una consulta de solo lectura desde PowerShell en formato JSON Lines
.\target\release\dataforge.exe query -c Staging --read-only --format jsonl "SELECT id, email FROM users LIMIT 10"

# Inspeccionar el DDL CREATE TABLE de una tabla
.\target\release\dataforge.exe describe -c Staging --ddl users
```

---

## Rendimiento real

Medido en Windows 11 con una compilación Release de `dataforge.exe` (`--release`):

| Prueba | Resultado observado |
| :--- | :--- |
| **Inicio en frío (`--smoke-startup`, 9/9 drivers)** | `9.21 – 10.11 ms` |
| **Inserción E2E de 100k filas SQLite (`sqlite_100k_rows_load_and_scroll_perf`)** | `78.97 ms` |
| **Carga de cuadrícula 100k filas + scroll de 10k fotogramas** | `24.43 ms` carga · `0.432 ms` scroll (`0.043 µs/frame`) |
| **Consulta indexada en 10M de filas (`ORDER BY event_ts DESC LIMIT 1000`)** | `5 – 15 ms` |
| **Escaneo completo de 10M de filas + JSON + 3 JOINs + 3 Window Functions** | `40.01 s` |
| **Memoria RAM durante pruebas de estrés de 100k / 500k / 1M filas** | `~55 MB` constantes |

---

## Instalación y actualizaciones automáticas

Descarga los binarios precompilados para Windows x64 desde [**GitHub Releases**](https://github.com/williamcachamwri/DataForge/releases):

- **Instalador (Recomendado) — `DataForge-win-Setup.exe`**:
  - Se instala limpiamente en `%LocalAppData%` con accesos directos en el Menú Inicio y escritorio—**sin requerir permisos de administrador UAC**.
  - Utiliza **Velopack** para actualizaciones delta silenciosas y ligeras. Comprueba actualizaciones automáticamente al iniciar (configurable en **Settings → About**) y permite buscar, descargar y reiniciar para aplicar mejoras desde **Settings → About**.
- **Versión portable — `DataForge-win-Portable.zip`**:
  - Descomprime en cualquier carpeta y ejecuta `dataforge.exe` directamente en modo independiente sin modificar el estado del instalador.
- **Verificación de integridad**:
  - Cada lanzamiento incluye `SHA256SUMS.txt` para comprobar los archivos en PowerShell:

```powershell
Get-FileHash .\DataForge-win-Setup.exe -Algorithm SHA256
```

---

## Atajos de teclado

| Atajo | Acción |
| :--- | :--- |
| `Ctrl+K` / `Ctrl+Shift+K` / `Ctrl+P` | Abrir paleta de comandos |
| `Ctrl+Enter` / `F5` | Ejecutar consulta SQL activa |
| `Ctrl+Shift+Enter` | Ejecutar el texto SQL seleccionado |
| `Shift+Esc` | Cancelar consulta en ejecución |
| `Ctrl+S` | Guardar consulta en la biblioteca |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | Nueva pestaña / Cerrar pestaña / Reabrir pestaña cerrada |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Pestaña siguiente / anterior |
| `Ctrl+F` | Enfocar filtro del panel lateral |
| `Ctrl+B` | Mostrar/ocultar barra lateral de objetos |
| `Ctrl+I` | Mostrar/ocultar Row Inspector |
| `Ctrl+Shift+A` | Mostrar/ocultar panel de Asistente IA |
| `Ctrl+Shift+O` | Abrir biblioteca de conexiones |
| `Ctrl+,` | Abrir Configuración |
| `Ctrl+Shift+M` | Alternar / cambiar tema |
| `Ctrl+Shift+S` / `Ctrl+Shift+R` | Confirmar (`Commit`) / Revertir (`Rollback`) ediciones en cuadrícula |
| `Alt+Insert` / `Ctrl+Shift+Backspace` | Añadir fila / Eliminar filas seleccionadas |
| `Ctrl+Shift+E` | Exportar resultados o tabla |
| `Esc` | Cerrar modal o capa superpuesta activa |

*Consejo: Activa el **modo Vim** en la barra del editor SQL para navegar en modos `NORMAL`, `INSERT`, `VISUAL` y comandos `:`.*

---

## Compilar desde el código fuente en Windows

### Requisitos previos

- **Sistema operativo**: Windows 10 o Windows 11 (x64)
- **Toolchain de Rust**: Toolchain MSVC estable (`x86_64-pc-windows-msvc`, Rust `2024` Edition) vía [rustup](https://rustup.rs/)
- **Visual Studio C++ Build Tools & Windows SDK**: Necesarios para Win32, Direct3D 11 (`HeroGPUI`) y los drivers en C incluidos
- **Terminal**: Windows PowerShell (usa `;` en lugar de `&&` para encadenar comandos)

> [!CAUTION]
> **Compila y prueba siempre con `--release`**: No ejecutes `cargo build` ni `cargo test` en modo debug sin perfilar sobre todo el workspace salvo que estés depurando símbolos específicamente. Los archivos PDB de depuración para todo el workspace y los 9 drivers en `target\debug` pueden superar los **140 GB**. Usa siempre `--release` (y `-j 1` en equipos con poca RAM).

### Verificar, compilar y ejecutar

```powershell
# Comprobar que no haya un proceso dataforge.exe bloqueando el binario
Get-Process dataforge -ErrorAction SilentlyContinue

# Comprobación de tipos en modo release
cargo check --release -j 1 -p dataforge-windows-app

# Compilar binario release
cargo build --release -j 1 -p dataforge-windows-app

# Verificar arranque en frío y registro de los 9/9 drivers
.\target\release\dataforge.exe --smoke-startup

# Iniciar DataForge
.\target\release\dataforge.exe
```

### Ejecutar pruebas en Release

```powershell
# Pruebas unitarias de filtros y dialectos SQL
cargo test --release -j 1 -p tablepro-core filter

# Pruebas de rendimiento de arranque y paridad de 6 idiomas
cargo test --release -j 1 -p dataforge-e2e --test startup_perf_e2e -- --nocapture

# Benchmark E2E de carga de 100k filas SQLite y scroll de 10k fotogramas
cargo test --release -j 1 -p dataforge-e2e --test sqlite_e2e -- --nocapture
```

---

## Arquitectura y mapa del workspace

```text
DataForge/
├── core/                             # Núcleo de bases de datos TablePro y 9 drivers nativos
│   └── crates/
│       ├── core/                     # tablepro-core: modelos de consulta, filtros, dialectos, exportación
│       ├── session/                  # tablepro-session: sesiones activas, pools, transacciones
│       ├── cli/                      # tablepro-cli: subcomandos de shell y ejecución sin UI
│       ├── mcp/                      # tablepro-mcp: servidor MCP HTTP local protegido por token
│       ├── ai/                       # tablepro-ai: cliente solo-esquema Ollama / OpenAI / Anthropic
│       ├── ssh/                      # tablepro-ssh: túnel SSH en proceso con russh
│       ├── net/                      # tablepro-net: rustls TLS + verificación de certificados del sistema
│       ├── storage/                  # tablepro-storage: conexiones guardadas, biblioteca e historial
│       ├── foreign-import/           # tablepro-foreign-import: importación desde otros clientes
│       ├── account/                  # tablepro-account: traits de cuenta y tokens
│       └── drivers/                  # postgres, mysql, sqlite, duckdb, mssql, clickhouse,
│                                     # cassandra, mongodb, redis
└── crates/
    ├── dataforge-gfx/                # Glifos vectoriales e iconos de motores
    ├── dataforge-state/              # Estado del editor, autocompletado, cuadrícula, pestañas, 14 temas
    ├── dataforge-services/           # Runtime Tokio asíncrono, orquestación sesión/IA/MCP y diagnósticos
    ├── dataforge-windows-platform/   # Ajustes JSON, Windows Credential Manager, DWM y movimiento reducido
    ├── dataforge-windows-app/        # App HeroGPUI (Direct3D 11), actualizador Velopack, 6 idiomas
    └── dataforge-e2e/                # Benchmarks E2E de SQLite, DuckDB, arranque y cuadrícula
```

---

## Contribución, Seguridad, Código de Conducta y Licencia

- **Contribuir**: Consulta [CONTRIBUTING.md](CONTRIBUTING.md) para ver notas de arquitectura, reglas de compilación `--release`, guías de superposición HeroGPUI y convenciones de localización en 6 idiomas.
- **Historial de cambios**: Consulta [CHANGELOG.md](CHANGELOG.md).
- **Código de conducta**: Consulta [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
- **Política de seguridad**: Consulta [SECURITY.md](SECURITY.md) para reportar vulnerabilidades en privado y conocer las garantías de privacidad local.
- **Licencia**: DataForge se distribuye bajo [LICENSE](LICENSE) (GNU Affero General Public License v3.0) y [LICENSE-MIT](LICENSE-MIT), con atribución al núcleo de [TableProApp/TablePro](https://github.com/TableProApp/TablePro) (MIT).
