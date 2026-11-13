//! Windows Credential Manager backend for [`SecretVault`], [`SecretItems`], and [`RefreshTokenVault`].

use async_trait::async_trait;
use secrecy::zeroize::{Zeroize, Zeroizing};
use secrecy::{ExposeSecret, SecretString};
use tablepro_account::RefreshTokenVault;
use tablepro_core::AiProvider;
use tablepro_core::credentials::{
    SecretError, SecretItem, SecretItems, SecretKind, SecretLookup, SecretVault,
};
use tablepro_storage::english_item_label;
use uuid::Uuid;
use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, GetLastError};
use windows_sys::Win32::Security::Credentials::{
    CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_MAX_STRING_LENGTH, CRED_PERSIST_LOCAL_MACHINE,
    CRED_TYPE_GENERIC, CREDENTIAL_ATTRIBUTEW, CREDENTIALW, CredDeleteW, CredEnumerateW, CredFree,
    CredReadW, CredWriteW,
};

use crate::credential_parts::{self, Layout, PartStore, Stored};
use crate::error::to_wide_null;

/// Default target prefix for DataForge credentials in Windows Credential Manager.
pub const DEFAULT_CREDENTIAL_PREFIX: &str = "DataForge";

/// Attribute keyword used to track multi-part secret chunks.
pub const LAYOUT_KEYWORD: &str = "DataForge_Parts";

/// Maximum credential blob size supported by `CredWriteW` (2,560 bytes).
pub const PART_SIZE: usize = CRED_MAX_CREDENTIAL_BLOB_SIZE as usize;

const COMMENT_UNITS: usize = CRED_MAX_STRING_LENGTH as usize - 1;
const USER_NAME: &str = "DataForge";

static STORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Windows Credential Manager store for connection secrets, AI/MCP items, and account tokens.
#[derive(Clone)]
pub struct WindowsCredentialStore {
    prefix: String,
}

impl Default for WindowsCredentialStore {
    fn default() -> Self {
        Self::new(DEFAULT_CREDENTIAL_PREFIX)
    }
}

impl std::fmt::Debug for WindowsCredentialStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowsCredentialStore")
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

impl WindowsCredentialStore {
    /// Creates a credential store scoped under `prefix` (e.g. `"DataForge"`).
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
        }
    }

    /// Returns the credential target name for a connection secret.
    pub fn connection_target(&self, id: Uuid, kind: SecretKind) -> String {
        format!("{}/connection/{id}/{}", self.prefix, kind_name(kind))
    }

    /// Returns the credential target name for an application-wide [`SecretItem`].
    pub fn item_target(&self, item: SecretItem) -> String {
        format!("{}/{}", self.prefix, item_name(item))
    }

    /// Returns the credential target name for the account refresh token.
    pub fn refresh_token_target(&self) -> String {
        format!("{}/account/refresh-token", self.prefix)
    }

    /// Enumerates all generic credential target names matching `{prefix}/*` via `CredEnumerateW`.
    pub async fn enumerate_targets(&self) -> Result<Vec<String>, SecretError> {
        let filter = format!("{}/*", self.prefix);
        self.blocking(move || enumerate_matching(&filter)).await
    }

    /// Synchronously loads an application-wide [`SecretItem`] from Windows Credential Manager.
    pub fn load_item_sync(&self, item: SecretItem) -> Result<Option<SecretString>, SecretError> {
        let _held = STORE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let target = self.item_target(item);
        read_secret_string(&target)
    }

    async fn blocking<T, F>(&self, work: F) -> Result<T, SecretError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, SecretError> + Send + 'static,
    {
        let serialized = move || {
            let _held = STORE_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            work()
        };
        tokio::task::spawn_blocking(serialized)
            .await
            .map_err(|failure| SecretError::Backend {
                detail: failure.to_string(),
                service_message: None,
            })?
    }
}

fn kind_name(kind: SecretKind) -> &'static str {
    match kind {
        SecretKind::DatabasePassword => "database-password",
        SecretKind::SshPassword => "ssh-password",
        SecretKind::SshPassphrase => "ssh-passphrase",
    }
}

fn item_name(item: SecretItem) -> &'static str {
    match item {
        SecretItem::McpToken => "mcp-token",
        SecretItem::AiKey(AiProvider::Anthropic) => "ai-key/anthropic",
        SecretItem::AiKey(AiProvider::OpenAi) => "ai-key/openai",
        SecretItem::AiKey(AiProvider::OpenAiCompatible) => "ai-key/openai-compatible",
    }
}

fn failure(action: &str) -> SecretError {
    // SAFETY: reads the calling thread's last error, which the failed Win32 call just set.
    let code = unsafe { GetLastError() };
    SecretError::Backend {
        detail: format!("Credential Manager could not {action} a credential (error {code})"),
        service_message: None,
    }
}

fn decode_utf16_blob(blob: &[u8]) -> Result<SecretString, SecretError> {
    if !blob.len().is_multiple_of(2) {
        return Err(SecretError::InvalidEncoding);
    }
    let (pairs, _) = blob.as_chunks::<2>();
    let units: Vec<u16> = pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
    let units = Zeroizing::new(units);

    let mut size = 0;
    for decoded in char::decode_utf16(units.iter().copied()) {
        size += decoded
            .map_err(|_| SecretError::InvalidEncoding)?
            .len_utf8();
    }
    let mut text = String::with_capacity(size);
    text.extend(char::decode_utf16(units.iter().copied()).flatten());
    Ok(SecretString::from(text))
}

struct Credentials;

impl PartStore for Credentials {
    type Error = SecretError;

    fn read(&self, target: &str) -> Result<Option<Stored>, SecretError> {
        let name = to_wide_null(target);
        let mut found: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: `name` is NUL-terminated and outlives the call; `found` is freed via `CredFree`.
        let ok = unsafe { CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &raw mut found) };
        if ok == 0 {
            // SAFETY: reads the last error set by `CredReadW`.
            if unsafe { GetLastError() } == ERROR_NOT_FOUND {
                return Ok(None);
            }
            return Err(failure("read"));
        }
        let keyword = to_wide_null(LAYOUT_KEYWORD);
        // SAFETY: `found`, its blob, and its attributes are valid until `CredFree`.
        let stored = unsafe { extract_stored(&*found, &keyword) };
        // SAFETY: `found` was allocated by `CredReadW` and is freed once.
        unsafe { CredFree(found.cast_const().cast()) };
        Ok(Some(stored))
    }

    fn write(
        &self,
        target: &str,
        blob: &[u8],
        label: &str,
        layout: Option<Layout>,
    ) -> Result<(), SecretError> {
        let name = to_wide_null(target);
        let mut comment = to_wide_null(credential_parts::fitted(label, COMMENT_UNITS));
        let mut user = to_wide_null(USER_NAME);
        let size = u32::try_from(blob.len()).map_err(|_| SecretError::Backend {
            detail: "a part of the secret is too long for Credential Manager".to_owned(),
            service_message: None,
        })?;

        // SAFETY: every field of `CREDENTIALW` is an integer or pointer; zero-initialization is valid.
        let mut credential: CREDENTIALW = unsafe { std::mem::zeroed() };
        credential.Type = CRED_TYPE_GENERIC;
        credential.TargetName = name.as_ptr().cast_mut();
        credential.Comment = comment.as_mut_ptr();
        credential.CredentialBlobSize = size;
        credential.CredentialBlob = if blob.is_empty() {
            std::ptr::null_mut()
        } else {
            blob.as_ptr().cast_mut()
        };
        credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
        credential.UserName = user.as_mut_ptr();

        let mut keyword = to_wide_null(LAYOUT_KEYWORD);
        let mut value = layout.map(Layout::to_bytes);
        let mut attribute = CREDENTIAL_ATTRIBUTEW::default();
        if let Some(value) = value.as_mut() {
            attribute.Keyword = keyword.as_mut_ptr();
            attribute.ValueSize = Layout::SIZE as u32;
            attribute.Value = value.as_mut_ptr();
            credential.AttributeCount = 1;
            credential.Attributes = &raw mut attribute;
        }

        // SAFETY: all pointers in `credential` reference buffers that outlive `CredWriteW`.
        let ok = unsafe { CredWriteW(&raw const credential, 0) };
        if ok == 0 {
            return Err(failure("write"));
        }
        Ok(())
    }

    fn delete(&self, target: &str) -> Result<bool, SecretError> {
        let name = to_wide_null(target);
        // SAFETY: `name` is NUL-terminated and outlives the call.
        if unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) } != 0 {
            return Ok(true);
        }
        // SAFETY: reads the last error set by `CredDeleteW`.
        if unsafe { GetLastError() } == ERROR_NOT_FOUND {
            return Ok(false);
        }
        Err(failure("delete"))
    }
}

/// # Safety
///
/// `credential` must point to a live `CREDENTIALW` returned by `CredReadW`, with valid
/// `CredentialBlob`/`CredentialBlobSize` and `Attributes`/`AttributeCount` slices.
unsafe fn extract_stored(credential: &CREDENTIALW, keyword: &[u16]) -> Stored {
    let size = credential.CredentialBlobSize as usize;
    let blob = if credential.CredentialBlob.is_null() || size == 0 {
        Zeroizing::new(Vec::new())
    } else {
        // SAFETY: caller guarantees `CredentialBlob` is valid for `size` writable bytes.
        let slice = unsafe { std::slice::from_raw_parts_mut(credential.CredentialBlob, size) };
        let copied = Zeroizing::new(slice.to_vec());
        slice.zeroize();
        copied
    };

    let attributes = if credential.Attributes.is_null() {
        &[][..]
    } else {
        // SAFETY: caller guarantees `Attributes` is valid for `AttributeCount` elements.
        unsafe {
            std::slice::from_raw_parts(credential.Attributes, credential.AttributeCount as usize)
        }
    };

    let layout = attributes
        .iter()
        // SAFETY: `attribute.Keyword` is NUL-terminated per `CREDENTIALW` contract.
        .find(|attribute| unsafe { matches_wide(attribute.Keyword, keyword) })
        .filter(|attribute| !attribute.Value.is_null())
        .and_then(|attribute| {
            // SAFETY: `attribute.Value` is valid for `attribute.ValueSize` bytes.
            let bytes = unsafe {
                std::slice::from_raw_parts(attribute.Value, attribute.ValueSize as usize)
            };
            Layout::from_bytes(bytes)
        });

    Stored { blob, layout }
}

/// # Safety
///
/// If `keyword` is non-null, it must point to a NUL-terminated UTF-16 string.
unsafe fn matches_wide(keyword: *const u16, expected: &[u16]) -> bool {
    if keyword.is_null() {
        return false;
    }
    for (index, unit) in expected.iter().enumerate() {
        // SAFETY: all preceding units matched `expected` (which has NUL only at its end).
        if unsafe { *keyword.add(index) } != *unit {
            return false;
        }
    }
    true
}

fn enumerate_matching(filter: &str) -> Result<Vec<String>, SecretError> {
    let wide_filter = to_wide_null(filter);
    let mut count: u32 = 0;
    let mut creds: *mut *mut CREDENTIALW = std::ptr::null_mut();

    // SAFETY: `wide_filter` is NUL-terminated; `creds` is freed via `CredFree`.
    let ok = unsafe { CredEnumerateW(wide_filter.as_ptr(), 0, &raw mut count, &raw mut creds) };
    if ok == 0 {
        // SAFETY: reads the last error set by `CredEnumerateW`.
        if unsafe { GetLastError() } == ERROR_NOT_FOUND {
            return Ok(Vec::new());
        }
        return Err(failure("enumerate"));
    }

    let mut targets = Vec::with_capacity(count as usize);
    if !creds.is_null() {
        // SAFETY: `CredEnumerateW` succeeded and returned `count` valid pointers.
        let entries = unsafe { std::slice::from_raw_parts(creds, count as usize) };
        for &ptr in entries {
            if !ptr.is_null() {
                // SAFETY: each entry points to a valid `CREDENTIALW` with NUL-terminated `TargetName`.
                let target_ptr = unsafe { (*ptr).TargetName };
                if let Some(target) = unsafe { wide_ptr_to_string(target_ptr) } {
                    targets.push(target);
                }
            }
        }
        // SAFETY: `creds` was allocated by `CredEnumerateW` and is freed once.
        unsafe { CredFree(creds.cast_const().cast()) };
    }
    Ok(targets)
}

/// # Safety
///
/// If `ptr` is non-null, it must point to a valid NUL-terminated UTF-16 string.
unsafe fn wide_ptr_to_string(ptr: *const u16) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let mut len = 0usize;
    // SAFETY: caller guarantees `ptr` is NUL-terminated.
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: `ptr` is valid for `len` UTF-16 code units.
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    Some(String::from_utf16_lossy(slice))
}

fn read_secret_string(target: &str) -> Result<Option<SecretString>, SecretError> {
    match credential_parts::read_secret(&Credentials, target)? {
        Some(blob) => decode_utf16_blob(&blob).map(Some),
        None => Ok(None),
    }
}

fn write_secret_string(target: &str, secret: &str, label: &str) -> Result<(), SecretError> {
    let mut blob: Zeroizing<Vec<u8>> =
        Zeroizing::new(Vec::with_capacity(secret.encode_utf16().count() * 2));
    blob.extend(secret.encode_utf16().flat_map(u16::to_le_bytes));
    credential_parts::write_secret(&Credentials, target, &blob, label, PART_SIZE)
}

fn delete_secret_target(target: &str) -> Result<(), SecretError> {
    credential_parts::delete_secret(&Credentials, target)
}

#[async_trait]
impl SecretVault for WindowsCredentialStore {
    async fn store(
        &self,
        id: Uuid,
        kind: SecretKind,
        secret: &SecretString,
        label: &str,
    ) -> Result<(), SecretError> {
        let target = self.connection_target(id, kind);
        let (secret, label) = (secret.clone(), label.to_owned());
        self.blocking(move || write_secret_string(&target, secret.expose_secret(), &label))
            .await
    }

    async fn load(&self, id: Uuid, kind: SecretKind) -> Result<SecretLookup, SecretError> {
        let target = self.connection_target(id, kind);
        let found = self.blocking(move || read_secret_string(&target)).await?;
        Ok(found.map_or(SecretLookup::NotStored, SecretLookup::Found))
    }

    async fn relabel(&self, id: Uuid, kind: SecretKind, label: &str) -> Result<(), SecretError> {
        let target = self.connection_target(id, kind);
        let label = label.to_owned();
        self.blocking(move || match read_secret_string(&target)? {
            Some(secret) => write_secret_string(&target, secret.expose_secret(), &label),
            None => Ok(()),
        })
        .await
    }

    async fn delete_kind(&self, id: Uuid, kind: SecretKind) -> Result<(), SecretError> {
        let target = self.connection_target(id, kind);
        self.blocking(move || delete_secret_target(&target)).await
    }

    async fn delete_connection(&self, id: Uuid) -> Result<(), SecretError> {
        let targets: Vec<String> = SecretKind::ALL
            .iter()
            .map(|kind| self.connection_target(id, *kind))
            .collect();
        self.blocking(move || {
            targets
                .iter()
                .try_for_each(|target| delete_secret_target(target))
        })
        .await
    }
}

#[async_trait]
impl SecretItems for WindowsCredentialStore {
    async fn load_item(&self, item: SecretItem) -> Result<Option<SecretString>, SecretError> {
        let target = self.item_target(item);
        self.blocking(move || read_secret_string(&target)).await
    }

    async fn store_item(&self, item: SecretItem, secret: &SecretString) -> Result<(), SecretError> {
        let target = self.item_target(item);
        let secret = secret.clone();
        let label = english_item_label(item);
        self.blocking(move || write_secret_string(&target, secret.expose_secret(), &label))
            .await
    }

    async fn delete_item(&self, item: SecretItem) -> Result<(), SecretError> {
        let target = self.item_target(item);
        self.blocking(move || delete_secret_target(&target)).await
    }
}

#[async_trait]
impl RefreshTokenVault for WindowsCredentialStore {
    async fn load_refresh_token(&self) -> Result<Option<SecretString>, SecretError> {
        let target = self.refresh_token_target();
        self.blocking(move || read_secret_string(&target)).await
    }

    async fn store_refresh_token(&self, token: &SecretString) -> Result<(), SecretError> {
        let target = self.refresh_token_target();
        let token = token.clone();
        self.blocking(move || {
            write_secret_string(
                &target,
                token.expose_secret(),
                "DataForge account refresh token",
            )
        })
        .await
    }

    async fn delete_refresh_token(&self) -> Result<(), SecretError> {
        let target = self.refresh_token_target();
        self.blocking(move || delete_secret_target(&target)).await
    }
}
