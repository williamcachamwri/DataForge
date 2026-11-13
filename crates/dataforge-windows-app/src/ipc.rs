//! Single-instance coordination and Named Pipe IPC target forwarding.

use std::ffi::OsString;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use dataforge_windows_platform::{
    ForwardPayload, NamedPipeServer, SingleInstanceLock, is_forwardable_target,
    send_to_existing_instance,
};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::error::Result;

/// Custom Win32 message posted to the main window when IPC targets arrive.
pub const WM_APP_IPC_TARGETS: u32 = WM_APP + 1;

/// Extracts `.sqlite`, `.duckdb`, `.db`, and connection URLs from GUI arguments.
#[must_use]
pub fn extract_forward_targets(args: &[OsString]) -> Vec<String> {
    args.iter()
        .skip(1)
        .filter_map(|arg| arg.to_str())
        .filter(|s| is_forwardable_target(s))
        .map(ToOwned::to_owned)
        .collect()
}

/// Acquires the single-instance mutex or forwards GUI targets to the running instance.
///
/// Returns `Ok(Some(lock))` if this process is the primary instance, or `Ok(None)`
/// if an existing instance is already running and targets (if any) were forwarded.
pub fn acquire_or_forward(
    mutex_name: &str,
    pipe_name: &str,
    gui_args: &[OsString],
) -> Result<Option<SingleInstanceLock>> {
    if let Some(lock) = SingleInstanceLock::try_acquire(mutex_name)? {
        return Ok(Some(lock));
    }

    let targets = extract_forward_targets(gui_args);
    let payload = ForwardPayload { targets };
    let _ = send_to_existing_instance(pipe_name, &payload);
    Ok(None)
}

/// Thread-safe inbox for targets received from secondary launches.
#[derive(Debug, Default)]
pub struct IpcInbox {
    targets: Mutex<Vec<String>>,
    notify_hwnd: AtomicIsize,
    stopped: AtomicBool,
}

impl IpcInbox {
    /// Creates a new empty IPC inbox.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Registers the main window `HWND` to wake via `WM_APP_IPC_TARGETS`.
    pub fn attach_hwnd(&self, hwnd: HWND) {
        self.notify_hwnd.store(hwnd.0 as isize, Ordering::Release);
    }

    /// Detaches the window handle when shutting down.
    pub fn detach_hwnd(&self) {
        self.notify_hwnd.store(0, Ordering::Release);
        self.stopped.store(true, Ordering::Release);
    }

    /// Pushes new targets and wakes the attached window if present.
    pub fn push_targets(&self, incoming: Vec<String>) {
        if incoming.is_empty() {
            // Still wake to bring existing window to foreground on empty re-launch.
            self.post_wakeup();
            return;
        }
        if let Ok(mut guard) = self.targets.lock() {
            guard.extend(incoming);
        }
        self.post_wakeup();
    }

    /// Drains all pending targets from the inbox.
    #[must_use]
    pub fn drain_targets(&self) -> Vec<String> {
        match self.targets.lock() {
            Ok(mut guard) => std::mem::take(&mut *guard),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        }
    }

    fn post_wakeup(&self) {
        let raw = self.notify_hwnd.load(Ordering::Acquire);
        if raw != 0 {
            let hwnd = HWND(raw as *mut std::ffi::c_void);
            // SAFETY: `hwnd` was registered by the owning window and cleared on destroy.
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_APP_IPC_TARGETS, WPARAM(0), LPARAM(0));
            }
        }
    }
}

/// Spawns a background thread listening on `pipe_name` and forwarding into `inbox`.
pub fn spawn_ipc_listener(pipe_name: &str, inbox: Arc<IpcInbox>) -> Result<JoinHandle<()>> {
    let mut server = NamedPipeServer::bind(pipe_name)?;
    let handle = thread::Builder::new()
        .name("dataforge-ipc".to_owned())
        .spawn(move || {
            while !inbox.stopped.load(Ordering::Acquire) {
                match server.accept_one() {
                    Ok(payload) => inbox.push_targets(payload.targets),
                    Err(_) => break,
                }
            }
        })?;
    Ok(handle)
}
