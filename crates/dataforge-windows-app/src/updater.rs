//! Non-panicking Velopack auto-updater integration backed by GitHub Releases.

use std::any::Any;
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use tracing::{debug, info, warn};
use velopack::sources::GithubSource;
use velopack::{UpdateCheck, UpdateInfo, UpdateManager, VelopackAsset};

/// Canonical GitHub repository URL hosting DataForge Velopack releases.
pub const GITHUB_REPO_URL: &str = "https://github.com/williamcachamwri/DataForge";

/// Velopack package identifier (`--packId DataForge`).
pub const VELOPACK_PACK_ID: &str = "DataForge";

/// Maximum time to wait for a remote update feed check before reporting an error.
const UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(25);

/// Whether the running executable is managed by a Velopack installation or standalone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMode {
    /// Packaged and installed via Velopack (`Update.exe` + manifest present).
    VelopackManaged,
    /// Unpackaged `cargo run`, portable binary, or test harness execution.
    Standalone,
}

/// Observable lifecycle state of the application auto-updater.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateState {
    /// No check has been run in this session yet.
    #[default]
    Idle,
    /// Querying GitHub Releases for a newer Velopack package.
    Checking,
    /// Installed version matches or exceeds the latest published release.
    UpToDate,
    /// A newer release is available to download.
    Available {
        /// SemVer string of the target release.
        version: String,
        /// Markdown/plain-text release notes preview, if provided.
        notes: String,
    },
    /// Downloading and verifying the update package (`0..=100`).
    Downloading {
        /// Download/patch completion percentage.
        percent: u8,
    },
    /// Update package is staged on disk and ready to apply on restart.
    ReadyToRestart {
        /// SemVer string of the staged release.
        version: String,
    },
    /// Running unpackaged/standalone outside a Velopack installation.
    StandaloneMode,
    /// Non-fatal update check, download, or apply failure.
    Error(String),
}

/// Asynchronous events emitted by background update worker threads.
#[derive(Debug, Clone)]
pub enum UpdateEvent {
    /// Simple state transition (progress, up-to-date, standalone, or error).
    StateChanged(UpdateState),
    /// Check completed and found an available update payload.
    Available {
        /// Full Velopack update plan (base, deltas, and target release).
        info: Box<UpdateInfo>,
    },
    /// Download completed and verified; ready to apply on restart.
    Downloaded {
        /// Staged target release asset.
        asset: Box<VelopackAsset>,
    },
}

#[derive(Clone)]
struct UpdateChannel {
    events: Arc<Mutex<VecDeque<UpdateEvent>>>,
    wakeup_tx: Sender<()>,
    wakeup_rx: Receiver<()>,
}

impl UpdateChannel {
    fn new() -> Self {
        let (wakeup_tx, wakeup_rx) = async_channel::bounded(1);
        Self {
            events: Arc::new(Mutex::new(VecDeque::new())),
            wakeup_tx,
            wakeup_rx,
        }
    }

    fn send(&self, event: UpdateEvent) {
        {
            let mut guard = self.events.lock().unwrap_or_else(PoisonError::into_inner);
            guard.push_back(event);
        }
        let _ = self.wakeup_tx.try_send(());
    }

    fn drain(&self) -> Vec<UpdateEvent> {
        let mut guard = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        guard.drain(..).collect()
    }

    fn subscribe(&self) -> Receiver<()> {
        self.wakeup_rx.clone()
    }
}

/// Non-blocking Velopack update coordinator for [`crate::app::AppController`].
pub struct UpdaterManager {
    state: UpdateState,
    install_mode: InstallMode,
    current_version: String,
    manager: Option<UpdateManager>,
    pending_update: Option<UpdateInfo>,
    ready_asset: Option<VelopackAsset>,
    channel: UpdateChannel,
    auto_checked: bool,
}

impl Default for UpdaterManager {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdaterManager {
    /// Probes the current executable environment and initializes [`UpdaterManager`].
    ///
    /// Never panics: unpackaged/standalone binaries cleanly resolve to
    /// [`InstallMode::Standalone`] and [`UpdateState::StandaloneMode`].
    #[must_use]
    pub fn new() -> Self {
        let channel = UpdateChannel::new();
        let source = GithubSource::new(GITHUB_REPO_URL, None, false);
        match UpdateManager::new(source, None, None) {
            Ok(um) => Self::from_installed_manager(um, channel),
            Err(err) => Self::from_init_error(&err, channel),
        }
    }

    fn from_installed_manager(um: UpdateManager, channel: UpdateChannel) -> Self {
        let current_version = um.get_current_version_as_string();
        let pending_restart = um.get_update_pending_restart();
        let (state, ready_asset) = match pending_restart {
            Some(asset) => (
                UpdateState::ReadyToRestart {
                    version: asset.Version.clone(),
                },
                Some(asset),
            ),
            None => (UpdateState::Idle, None),
        };
        info!(
            version = %current_version,
            portable = um.get_is_portable(),
            "Velopack UpdateManager initialized"
        );
        Self {
            state,
            install_mode: InstallMode::VelopackManaged,
            current_version,
            manager: Some(um),
            pending_update: None,
            ready_asset,
            channel,
            auto_checked: false,
        }
    }

    fn from_init_error(err: &velopack::Error, channel: UpdateChannel) -> Self {
        let state = map_velopack_error(err);
        match &state {
            UpdateState::StandaloneMode => {
                debug!("DataForge is running in standalone mode (no Velopack install detected)");
            }
            UpdateState::Error(msg) => {
                warn!(error = %msg, "Velopack UpdateManager initialization warning");
            }
            _ => {}
        }
        Self {
            state,
            install_mode: InstallMode::Standalone,
            current_version: env!("CARGO_PKG_VERSION").to_owned(),
            manager: None,
            pending_update: None,
            ready_asset: None,
            channel,
            auto_checked: false,
        }
    }

    /// Returns the current [`UpdateState`].
    #[must_use]
    pub const fn state(&self) -> &UpdateState {
        &self.state
    }

    /// Returns whether the app is managed by Velopack or running standalone.
    #[must_use]
    pub const fn install_mode(&self) -> InstallMode {
        self.install_mode
    }

    /// Returns the active application version string.
    #[must_use]
    pub fn current_version(&self) -> &str {
        &self.current_version
    }

    /// Returns `true` while an asynchronous check or download is in progress.
    #[must_use]
    pub const fn is_busy(&self) -> bool {
        matches!(
            self.state,
            UpdateState::Checking | UpdateState::Downloading { .. }
        )
    }

    /// Subscribes to coalesced wakeup signals emitted when background [`UpdateEvent`]s arrive.
    #[must_use]
    pub fn subscribe(&self) -> Receiver<()> {
        self.channel.subscribe()
    }

    /// Triggers at most one startup update check when running in Velopack mode.
    ///
    /// Returns `true` if a background check was started.
    pub fn trigger_auto_check_once(&mut self) -> bool {
        if self.auto_checked {
            return false;
        }
        self.auto_checked = true;
        if self.install_mode == InstallMode::Standalone {
            return false;
        }
        if matches!(self.state, UpdateState::ReadyToRestart { .. }) {
            return false;
        }
        self.check_for_updates();
        matches!(self.state, UpdateState::Checking)
    }

    /// Starts a non-blocking update check against GitHub Releases.
    pub fn check_for_updates(&mut self) {
        if self.is_busy() {
            return;
        }
        let Some(um) = self.manager.clone() else {
            self.state = UpdateState::StandaloneMode;
            self.channel
                .send(UpdateEvent::StateChanged(UpdateState::StandaloneMode));
            return;
        };

        self.state = UpdateState::Checking;
        self.pending_update = None;
        let channel = self.channel.clone();
        let spawned = thread::Builder::new()
            .name("dataforge-update-check".to_owned())
            .spawn(move || run_check_with_timeout(um, &channel));
        if let Err(err) = spawned {
            self.state = UpdateState::Error(format!("failed to start update check: {err}"));
        }
    }

    /// Downloads the currently discovered update package in a background thread.
    pub fn download_update(&mut self) {
        if self.is_busy() {
            return;
        }
        let Some(um) = self.manager.clone() else {
            self.state = UpdateState::StandaloneMode;
            return;
        };
        let Some(info) = self.pending_update.clone() else {
            self.state = UpdateState::Error("no pending update metadata available".to_owned());
            return;
        };

        self.state = UpdateState::Downloading { percent: 0 };
        let (prog_tx, prog_rx) = mpsc::channel::<i16>();
        let channel_progress = self.channel.clone();
        let _ = thread::Builder::new()
            .name("dataforge-update-progress".to_owned())
            .spawn(move || forward_progress_events(&prog_rx, &channel_progress));

        let channel_done = self.channel.clone();
        let spawned = thread::Builder::new()
            .name("dataforge-update-download".to_owned())
            .spawn(move || run_download_worker(um, info, prog_tx, &channel_done));
        if let Err(err) = spawned {
            self.state = UpdateState::Error(format!("failed to start update download: {err}"));
        }
    }

    /// Applies the staged update package and restarts the application.
    ///
    /// On success, Velopack exits the current process (`exit(0)`) after launching
    /// `Update.exe` to swap binaries and relaunch DataForge.
    pub fn apply_and_restart(&mut self) -> Result<(), String> {
        let Some(um) = self.manager.as_ref() else {
            return Err("application is running in standalone mode".to_owned());
        };
        let Some(asset) = self
            .ready_asset
            .as_ref()
            .or_else(|| self.pending_update.as_ref().map(|u| &u.TargetFullRelease))
        else {
            return Err("no downloaded update package is ready to apply".to_owned());
        };
        um.apply_updates_and_restart(asset).map_err(|err| {
            let msg = err.to_string();
            self.state = UpdateState::Error(msg.clone());
            msg
        })
    }

    /// Drains all queued [`UpdateEvent`]s and updates [`Self::state`].
    ///
    /// Returns `true` if at least one event modified state.
    pub fn drain_events(&mut self) -> bool {
        let events = self.channel.drain();
        let changed = !events.is_empty();
        for event in events {
            self.apply_event(event);
        }
        changed
    }

    /// Applies a single [`UpdateEvent`] to internal state.
    pub fn apply_event(&mut self, event: UpdateEvent) {
        match event {
            UpdateEvent::StateChanged(next) => {
                self.state = next;
            }
            UpdateEvent::Available { info } => {
                let version = info.TargetFullRelease.Version.clone();
                let notes = extract_release_notes(&info.TargetFullRelease);
                self.pending_update = Some(*info);
                self.state = UpdateState::Available { version, notes };
            }
            UpdateEvent::Downloaded { asset } => {
                let version = asset.Version.clone();
                self.ready_asset = Some(*asset);
                self.state = UpdateState::ReadyToRestart { version };
            }
        }
    }
}

fn run_check_with_timeout(um: UpdateManager, channel: &UpdateChannel) {
    let (done_tx, done_rx) = mpsc::channel();
    let spawned = thread::Builder::new()
        .name("dataforge-update-http".to_owned())
        .spawn(move || {
            let outcome = catch_unwind(AssertUnwindSafe(|| um.check_for_updates()));
            let event = match outcome {
                Ok(res) => event_from_check_result(res),
                Err(payload) => UpdateEvent::StateChanged(UpdateState::Error(format!(
                    "update check panicked: {}",
                    panic_payload_message(payload)
                ))),
            };
            let _ = done_tx.send(event);
        });
    if let Err(err) = spawned {
        channel.send(UpdateEvent::StateChanged(UpdateState::Error(format!(
            "failed to spawn update check worker: {err}"
        ))));
        return;
    }

    let event = match done_rx.recv_timeout(UPDATE_CHECK_TIMEOUT) {
        Ok(event) => event,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            warn!("Velopack update check timed out");
            UpdateEvent::StateChanged(UpdateState::Error(format!(
                "Update check timed out after {}s.",
                UPDATE_CHECK_TIMEOUT.as_secs()
            )))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            warn!("Velopack update check worker terminated unexpectedly");
            UpdateEvent::StateChanged(UpdateState::Error(
                "Update check worker exited unexpectedly.".to_owned(),
            ))
        }
    };
    log_check_event(&event);
    channel.send(event);
}

fn run_download_worker(
    um: UpdateManager,
    info: UpdateInfo,
    prog_tx: mpsc::Sender<i16>,
    channel: &UpdateChannel,
) {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        um.download_updates(&info, Some(prog_tx))
    }));
    let event = match outcome {
        Ok(Ok(())) => {
            info!(
                version = %info.TargetFullRelease.Version,
                "Velopack update package downloaded"
            );
            UpdateEvent::Downloaded {
                asset: Box::new(info.TargetFullRelease),
            }
        }
        Ok(Err(err)) => {
            warn!(error = %err, "Velopack update download failed");
            UpdateEvent::StateChanged(map_velopack_error(&err))
        }
        Err(payload) => {
            let msg = format!(
                "update download panicked: {}",
                panic_payload_message(payload)
            );
            warn!(error = %msg, "Velopack update download panicked");
            UpdateEvent::StateChanged(UpdateState::Error(msg))
        }
    };
    channel.send(event);
}

fn panic_payload_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(msg) = payload.downcast_ref::<&str>() {
        (*msg).to_owned()
    } else if let Some(msg) = payload.downcast_ref::<String>() {
        msg.clone()
    } else {
        "unknown panic".to_owned()
    }
}

fn log_check_event(event: &UpdateEvent) {
    match event {
        UpdateEvent::Available { info } => {
            info!(
                version = %info.TargetFullRelease.Version,
                "DataForge update available"
            );
        }
        UpdateEvent::StateChanged(UpdateState::UpToDate) => {
            info!("DataForge is up to date");
        }
        UpdateEvent::StateChanged(UpdateState::Error(err)) => {
            warn!(error = %err, "Velopack update check failed");
        }
        _ => {}
    }
}

fn event_from_check_result(result: Result<UpdateCheck, velopack::Error>) -> UpdateEvent {
    match result {
        Ok(UpdateCheck::UpdateAvailable(info)) => UpdateEvent::Available { info },
        Ok(UpdateCheck::NoUpdateAvailable | UpdateCheck::RemoteIsEmpty) => {
            UpdateEvent::StateChanged(UpdateState::UpToDate)
        }
        Err(err) => UpdateEvent::StateChanged(map_velopack_error(&err)),
    }
}

fn map_velopack_error(err: &velopack::Error) -> UpdateState {
    match err {
        velopack::Error::NotInstalled(_) => UpdateState::StandaloneMode,
        other => UpdateState::Error(other.to_string()),
    }
}

fn forward_progress_events(prog_rx: &mpsc::Receiver<i16>, channel: &UpdateChannel) {
    let mut last_percent: Option<u8> = None;
    while let Ok(raw) = prog_rx.recv() {
        let clamped = u8::try_from(raw.clamp(0, 100)).unwrap_or(100);
        if last_percent == Some(clamped) {
            continue;
        }
        last_percent = Some(clamped);
        channel.send(UpdateEvent::StateChanged(UpdateState::Downloading {
            percent: clamped,
        }));
    }
}

/// Extracts readable release notes from a [`VelopackAsset`], preferring Markdown.
#[must_use]
pub fn extract_release_notes(asset: &VelopackAsset) -> String {
    let md = asset.NotesMarkdown.trim();
    if !md.is_empty() {
        return md.to_owned();
    }
    let html = asset.NotesHtml.trim();
    if html.is_empty() {
        return String::new();
    }
    strip_html_tags(html)
}

fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_environment_maps_to_standalone_mode_without_panicking() {
        let mut mgr = UpdaterManager::new();
        assert_eq!(mgr.install_mode(), InstallMode::Standalone);
        assert_eq!(mgr.state(), &UpdateState::StandaloneMode);
        assert_eq!(mgr.current_version(), env!("CARGO_PKG_VERSION"));
        assert!(!mgr.is_busy());

        // Auto-check in standalone mode is a quiet no-op.
        assert!(!mgr.trigger_auto_check_once());
        assert_eq!(mgr.state(), &UpdateState::StandaloneMode);

        // Second call also returns false.
        assert!(!mgr.trigger_auto_check_once());
    }

    #[test]
    fn map_velopack_error_distinguishes_not_installed_from_network_errors() {
        let not_installed = velopack::Error::NotInstalled("missing Update.exe".to_owned());
        assert_eq!(
            map_velopack_error(&not_installed),
            UpdateState::StandaloneMode
        );

        let other = velopack::Error::Other("timeout".to_owned());
        assert_eq!(
            map_velopack_error(&other),
            UpdateState::Error("timeout".to_owned())
        );
    }

    #[test]
    fn event_transitions_cover_available_download_and_restart() {
        let mut mgr = UpdaterManager::new();
        mgr.apply_event(UpdateEvent::StateChanged(UpdateState::Checking));
        assert!(mgr.is_busy());

        let asset = VelopackAsset {
            PackageId: VELOPACK_PACK_ID.to_owned(),
            Version: "0.2.0".to_owned(),
            Type: "Full".to_owned(),
            FileName: "DataForge-0.2.0-full.nupkg".to_owned(),
            NotesMarkdown: "## Highlights\n- Faster SQL grid".to_owned(),
            ..VelopackAsset::default()
        };
        let info = UpdateInfo {
            TargetFullRelease: asset.clone(),
            BaseRelease: None,
            DeltasToTarget: Vec::new(),
            IsDowngrade: false,
        };
        mgr.apply_event(UpdateEvent::Available {
            info: Box::new(info),
        });
        assert_eq!(
            mgr.state(),
            &UpdateState::Available {
                version: "0.2.0".to_owned(),
                notes: "## Highlights\n- Faster SQL grid".to_owned(),
            }
        );

        mgr.apply_event(UpdateEvent::StateChanged(UpdateState::Downloading {
            percent: 64,
        }));
        assert!(mgr.is_busy());
        assert_eq!(mgr.state(), &UpdateState::Downloading { percent: 64 });

        mgr.apply_event(UpdateEvent::Downloaded {
            asset: Box::new(asset),
        });
        assert!(!mgr.is_busy());
        assert_eq!(
            mgr.state(),
            &UpdateState::ReadyToRestart {
                version: "0.2.0".to_owned(),
            }
        );
    }

    #[test]
    fn extract_release_notes_prefers_markdown_and_falls_back_to_html() {
        let md_asset = VelopackAsset {
            NotesMarkdown: "  - Fixed bug  ".to_owned(),
            NotesHtml: "<p>Ignored</p>".to_owned(),
            ..VelopackAsset::default()
        };
        assert_eq!(extract_release_notes(&md_asset), "- Fixed bug");

        let html_asset = VelopackAsset {
            NotesMarkdown: String::new(),
            NotesHtml: "<p>Release <b>0.2.0</b></p>".to_owned(),
            ..VelopackAsset::default()
        };
        assert_eq!(extract_release_notes(&html_asset), "Release 0.2.0");
    }

    #[test]
    fn forward_progress_deduplicates_and_clamps_percentages() {
        let (prog_tx, prog_rx) = mpsc::channel();
        let channel = UpdateChannel::new();
        let wakeup = channel.subscribe();
        let _ = prog_tx.send(-5);
        let _ = prog_tx.send(0);
        let _ = prog_tx.send(42);
        let _ = prog_tx.send(42);
        let _ = prog_tx.send(150);
        drop(prog_tx);

        forward_progress_events(&prog_rx, &channel);

        assert!(wakeup.try_recv().is_ok());
        let mut percents = Vec::new();
        for event in channel.drain() {
            if let UpdateEvent::StateChanged(UpdateState::Downloading { percent }) = event {
                percents.push(percent);
            }
        }
        assert_eq!(percents, vec![0, 42, 100]);
    }

    #[test]
    fn subscribe_wakeup_does_not_consume_queued_events() {
        let mut mgr = UpdaterManager::new();
        let wakeup = mgr.subscribe();
        mgr.apply_event(UpdateEvent::StateChanged(UpdateState::Checking));
        mgr.channel
            .send(UpdateEvent::StateChanged(UpdateState::UpToDate));

        // Receiving the wakeup signal must leave queued events intact for drain_events().
        assert!(wakeup.try_recv().is_ok());
        assert!(mgr.drain_events());
        assert_eq!(mgr.state(), &UpdateState::UpToDate);
    }
}
