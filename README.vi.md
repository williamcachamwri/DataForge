<div align="center">

<img src="crates/dataforge-windows-app/assets/dataforge_logo.png" alt="DataForge Logo" width="112" height="112" />

# DataForge

**Trình quản lý cơ sở dữ liệu & SQL Workbench thuần Rust, siêu nhẹ và mượt trên Windows.**

[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![GPUI](https://img.shields.io/badge/UI-HeroGPUI%20%2F%20Direct3D%2011-0078D4?style=flat-square&logo=windows&logoColor=white)](#tại-sao-chọn-dataforge)
[![Drivers](https://img.shields.io/badge/Drivers-9%20Engines-10B981?style=flat-square)](#hệ-quản-trị-csdl-hỗ-trợ)
[![Memory](https://img.shields.io/badge/RAM-~55%20MB%20khi%20tải-F59E0B?style=flat-square)](#hiệu-năng-thực-tế)
[![Startup](https://img.shields.io/badge/Khởi%20động-~10%20ms-6366F1?style=flat-square)](#hiệu-năng-thực-tế)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue?style=flat-square)](LICENSE-MIT)

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [Español](README.es.md) · [Français](README.fr.md)

</div>

---

## Tại sao chọn DataForge?

Phần lớn công cụ SQL hiện nay chạy trên Electron (ngốn hàng trăm MB đến hơn 1 GB RAM chỉ để mở một tab) hoặc JVM (khởi động chậm).

DataForge được xây dựng hoàn toàn bằng **Rust thuần** kết hợp **HeroGPUI (Direct3D 11)** và hiệu ứng **Windows 11 Gaussian Blur / Mica**. Ứng dụng khởi động chỉ khoảng **10 ms**, giữ mức RAM quanh **55 MB** ngay cả khi truy vấn bảng SQLite **10 triệu dòng**, nhưng vẫn đầy đủ tính năng hiện đại: autocomplete theo schema, Vim mode, chỉnh sửa trực tiếp trên lưới, Safe Mode bảo vệ dữ liệu và trợ lý AI nội bộ.

---

## Ảnh chụp màn hình

### SQL Editor & Truy vấn 10 triệu dòng (`15 ms`)
![DataForge SQL Editor querying 10 million rows](docs/screenshots/sql-editor-10m.jpg)

### Data Grid mật độ cao & Lọc dữ liệu
![DataForge Data Grid view](docs/screenshots/data-grid.jpg)

### Trợ lý AI nhận biết Schema (Bảo mật tuyệt đối)
![DataForge Schema-Aware AI Assistant](docs/screenshots/ai-assistant.jpg)

---

## Hệ quản trị CSDL hỗ trợ

| Engine | Driver Crate |
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

## Hiệu năng thực tế

| Bài đo | Kết quả ghi nhận |
| :--- | :--- |
| **Khởi động lạnh (`--smoke-startup`, 9/9 drivers)** | `9.21 – 10.11 ms` |
| **Benchmark 100k dòng SQLite (`sqlite_100k_rows_load_and_scroll_perf`)** | `78.97 ms` seed · `24.43 ms` load · `0.432 ms` scroll 10k frame |
| **Truy vấn có index trên bảng 10 triệu dòng (`LIMIT 1000`)** | `5 – 15 ms` |
| **Quét toàn bộ 10 triệu dòng + `json_extract` + 3-table `JOIN` + 3 Window Functions** | `40.01 s` |
| **Mức RAM khi chạy stress test 100k / 500k / 1M dòng** | `~55 MB` ổn định |

---

## Biên dịch từ mã nguồn

```powershell
cargo build --release -p dataforge-windows-app
.\target\release\dataforge.exe --smoke-startup
.\target\release\dataforge.exe
```

## Cộng đồng, Bảo mật & Giấy phép

- **Quy tắc ứng xử**: [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)
- **Chính sách bảo mật**: [SECURITY.md](SECURITY.md)
- **Giấy phép**: [MIT License](LICENSE-MIT) (ghi nhận nguồn core từ [TableProApp/TablePro](https://github.com/TableProApp/TablePro)) và [LICENSE](LICENSE).
