//! Single-instance mutex lock and Named Pipe IPC for forwarding targets to an existing instance.

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_BROKEN_PIPE, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED,
    GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE, FlushFileBuffers, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
    PIPE_UNLIMITED_INSTANCES, PIPE_WAIT, WaitNamedPipeW,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::PCWSTR;

use crate::error::{PlatformError, last_win32_error, to_wide_null};

/// Default Win32 named mutex used to coordinate a single DataForge GUI instance.
pub const DEFAULT_MUTEX_NAME: &str = r"Local\DataForge-SingleInstance-Mutex";

/// Default Win32 named pipe used to forward files and connection URLs to the primary instance.
pub const DEFAULT_PIPE_NAME: &str = r"\\.\pipe\DataForge-SingleInstance-IPC";

const PIPE_BUFFER_BYTES: u32 = 4096;

/// RAII wrapper around a Win32 `HANDLE` that closes it on drop.
#[derive(Debug)]
struct OwnedHandle(HANDLE);

// SAFETY: Win32 kernel handles for mutexes and pipes are safe to transfer and close across threads.
unsafe impl Send for OwnedHandle {}
// SAFETY: `OwnedHandle` only exposes shared read-only access or `Drop` (`CloseHandle`).
unsafe impl Sync for OwnedHandle {}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() && !self.0.0.is_null() {
            // SAFETY: `self.0` is a valid Win32 handle owned by this wrapper.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

/// Process-wide single-instance lock backed by `CreateMutexW`.
#[derive(Debug)]
pub struct SingleInstanceLock {
    _handle: OwnedHandle,
    name: String,
}

impl SingleInstanceLock {
    /// Attempts to acquire the named mutex.
    ///
    /// Returns `Ok(Some(lock))` if this process is the primary instance, or `Ok(None)` if
    /// another process already holds the mutex (`ERROR_ALREADY_EXISTS`).
    pub fn try_acquire(name: &str) -> Result<Option<Self>, PlatformError> {
        let wide = to_wide_null(name);
        // SAFETY: `wide` is NUL-terminated and outlives the `CreateMutexW` call.
        let handle =
            unsafe { CreateMutexW(None, false, PCWSTR(wide.as_ptr())) }.map_err(|err| {
                PlatformError::Win32 {
                    operation: "CreateMutexW",
                    source: std::io::Error::from_raw_os_error(err.code().0),
                }
            })?;

        let owned = OwnedHandle(handle);
        // SAFETY: `GetLastError` reads the status set by `CreateMutexW` on this thread.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            return Ok(None);
        }

        Ok(Some(Self {
            _handle: owned,
            name: name.to_owned(),
        }))
    }

    /// Returns the mutex name held by this lock.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Payload forwarded from a secondary launch to the primary DataForge instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForwardPayload {
    /// File paths (`.sqlite`, `.duckdb`, etc.) or connection URLs to open.
    pub targets: Vec<String>,
}

/// Returns `true` if `arg` looks like a database file or connection URL DataForge can open.
pub fn is_forwardable_target(arg: &str) -> bool {
    let lower = arg.trim().to_ascii_lowercase();
    let has_db_ext = lower.ends_with(".sqlite")
        || lower.ends_with(".sqlite3")
        || lower.ends_with(".duckdb")
        || lower.ends_with(".db");
    let has_scheme = [
        "postgres://",
        "postgresql://",
        "mysql://",
        "sqlite://",
        "duckdb://",
        "mongodb://",
        "mongodb+srv://",
        "redis://",
        "rediss://",
        "clickhouse://",
        "mssql://",
        "sqlserver://",
        "cassandra://",
        "dataforge://",
        "tablepro://",
    ]
    .iter()
    .any(|scheme| lower.starts_with(scheme));

    has_db_ext || has_scheme
}

/// Named-pipe IPC server that receives [`ForwardPayload`] messages from secondary instances.
#[derive(Debug)]
pub struct NamedPipeServer {
    pipe_name: String,
    listening_handle: OwnedHandle,
}

impl NamedPipeServer {
    /// Creates and binds the named pipe so clients can immediately connect.
    pub fn bind(pipe_name: &str) -> Result<Self, PlatformError> {
        let listening_handle = create_pipe_instance(pipe_name)?;
        Ok(Self {
            pipe_name: pipe_name.to_owned(),
            listening_handle,
        })
    }

    /// Returns the Win32 pipe path this server listens on.
    pub fn pipe_name(&self) -> &str {
        &self.pipe_name
    }

    /// Blocks until a client connects, reads one [`ForwardPayload`], and re-arms the pipe.
    pub fn accept_one(&mut self) -> Result<ForwardPayload, PlatformError> {
        let next = create_pipe_instance(&self.pipe_name)?;
        let current = std::mem::replace(&mut self.listening_handle, next);
        wait_for_client(current.0)?;
        let payload = read_payload_from_pipe(current.0)?;
        // SAFETY: `current.0` is a connected server-side named pipe handle.
        unsafe {
            let _ = DisconnectNamedPipe(current.0);
        }
        Ok(payload)
    }
}

/// Sends `payload` to an existing primary instance listening on `pipe_name`.
pub fn send_to_existing_instance(
    pipe_name: &str,
    payload: &ForwardPayload,
) -> Result<(), PlatformError> {
    let encoded = serde_json::to_vec(payload).map_err(|err| PlatformError::Json {
        context: "ForwardPayload",
        source: err,
    })?;
    let handle = open_pipe_client(pipe_name)?;
    write_all_to_pipe(handle.0, &encoded)?;
    // SAFETY: `handle.0` is an open pipe handle.
    unsafe {
        FlushFileBuffers(handle.0).map_err(|err| PlatformError::Win32 {
            operation: "FlushFileBuffers",
            source: std::io::Error::from_raw_os_error(err.code().0),
        })?;
    }
    Ok(())
}

fn create_pipe_instance(pipe_name: &str) -> Result<OwnedHandle, PlatformError> {
    let wide = to_wide_null(pipe_name);
    // SAFETY: `wide` is NUL-terminated and outlives `CreateNamedPipeW`.
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(wide.as_ptr()),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            PIPE_UNLIMITED_INSTANCES,
            PIPE_BUFFER_BYTES,
            PIPE_BUFFER_BYTES,
            0,
            None,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(last_win32_error("CreateNamedPipeW"));
    }
    Ok(OwnedHandle(handle))
}

fn wait_for_client(handle: HANDLE) -> Result<(), PlatformError> {
    // SAFETY: `handle` is a valid server-side named pipe handle.
    let result = unsafe { ConnectNamedPipe(handle, None) };
    match result {
        Ok(()) => Ok(()),
        Err(err) if err.code() == ERROR_PIPE_CONNECTED.to_hresult() => Ok(()),
        Err(err) => Err(PlatformError::Win32 {
            operation: "ConnectNamedPipe",
            source: std::io::Error::from_raw_os_error(err.code().0),
        }),
    }
}

fn open_pipe_client(pipe_name: &str) -> Result<OwnedHandle, PlatformError> {
    let wide = to_wide_null(pipe_name);
    for attempt in 0..2 {
        // SAFETY: `wide` is NUL-terminated and outlives `CreateFileW`.
        let opened = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        };
        match opened {
            Ok(handle) if handle != INVALID_HANDLE_VALUE => return Ok(OwnedHandle(handle)),
            Ok(_) => return Err(last_win32_error("CreateFileW(named pipe)")),
            Err(err) if attempt == 0 && err.code() == ERROR_PIPE_BUSY.to_hresult() => {
                // SAFETY: `wide` is NUL-terminated and outlives `WaitNamedPipeW`.
                let _ = unsafe { WaitNamedPipeW(PCWSTR(wide.as_ptr()), 2_000) };
            }
            Err(err) => {
                return Err(PlatformError::Win32 {
                    operation: "CreateFileW(named pipe)",
                    source: std::io::Error::from_raw_os_error(err.code().0),
                });
            }
        }
    }
    Err(last_win32_error("CreateFileW(named pipe)"))
}

fn write_all_to_pipe(handle: HANDLE, data: &[u8]) -> Result<(), PlatformError> {
    let mut offset = 0usize;
    while offset < data.len() {
        let chunk = &data[offset..];
        let mut written: u32 = 0;
        // SAFETY: `chunk` is valid for `chunk.len()` bytes; `written` receives bytes written.
        unsafe {
            WriteFile(handle, Some(chunk), Some(&raw mut written), None).map_err(|err| {
                PlatformError::Win32 {
                    operation: "WriteFile(named pipe)",
                    source: std::io::Error::from_raw_os_error(err.code().0),
                }
            })?;
        }
        if written == 0 {
            return Err(PlatformError::Io {
                operation: "WriteFile(named pipe)",
                source: std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "named pipe wrote zero bytes",
                ),
            });
        }
        offset += written as usize;
    }
    Ok(())
}

fn read_payload_from_pipe(handle: HANDLE) -> Result<ForwardPayload, PlatformError> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let mut read_bytes: u32 = 0;
        // SAFETY: `chunk` is a mutable 1024-byte stack buffer; `read_bytes` receives bytes read.
        let res = unsafe { ReadFile(handle, Some(&mut chunk), Some(&raw mut read_bytes), None) };
        match res {
            Ok(()) => {
                if read_bytes == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read_bytes as usize]);
            }
            Err(err) if err.code() == ERROR_BROKEN_PIPE.to_hresult() => break,
            Err(err) => {
                return Err(PlatformError::Win32 {
                    operation: "ReadFile(named pipe)",
                    source: std::io::Error::from_raw_os_error(err.code().0),
                });
            }
        }
    }
    serde_json::from_slice(&buffer).map_err(|err| PlatformError::Json {
        context: "ForwardPayload",
        source: err,
    })
}
