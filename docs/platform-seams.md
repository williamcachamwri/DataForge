# Windows Platform Seams (`dataforge-windows-platform`)

`crates/dataforge-windows-platform` isolates all Win32/Registry/Credential Manager/IPC calls behind safe Rust abstractions implementing TablePro's platform traits.

## 1. Windows Credential Manager (`secrets.rs`, `credential_parts.rs`)

`WindowsCredentialStore` implements `SecretVault`, `SecretItems`, and `RefreshTokenVault` using `CredReadW`, `CredWriteW`, `CredDeleteW`, `CredEnumerateW`, and `CredFree` with `CRED_TYPE_GENERIC` and `CRED_PERSIST_LOCAL_MACHINE`.

- **Target Naming**: Credentials are prefixed with `DEFAULT_CREDENTIAL_PREFIX` (`"DataForge"`).
- **Multi-Part Chunking**: Windows Credential Manager caps `CredentialBlobSize` at `CRED_MAX_CREDENTIAL_BLOB_SIZE` (`2560` bytes, `PART_SIZE`). Secrets exceeding `PART_SIZE` (such as large SSH private keys or OAuth tokens) are split across numbered part entries tracked by the `DataForge_Parts` (`LAYOUT_KEYWORD`) credential attribute.
- **Memory Hygiene**: Secret buffers are wrapped in `Zeroizing` / `SecretString` and explicitly zeroized on drop.
