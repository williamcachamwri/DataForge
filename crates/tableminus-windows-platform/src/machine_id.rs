//! Windows `MachineGuid` registry reader for [`MachineId`].

use tablepro_account::MachineId;
use windows::Win32::Foundation::{ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, REG_SZ, REG_VALUE_TYPE, RegCloseKey,
    RegOpenKeyExW, RegQueryValueExW,
};
use windows::core::PCWSTR;

use crate::error::{PlatformError, to_wide_null};

const CRYPTOGRAPHY_SUBKEY: &str = r"SOFTWARE\Microsoft\Cryptography";
const MACHINE_GUID_VALUE: &str = "MachineGuid";

/// RAII guard for an opened Win32 `HKEY`.
pub(crate) struct RegistryKey(pub(crate) HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: `self.0` was opened by `RegOpenKeyExW` and is closed once.
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }
}

/// Normalizes a Windows `MachineGuid` string (e.g. `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`)
/// into 32 lowercase hexadecimal characters suitable for [`MachineId::derive`].
pub fn normalize_machine_guid(raw: &str) -> Result<String, PlatformError> {
    let cleaned: String = raw
        .trim()
        .trim_end_matches('\0')
        .chars()
        .filter(|ch| *ch != '-' && *ch != '{' && *ch != '}')
        .map(|ch| ch.to_ascii_lowercase())
        .collect();

    if cleaned.len() != 32 || !cleaned.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(PlatformError::InvalidRegistryValue {
            value_name: MACHINE_GUID_VALUE.to_owned(),
            detail: format!("expected 32 hex digits, got {} chars", cleaned.len()),
        });
    }

    if cleaned.bytes().all(|byte| byte == b'0') {
        return Err(PlatformError::InvalidRegistryValue {
            value_name: MACHINE_GUID_VALUE.to_owned(),
            detail: "MachineGuid is all zeroes".to_owned(),
        });
    }

    Ok(cleaned)
}

/// Reads `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid` via the Win32 Registry API
/// and normalizes it to 32 lowercase hex characters for [`MachineId::derive`].
pub fn read_machine_id() -> Result<String, PlatformError> {
    let subkey = to_wide_null(CRYPTOGRAPHY_SUBKEY);
    let value_name = to_wide_null(MACHINE_GUID_VALUE);
    let mut hkey = HKEY::default();

    // SAFETY: `subkey` is NUL-terminated; `hkey` receives the opened handle.
    let open_status = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            KEY_READ | KEY_WOW64_64KEY,
            &raw mut hkey,
        )
    };
    if open_status != ERROR_SUCCESS {
        return Err(reg_error("RegOpenKeyExW(Cryptography)", open_status));
    }
    let _guard = RegistryKey(hkey);

    let mut value_type = REG_VALUE_TYPE::default();
    let mut byte_len: u32 = 0;

    // SAFETY: First query with null buffer retrieves required byte size and value type.
    let size_status = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&raw mut value_type),
            None,
            Some(&raw mut byte_len),
        )
    };
    if size_status != ERROR_SUCCESS {
        return Err(reg_error("RegQueryValueExW(MachineGuid size)", size_status));
    }
    if value_type != REG_SZ {
        return Err(PlatformError::InvalidRegistryValue {
            value_name: MACHINE_GUID_VALUE.to_owned(),
            detail: format!("expected REG_SZ, got type {}", value_type.0),
        });
    }

    let unit_count = (byte_len as usize).div_ceil(2);
    let mut buffer = vec![0u16; unit_count.max(1)];
    let mut actual_bytes = (buffer.len() * 2) as u32;

    // SAFETY: `buffer` is writable for `actual_bytes` bytes.
    let read_status = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&raw mut value_type),
            Some(buffer.as_mut_ptr().cast()),
            Some(&raw mut actual_bytes),
        )
    };
    if read_status != ERROR_SUCCESS {
        return Err(reg_error("RegQueryValueExW(MachineGuid)", read_status));
    }

    let units = &buffer[..(actual_bytes as usize / 2).min(buffer.len())];
    let raw = String::from_utf16(units).map_err(|err| PlatformError::InvalidRegistryValue {
        value_name: MACHINE_GUID_VALUE.to_owned(),
        detail: err.to_string(),
    })?;

    normalize_machine_guid(&raw)
}

/// Reads the Windows `MachineGuid` and derives the HMAC-SHA256 [`MachineId`].
pub fn derive_machine_id() -> Result<MachineId, PlatformError> {
    let raw = read_machine_id()?;
    MachineId::derive(&raw).ok_or_else(|| PlatformError::InvalidRegistryValue {
        value_name: MACHINE_GUID_VALUE.to_owned(),
        detail: "MachineId::derive rejected normalized MachineGuid".to_owned(),
    })
}

fn reg_error(operation: &'static str, status: WIN32_ERROR) -> PlatformError {
    PlatformError::Win32 {
        operation,
        source: std::io::Error::from_raw_os_error(status.0 as i32),
    }
}
