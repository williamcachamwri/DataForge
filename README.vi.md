<div align="center">

<img src="crates/dataforge-windows-app/assets/dataforge_logo.png" alt="DataForge Logo" width="112" height="112" />

# DataForge

**Trình quản lý cơ sở dữ liệu & SQL Workbench thuần Rust, siêu nhẹ và mượt trên Windows.**

[![CI](https://img.shields.io/github/actions/workflow/status/williamcachamwri/DataForge/ci.yml?branch=main&style=flat-square&label=CI&logo=github)](https://github.com/williamcachamwri/DataForge/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/actions/workflow/status/williamcachamwri/DataForge/release.yml?style=flat-square&label=Release&logo=github)](https://github.com/williamcachamwri/DataForge/actions/workflows/release.yml)
[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![GPUI](https://img.shields.io/badge/UI-HeroGPUI%20%2F%20Direct3D%2011-0078D4?style=flat-square&logo=windows&logoColor=white)](#vì-sao-dataforge-ra-đời)
[![Drivers](https://img.shields.io/badge/Drivers-9%20Engines-10B981?style=flat-square)](#hệ-quản-trị-csdl--bảo-mật-kết-nối)
[![Memory](https://img.shields.io/badge/RAM-~55%20MB%20khi%20tải-F59E0B?style=flat-square)](#hiệu-năng-thực-tế)
[![Startup](https://img.shields.io/badge/Khởi%20động-~10%20ms-6366F1?style=flat-square)](#hiệu-năng-thực-tế)
[![Windows](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-0078D4?style=flat-square&logo=windows11&logoColor=white)](#cài-đặt--tự-động-cập-nhật)
[![License: MIT / AGPL-3.0](https://img.shields.io/badge/License-MIT%20%2F%20AGPL--3.0-blue?style=flat-square)](LICENSE-MIT)

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [Español](README.es.md) · [Français](README.fr.md)

</div>

---

## Vì sao DataForge ra đời

Phần lớn phần mềm quản lý cơ sở dữ liệu trên desktop buộc bạn phải đánh đổi: hoặc dùng ứng dụng Electron ngốn cả gigabyte RAM chỉ để mở một tab trống, hoặc dùng ứng dụng JVM mất vài giây khởi động trước khi mở được một tệp SQLite nội bộ.

DataForge chọn hướng đi khác. Lấy cảm hứng từ [TablePro](https://github.com/TableProApp/TablePro) và kế thừa lõi database bằng Rust của dự án này, DataForge thay thế hoàn toàn lớp webview bằng kiến trúc Windows thuần dựng trên **Direct3D 11** qua **HeroGPUI**, kết hợp vật liệu nền **Windows 11 DWM** (**Gaussian Blur**, **Mica**, **Mica Alt** và **Opaque**).

Trải nghiệm thực tế phản hồi gần như tức thì: khởi động lạnh quanh **10 ms**, mức RAM ổn định khoảng **~55 MB** ngay cả khi cuộn và truy vấn bảng SQLite **10 triệu dòng**, nhưng vẫn đầy đủ công cụ làm việc hiện đại—autocomplete hiểu schema, phím tắt Vim, chỉnh sửa trực tiếp trên lưới theo lô, cơ chế bảo vệ Safe Mode, máy chủ MCP tích hợp và trợ lý AI SQL ưu tiên quyền riêng tư.

---

## Khám phá giao diện

### SQL Editor tốc độ cao, Vim Mode & Kết quả trực tiếp (`15 ms` trên 10 triệu dòng)
*Hiệu ứng Windows 11 Gaussian Blur toàn cửa sổ, tô sáng cú pháp theo dialect, autocomplete theo schema, tùy chọn soạn thảo kiểu Vim (`NORMAL` / `INSERT` / `VISUAL` / `COMMAND`) và khung kết quả chia đôi trực tiếp.*

![DataForge SQL Editor querying 10 million rows](docs/screenshots/sql-editor-10m.jpg)

---

### Data Grid ảo hóa, Khóa ngoại & Bộ lọc tham số hóa
*Hiển thị bảng ảo hóa mượt 60+ FPS với chỉnh sửa ô trực tiếp, `Commit` / `Rollback` theo lô, cố định cột đầu, bật/tắt cột, bộ lọc đa toán tử tham số hóa, trình xem cấu trúc/DDL/khóa ngoại và xuất tệp nguyên tử.*

![DataForge Data Grid view](docs/screenshots/data-grid.jpg)

---

### Trợ lý AI chỉ đọc Schema (Ưu tiên bảo mật)
*Ngăn AI `320px` bên phải chạy bất đồng bộ, kết nối với Ollama nội bộ (ví dụ `llama3.2`) hoặc khóa API riêng (BYOK). Chỉ metadata của schema được gửi đi—dữ liệu từng dòng tuyệt đối không rời khỏi máy của bạn.*

![DataForge Schema-Aware AI Assistant](docs/screenshots/ai-assistant.jpg)

---

## Chi tiết tính năng

### Hệ quản trị CSDL & Bảo mật kết nối

DataForge biên dịch sẵn **9 driver cơ sở dữ liệu thuần Rust** ngay trong tệp thực thi—không cần cài thêm ODBC driver hay plugin ngoài:

| Hệ quản trị (Engine) | Driver Crate | Điểm nổi bật |
| :--- | :--- | :--- |
| **PostgreSQL** | `tablepro-driver-postgres` | `tokio-postgres` + `deadpool`, hỗ trợ `COPY IN`, xem schema/DDL |
| **MySQL / MariaDB** | `tablepro-driver-mysql` | Hỗ trợ đầy đủ dialect SQL và metadata danh mục |
| **SQLite** | `tablepro-driver-sqlite` | Mở trực tiếp tệp `.sqlite` / `.db` không cần cấu hình |
| **DuckDB** | `tablepro-driver-duckdb` | Tích hợp sẵn DuckDB 2.0 hỗ trợ `json` và `parquet` |
| **Microsoft SQL Server** | `tablepro-driver-mssql` | TDS 8.0 thuần Rust (`tiberius`), hỗ trợ TLS trên Windows và batch nhiều kết quả |
| **ClickHouse** | `tablepro-driver-clickhouse` | Truy vấn phân tích qua HTTP/TLS với `hyper-rustls` |
| **Cassandra / ScyllaDB** | `tablepro-driver-cassandra` | Driver CQL thuần (`scylla`) qua lớp truyền tải `rustls` |
| **MongoDB** | `tablepro-driver-mongodb` | Truy vấn tài liệu, ánh xạ BSON 3 và phân giải DNS seed list |
| **Redis** | `tablepro-driver-redis` | Kiểm tra key-value và chạy lệnh qua TLS/TCP bất đồng bộ |

Bảo mật được tích hợp ngay từ tầng kết nối:

- **SSH Tunnel nội tuyến**: Tích hợp client SSH `russh` (`tablepro-ssh`) hỗ trợ mật khẩu, khóa riêng tư (có hộp thoại nhập passphrase) và xác minh host-key ngay trong ứng dụng—không phụ thuộc `ssh.exe`.
- **TLS & Kho chứng chỉ hệ thống Windows**: Sử dụng `rustls` (`aws-lc-rs`) và `rustls-platform-verifier`, hỗ trợ các chế độ `Disable`, `Prefer`, `Require`, `VerifyCA` và `VerifyFull` cùng tệp CA, client cert và client key tùy chỉnh.
- **Windows Credential Manager**: Mật khẩu CSDL, mật khẩu/passphrase SSH, khóa API AI và token MCP được lưu trong **Windows Credential Manager** (`CredWriteW` / `CredReadW` kèm xóa vùng nhớ bằng `zeroize`)—không bao giờ lưu dạng văn bản thuần trong tệp cấu hình hay log.
- **Safe Mode bảo vệ ghi dữ liệu**: Tùy chọn `Off`, `ConfirmWrites` hoặc `ReadOnly` để tránh chạy nhầm `UPDATE`, `DELETE` hay `DROP` nếu chưa xác nhận.
- **Nhập kết nối từ ứng dụng khác**: Hỗ trợ nhập cấu hình kết nối và truy vấn đã lưu qua `tablepro-foreign-import`.

---

### SQL Editor & Data Grid khối lượng lớn

- **SQL Editor hiểu dialect**:
  - Tô sáng cú pháp theo từng hệ quản trị (`PostgreSQL`, `MySQL`, `SQLite`, `MsSql`, v.v.) với bảng màu giao diện và bảng màu code tách biệt.
  - Gợi ý tự động theo ngữ cảnh cho bảng, cột, schema, từ khóa SQL và snippet (`FROM`, `JOIN`, `INTO`, `UPDATE`, v.v.).
  - Hỗ trợ đa con trỏ, tùy chọn **Vim mode** (`Normal`, `Insert`, `Visual`, `Command`), thư viện truy vấn đã lưu và lịch sử truy vấn.
  - Chạy toàn bộ truy vấn (`Ctrl+Enter` / `F5`), chạy phần đang bôi đen (`Ctrl+Shift+Enter`), hoặc hủy truy vấn đang chạy (`Shift+Esc`).
- **Data Grid ảo hóa tương tác**:
  - Cuộn ảo hóa mượt mà với **Row Inspector** gắn cạnh (`Ctrl+I`) và các tab xem **Cấu trúc / Cột / Chỉ mục / Khóa ngoại / DDL**.
  - **Chỉnh sửa trực tiếp trên ô**: Sửa giá trị, thêm dòng (`Alt+Insert`) hoặc đánh dấu xóa dòng (`Ctrl+Shift+Backspace`) theo lô tạm—xem trước thay đổi rồi bấm **Commit** (`Ctrl+Shift+S`) hoặc **Rollback** (`Ctrl+Shift+R`).
  - **Cố định cột đầu (Freeze Columns)**: Ghim các cột đầu tiên (tối đa 2/3 chiều rộng khung nhìn), ẩn/hiện cột và kéo đổi độ rộng cột.
  - **Bộ lọc cột tham số hóa**: Kết hợp điều kiện lọc với 11 toán tử an toàn theo dialect (`=`, `!=`, `Contains`, `StartsWith`, `EndsWith`, `<`, `<=`, `>`, `>=`, `IS NULL`, `IS NOT NULL`).
  - **Xuất dữ liệu nguyên tử**: Xuất kết quả hoặc bảng sang **CSV** (`.csv`), **JSON Array** (`.json`), **JSON Lines** (`.jsonl`) hoặc **SQL `INSERT`** (`.sql`) qua hộp thoại Export (`Ctrl+Shift+E`), ghi qua tệp tạm `.part` rồi đổi tên (`tablepro-core` còn hỗ trợ bộ mã hóa TSV, XML và XLSX).

---

### Giao diện, Windows DWM Backdrops & Đa ngôn ngữ

- **14 giao diện tích hợp sẵn + Nhập theme VS Code**:
  - Bao gồm **14 preset có sẵn**: `Dark`, `Light`, `High Contrast`, `Gaussian Glass Dark`, `Gaussian Glass Light`, `Tokyo Night`, `Dracula`, `Catppuccin Mocha`, `One Dark Pro`, `Nord`, `Monokai Pro`, `Gruvbox Dark`, `GitHub Light` và `Catppuccin Latte`.
  - Phối độc lập **Theme giao diện** và **Bảng màu cú pháp SQL**, hoặc nhập tệp theme **VS Code `.json`** (có kiểm tra giới hạn kích thước và chống thoát thư mục).
- **Vật liệu nền Windows 11 DWM**: Hỗ trợ **Gaussian Blur (Frosted Glass)**, **Mica**, **Mica Alt** và **Opaque**, trong khi các hộp thoại/menu nổi giữ độ đục `1.0` alpha để chữ luôn sắc nét.
- **Giảm chuyển động (Reduced Motion)**: Tự động đọc `SPI_GETCLIENTAREAANIMATION` của Windows để chuyển ngăn kéo và hộp thoại về `0 ms` khi người dùng tắt hiệu ứng hệ thống.
- **6 ngôn ngữ giao diện & thông báo chẩn đoán**:
  - `en-US` (English) · `vi-VN` (Tiếng Việt) · `zh-CN` (简体中文) · `ja-JP` (日本語) · `es-ES` (Español) · `fr-FR` (Français)

---

### Trợ lý AI nội bộ & Máy chủ MCP tích hợp

- **Ngăn AI ưu tiên riêng tư (`Ctrl+Shift+A`)**:
  - **Cam kết chỉ gửi Schema**: Chỉ ngữ cảnh schema (tên bảng, tên cột, kiểu dữ liệu, dialect) được gửi tới mô hình AI. Dữ liệu bản ghi không bao giờ bị gửi ra ngoài.
  - **Hỗ trợ mô hình nội bộ & BYOK**: Kết nối sẵn với **Ollama** cục bộ (`http://127.0.0.1:11434/v1`, ví dụ `llama3.2`) hoặc bất kỳ endpoint tương thích **OpenAI**, cùng tùy chọn **OpenAI** và **Anthropic** (BYOK lưu trong Windows Credential Manager).
  - Phản hồi dạng stream theo thời gian thực kèm khối SQL tô sáng cú pháp để sao chép hoặc chèn nhanh.
- **Máy chủ MCP cục bộ tích hợp (`tablepro-mcp`)**:
  - Chia sẻ kết nối đang mở trong DataForge cho IDE hoặc agent qua HTTP loopback (`127.0.0.1`), bảo vệ bằng Bearer token trong Windows Credential Manager.
  - Cung cấp **11 công cụ MCP**: `list_connections`, `connect`, `list_databases`, `list_schemas`, `list_tables`, `describe_table`, `get_table_ddl`, `browse_table`, `count_rows`, `execute_query` và `export_data`.
  - `execute_query` tuân thủ cấu hình External Clients / Safe Mode và yêu cầu người dùng xác nhận trực tiếp trên DataForge trước khi thực thi bất kỳ lệnh ghi nào.

---

### CLI & Tự động hóa không giao diện

`dataforge.exe` vừa là trình khởi chạy GUI vừa là công cụ dòng lệnh (`tablepro-cli`):

- **Cờ cấp ứng dụng**:
  - `dataforge.exe --version` (`-V`) — In phiên bản và thoát.
  - `dataforge.exe --help` (`-h`) — Hiển thị hướng dẫn sử dụng.
  - `dataforge.exe --smoke-startup` — Đo thời gian khởi động lạnh và kiểm tra đăng ký `9/9` driver.
- **Mở tệp / URL trực tiếp & Chuyển tiếp một phiên bản (Single-Instance)**:
  - Truyền đường dẫn `.sqlite` / `.db` / `.duckdb` / `.sql` hoặc URL kết nối (`postgresql://...`) trực tiếp cho `dataforge.exe` để mở trong cửa sổ đang chạy qua Named Pipe IPC (hoặc thêm `-n` / `--new-window`).
- **Các lệnh con CLI**:
  - `open`, `query`, `connections`, `databases`, `schemas`, `tables`, `describe`, `export`, `status` và `completion` (`bash`, `zsh`, `fish`).
  - Hỗ trợ `--format` (`table`, `csv`, `tsv`, `json`, `jsonl` cho `query`; `csv`, `json`, `sql` cho `export`), `--read-only`, `--limit` và `--timeout`.

```powershell
# Chạy truy vấn chỉ đọc từ PowerShell và xuất JSON Lines
.\target\release\dataforge.exe query -c Staging --read-only --format jsonl "SELECT id, email FROM users LIMIT 10"

# Xem DDL của bảng
.\target\release\dataforge.exe describe -c Staging --ddl users
```

---

## Hiệu năng thực tế

Đo trực tiếp trên Windows 11 với bản dựng release của `dataforge.exe` (`--release`):

| Bài đo | Kết quả ghi nhận |
| :--- | :--- |
| **Khởi động lạnh (`--smoke-startup`, 9/9 driver)** | `9.21 – 10.11 ms` |
| **Ghi dữ liệu E2E 100k dòng SQLite (`sqlite_100k_rows_load_and_scroll_perf`)** | `78.97 ms` |
| **Nạp lưới 100k dòng + cuộn 10k khung hình** | `24.43 ms` nạp · `0.432 ms` cuộn (`0.043 µs/frame`) |
| **Truy vấn có index trên 10 triệu dòng (`ORDER BY event_ts DESC LIMIT 1000`)** | `5 – 15 ms` |
| **Quét toàn bảng 10 triệu dòng + JSON + 3-table JOIN + 3 Window Functions** | `40.01 s` |
| **Mức RAM khi chạy stress test 100k / 500k / 1M dòng** | Ổn định `~55 MB` |

---

## Cài đặt & Tự động cập nhật

Tải bản dựng sẵn cho Windows x64 tại [**GitHub Releases**](https://github.com/williamcachamwri/DataForge/releases):

- **Trình cài đặt (Khuyên dùng) — `DataForge-win-Setup.exe`**:
  - Cài đặt gọn nhẹ vào `%LocalAppData%` kèm lối tắt Start Menu/Desktop—**không cần quyền quản trị UAC**.
  - Tích hợp **Velopack** hỗ trợ cập nhật delta tự động tiết kiệm băng thông. DataForge tự động kiểm tra bản mới khi khởi động (bật/tắt trong **Settings → About**) và cho phép kiểm tra, tải về, khởi động lại để cập nhật ngay tại **Settings → About**.
- **Bản Portable — `DataForge-win-Portable.zip`**:
  - Giải nén ở bất kỳ đâu và chạy thẳng `dataforge.exe` ở chế độ độc lập (Standalone) mà không can thiệp trình cài đặt hệ thống.
- **Kiểm tra toàn vẹn SHA-256**:
  - Mỗi bản phát hành đều đính kèm `SHA256SUMS.txt` để đối chiếu qua PowerShell:

```powershell
Get-FileHash .\DataForge-win-Setup.exe -Algorithm SHA256
```

---

## Phím tắt nhanh

| Phím tắt | Chức năng |
| :--- | :--- |
| `Ctrl+K` / `Ctrl+Shift+K` / `Ctrl+P` | Mở Command Palette |
| `Ctrl+Enter` / `F5` | Chạy truy vấn SQL hiện tại |
| `Ctrl+Shift+Enter` | Chạy đoạn SQL đang chọn |
| `Shift+Esc` | Hủy truy vấn đang chạy |
| `Ctrl+S` | Lưu truy vấn vào thư viện |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | Mở tab mới / Đóng tab / Mở lại tab vừa đóng |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Chuyển tab tiếp theo / trước đó |
| `Ctrl+F` | Focus ô lọc thanh đối tượng (Sidebar) |
| `Ctrl+B` | Ẩn/hiện thanh đối tượng (Sidebar) |
| `Ctrl+I` | Ẩn/hiện Row Inspector |
| `Ctrl+Shift+A` | Ẩn/hiện ngăn Trợ lý AI |
| `Ctrl+Shift+O` | Mở thư viện kết nối |
| `Ctrl+,` | Mở Cài đặt (Settings) |
| `Ctrl+Shift+M` | Đổi / chuyển theme |
| `Ctrl+Shift+S` / `Ctrl+Shift+R` | Lưu thay đổi (`Commit`) / Hoàn tác (`Rollback`) trên lưới |
| `Alt+Insert` / `Ctrl+Shift+Backspace` | Thêm dòng mới / Xóa các dòng đang chọn |
| `Ctrl+Shift+E` | Xuất kết quả hoặc bảng |
| `Esc` | Đóng hộp thoại / lớp phủ hiện tại |

*Mẹo: Bật **Vim mode** trên thanh công cụ SQL Editor để điều hướng theo chế độ `NORMAL`, `INSERT`, `VISUAL` và dòng lệnh `:`.*

---

## Biên dịch từ mã nguồn trên Windows

### Yêu cầu hệ thống

- **Hệ điều hành**: Windows 10 hoặc Windows 11 (x64)
- **Rust Toolchain**: Bản stable MSVC (`x86_64-pc-windows-msvc`, Rust `2024` Edition) qua [rustup](https://rustup.rs/)
- **Visual Studio C++ Build Tools & Windows SDK**: Cần thiết cho Win32, Direct3D 11 (`HeroGPUI`) và các driver C đi kèm
- **Shell**: Windows PowerShell (dùng dấu `;` thay cho `&&` khi nối lệnh)

> [!CAUTION]
> **Luôn build và test với `--release`**: Không chạy `cargo build` hoặc `cargo test` chế độ debug không tối ưu trên toàn workspace trừ khi thực sự cần debug symbol. Tệp debug PDB của toàn bộ workspace và 9 driver trong `target\debug` có thể phình to hơn **140 GB**. Luôn dùng `--release` (và `-j 1` nếu máy giới hạn RAM).

### Kiểm tra, Biên dịch & Chạy

```powershell
# Đảm bảo không có tiến trình dataforge.exe nào đang khóa tệp thực thi
Get-Process dataforge -ErrorAction SilentlyContinue

# Kiểm tra biên dịch ở chế độ release
cargo check --release -j 1 -p dataforge-windows-app

# Biên dịch bản release
cargo build --release -j 1 -p dataforge-windows-app

# Kiểm tra khởi động lạnh & đăng ký 9/9 driver
.\target\release\dataforge.exe --smoke-startup

# Khởi chạy DataForge
.\target\release\dataforge.exe
```

### Chạy bộ kiểm thử Release

```powershell
# Unit test bộ lọc & dialect SQL lõi
cargo test --release -j 1 -p tablepro-core filter

# Kiểm thử hiệu năng khởi động & độ phủ 6 ngôn ngữ
cargo test --release -j 1 -p dataforge-e2e --test startup_perf_e2e -- --nocapture

# Benchmark E2E tải 100k dòng SQLite & cuộn 10k khung hình
cargo test --release -j 1 -p dataforge-e2e --test sqlite_e2e -- --nocapture
```

---

## Kiến trúc & Bản đồ Workspace

```text
DataForge/
├── core/                             # Lõi cơ sở dữ liệu TablePro thượng nguồn & 9 driver thuần
│   └── crates/
│       ├── core/                     # tablepro-core: mô hình truy vấn, bộ lọc, dialect, xuất dữ liệu
│       ├── session/                  # tablepro-session: phiên kết nối, pool, giao dịch
│       ├── cli/                      # tablepro-cli: lệnh con shell & chạy không giao diện
│       ├── mcp/                      # tablepro-mcp: máy chủ MCP HTTP cục bộ bảo vệ bằng token
│       ├── ai/                       # tablepro-ai: client AI chỉ gửi schema (Ollama / OpenAI / Anthropic)
│       ├── ssh/                      # tablepro-ssh: SSH tunnel nội tuyến bằng russh
│       ├── net/                      # tablepro-net: rustls TLS + kho chứng chỉ hệ thống
│       ├── storage/                  # tablepro-storage: kết nối đã lưu, thư viện, lịch sử truy vấn
│       ├── foreign-import/           # tablepro-foreign-import: nhập cấu hình từ trình quản lý DB khác
│       ├── account/                  # tablepro-account: trait tài khoản & token
│       └── drivers/                  # postgres, mysql, sqlite, duckdb, mssql, clickhouse,
│                                     # cassandra, mongodb, redis
└── crates/
    ├── dataforge-gfx/                # Vector glyphs & biểu tượng hệ quản trị CSDL
    ├── dataforge-state/              # Trạng thái editor, autocomplete, mô hình lưới, tab, 14 theme
    ├── dataforge-services/           # Tokio runtime bất đồng bộ, điều phối session/AI/MCP, chẩn đoán
    ├── dataforge-windows-platform/   # Lưu settings JSON, Windows Credential Manager, DWM & giảm chuyển động
    ├── dataforge-windows-app/        # Ứng dụng HeroGPUI (Direct3D 11), trình cập nhật Velopack, 6 ngôn ngữ
    └── dataforge-e2e/                # Benchmark E2E SQLite, DuckDB, khởi động & hiệu năng lưới
```

---

## Đóng góp, Bảo mật, Quy tắc ứng xử & Giấy phép

- **Đóng góp**: Xem [CONTRIBUTING.md](CONTRIBUTING.md) để nắm quy tắc kiến trúc, build `--release`, hướng dẫn lớp phủ HeroGPUI và quy ước dịch 6 ngôn ngữ.
- **Lịch sử thay đổi**: Xem [CHANGELOG.md](CHANGELOG.md).
- **Quy tắc ứng xử**: Xem [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
- **Chính sách bảo mật**: Xem [SECURITY.md](SECURITY.md) để báo cáo lỗ hổng bảo mật riêng tư và tìm hiểu cam kết xử lý cục bộ.
- **Giấy phép**: DataForge được phát hành theo [LICENSE](LICENSE) (GNU Affero General Public License v3.0) và [LICENSE-MIT](LICENSE-MIT), đồng thời ghi nhận phần lõi thượng nguồn từ [TableProApp/TablePro](https://github.com/TableProApp/TablePro) (MIT).
