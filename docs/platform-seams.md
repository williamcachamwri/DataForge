# Windows Platform Seams (`dataforge-windows-platform`)

`crates/dataforge-windows-platform` isolates all Win32/Registry/Credential Manager/IPC calls behind safe Rust abstractions implementing TablePro's platform traits.

## 1. Windows Credential Manager (`secrets.rs`, `credential_parts.rs`)

`WindowsCredentialStore` implements `SecretVault`, `SecretItems`, and `RefreshTokenVault` using `CredReadW`, `CredWriteW`, `CredDeleteW`, `CredEnumerateW`, and `CredFree` with `CRED_TYPE_GENERIC` and `CRED_PERSIST_LOCAL_MACHINE`.

- **Target Naming**: Credentials are prefixed with `DEFAULT_CREDENTIAL_PREFIX` (`"DataForge"`).
- **Multi-Part Chunking**: Windows Credential Manager caps `CredentialBlobSize` at `CRED_MAX_CREDENTIAL_BLOB_SIZE` (`2560` bytes, `PART_SIZE`). Secrets exceeding `PART_SIZE` (such as large SSH private keys or OAuth tokens) are split across numbered part entries tracked by the `DataForge_Parts` (`LAYOUT_KEYWORD`) credential attribute.
- **Memory Hygiene**: Secret buffers are wrapped in `Zeroizing` / `SecretString` and explicitly zeroized on drop.

## 2. Single-Instance Mutex & Named Pipe IPC (`single_instance.rs`)

DataForge coordinates single-instance GUI launches and file/URL forwarding via Win32 kernel objects wrapped in RAII `OwnedHandle` guards:

- **Mutex (`SingleInstanceLock`)**: Acquires `Local\DataForge-SingleInstance-Mutex` (`DEFAULT_MUTEX_NAME`) via `CreateMutexW`. If `ERROR_ALREADY_EXISTS` is returned, the secondary process forwards its CLI target to the primary instance and exits cleanly.
- **Named Pipe (`NamedPipeServer` / `send_to_existing_instance`)**: Uses `\\.\pipe\DataForge-SingleInstance-IPC` (`DEFAULT_PIPE_NAME`) with `PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT` (`PIPE_BUFFER_BYTES = 4096`) to transmit JSON-serialized `ForwardPayload` messages.
- **Target Validation**: `is_forwardable_target` accepts database file paths (`.sqlite`, `.sqlite3`, `.db`, `.duckdb`, `.ddb`) and supported connection URL schemes so arbitrary arguments are not forwarded blindly.

## 3. Windows Registry Seams (`theme_detect.rs`, `machine_id.rs`)

Registry keys are opened with `RegOpenKeyExW` and closed deterministically via `RegistryKey` RAII `Drop` guards:

- **Theme & High-Contrast Detection (`detect_theme`)**:
  1. Queries `SystemParametersInfoW(SPI_GETHIGHCONTRAST)` for `HCF_HIGHCONTRASTON` -> `WindowsTheme::HighContrast`.
  2. Reads `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme` (`REG_DWORD`) -> `WindowsTheme::Light` (`1`) or `WindowsTheme::Dark` (`0`).
- **Machine Identity (`read_machine_id`)**:
  - Reads `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid` (`KEY_READ | KEY_WOW64_64KEY`), strips braces/hyphens via `normalize_machine_guid`, and derives a `MachineId` (`derive_machine_id`) for account/device binding.
