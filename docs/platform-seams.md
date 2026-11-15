# Windows Platform Seams (`tableminus-windows-platform`)

`crates/tableminus-windows-platform` isolates all Win32/Registry/Credential Manager/IPC calls behind safe Rust abstractions implementing TablePro's platform traits.

## 1. Windows Credential Manager (`secrets.rs`, `credential_parts.rs`)

`WindowsCredentialStore` implements `SecretVault`, `SecretItems`, and `RefreshTokenVault` using `CredReadW`, `CredWriteW`, `CredDeleteW`, `CredEnumerateW`, and `CredFree` with `CRED_TYPE_GENERIC` and `CRED_PERSIST_LOCAL_MACHINE`.

- **Target Naming**: Credentials are prefixed with `DEFAULT_CREDENTIAL_PREFIX` (`"TableMinus"`).
- **Multi-Part Chunking**: Windows Credential Manager caps `CredentialBlobSize` at `CRED_MAX_CREDENTIAL_BLOB_SIZE` (`2560` bytes, `PART_SIZE`). Secrets exceeding `PART_SIZE` (such as large SSH private keys or OAuth tokens) are split across numbered part entries tracked by the `TableMinus_Parts` (`LAYOUT_KEYWORD`) credential attribute.
- **Memory Hygiene**: Secret buffers are wrapped in `Zeroizing` / `SecretString` and explicitly zeroized on drop.

## 2. Single-Instance Mutex & Named Pipe IPC (`single_instance.rs`)

TableMinus coordinates single-instance GUI launches and file/URL forwarding via Win32 kernel objects wrapped in RAII `OwnedHandle` guards:

- **Mutex (`SingleInstanceLock`)**: Acquires `Local\TableMinus-SingleInstance-Mutex` (`DEFAULT_MUTEX_NAME`) via `CreateMutexW`. If `ERROR_ALREADY_EXISTS` is returned, the secondary process forwards its CLI target to the primary instance and exits cleanly.
- **Named Pipe (`NamedPipeServer` / `send_to_existing_instance`)**: Uses `\\.\pipe\TableMinus-SingleInstance-IPC` (`DEFAULT_PIPE_NAME`) with `PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT` (`PIPE_BUFFER_BYTES = 4096`) to transmit JSON-serialized `ForwardPayload` messages.
- **Target Validation**: `is_forwardable_target` accepts database file paths (`.sqlite`, `.sqlite3`, `.db`, `.duckdb`, `.ddb`) and supported connection URL schemes so arbitrary arguments are not forwarded blindly.

## 3. Windows Registry Seams (`theme_detect.rs`, `machine_id.rs`)

Registry keys are opened with `RegOpenKeyExW` and closed deterministically via `RegistryKey` RAII `Drop` guards:

- **Theme & High-Contrast Detection (`detect_theme`)**:
  1. Queries `SystemParametersInfoW(SPI_GETHIGHCONTRAST)` for `HCF_HIGHCONTRASTON` -> `WindowsTheme::HighContrast`.
  2. Reads `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme` (`REG_DWORD`) -> `WindowsTheme::Light` (`1`) or `WindowsTheme::Dark` (`0`).
- **Machine Identity (`read_machine_id`)**:
  - Reads `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid` (`KEY_READ | KEY_WOW64_64KEY`), strips braces/hyphens via `normalize_machine_guid`, and derives a `MachineId` (`derive_machine_id`) for account/device binding.

## 4. Storage Paths, Settings, and Foreign Discovery (`paths.rs`, `settings.rs`, `foreign.rs`, `browser.rs`, `cli_host.rs`)

- **Storage Paths (`storage_paths`, `storage_paths_for`)**: Resolves `APP_DIR_NAME` (`"TableMinus"`) under Windows `%APPDATA%` / `%LOCALAPPDATA%` via `StoragePaths` for connections, saved queries, themes, logs, and crash reports.
- **Settings (`AppSettings`)**: Persists `SETTINGS_FILENAME` (`settings.json`) atomically with `ThemePreference`, `EditorFont`, `WindowGeometry`, and `McpSettings`.
- **Browser & CLI (`WindowsBrowserLauncher`, `WindowsCliHost`)**: `validate_http_url` restricts external browser launches to `http://` and `https://` before invoking `ShellExecuteW`.
- **Foreign Import Discovery (`foreign_app_dirs`, `ForeignPlatform`)**: Locates configuration directories for TablePlus, DBeaver, Navicat, DataGrip, and Sequel Ace on Windows so `tablepro-foreign-import` can migrate saved connections.
