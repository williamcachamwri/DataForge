#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

//! DataForge Windows desktop application entrypoint (`dataforge.exe`).

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use dataforge_windows_app::app::AppController;
use dataforge_windows_app::cli_flags::{AppFlagOutcome, handle_app_flags};
use dataforge_windows_app::crash::init_logging_and_crash_reports;
use dataforge_windows_app::error::Result;
use dataforge_windows_app::i18n::init_locale;
use dataforge_windows_app::ipc::{
    IpcInbox, acquire_or_forward, extract_forward_targets, spawn_ipc_listener,
};
use dataforge_windows_app::ui::launch_gpui_app;
use dataforge_windows_platform::{
    AppSettings, DEFAULT_MUTEX_NAME, DEFAULT_PIPE_NAME, SETTINGS_FILENAME, WindowsCliHost,
    storage_paths,
};
use tablepro_cli::{Dispatch, classify, run as run_cli};

fn main() -> ExitCode {
    // Handle Velopack installer/updater lifecycle hooks (--veloapp-*) before CLI/GUI dispatch.
    velopack::VelopackApp::build().run();

    let args: Vec<OsString> = std::env::args_os().collect();
    if should_attach_parent_console(&args) {
        attach_parent_console_if_cli();
    }
    {
        let mut stdout = std::io::stdout().lock();
        if let AppFlagOutcome::Exit(code) = handle_app_flags(&args, &mut stdout) {
            return ExitCode::from(code);
        }
    }
    match classify(&args) {
        Dispatch::Command => run_cli_subcommand(args),
        Dispatch::Invalid(message) => {
            let mut stderr = std::io::stderr().lock();
            let _ = writeln!(stderr, "{message}");
            ExitCode::from(2)
        }
        Dispatch::Gui(gui_args) => match launch_gui(&gui_args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                attach_parent_console_if_cli();
                let mut stderr = std::io::stderr().lock();
                let _ = writeln!(stderr, "dataforge: {err}");
                ExitCode::FAILURE
            }
        },
    }
}

fn should_attach_parent_console(args: &[OsString]) -> bool {
    let is_app_flag = args.get(1).and_then(|s| s.to_str()).is_some_and(|flag| {
        matches!(
            flag,
            "--version" | "-V" | "--help" | "-h" | "--smoke-startup"
        )
    });
    is_app_flag || !matches!(classify(args), Dispatch::Gui(_))
}

#[cfg(target_os = "windows")]
fn attach_parent_console_if_cli() {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
    };

    // Preserve redirected stdout/stderr pipes used by scripts, tests, and CI.
    // SAFETY: Standard Win32 console handle probe and optional parent console attach.
    unsafe {
        let out = GetStdHandle(STD_OUTPUT_HANDLE).unwrap_or_default();
        let err = GetStdHandle(STD_ERROR_HANDLE).unwrap_or_default();
        if (out == HANDLE::default() || out.is_invalid())
            && (err == HANDLE::default() || err.is_invalid())
        {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn attach_parent_console_if_cli() {}

fn run_cli_subcommand(args: Vec<OsString>) -> ExitCode {
    let Ok(paths) = storage_paths() else {
        return ExitCode::FAILURE;
    };
    let host = WindowsCliHost::new(paths).with_app_version(env!("CARGO_PKG_VERSION"));
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    let exit = run_cli(args, &host, &mut stdout, &mut stderr);
    ExitCode::from(exit.code())
}

fn launch_gui(gui_args: &[OsString]) -> Result<()> {
    enable_per_monitor_dpi();
    let paths = storage_paths()?;
    init_logging_and_crash_reports(&paths.state.join("logs"))?;

    let Some(_lock) = acquire_or_forward(DEFAULT_MUTEX_NAME, DEFAULT_PIPE_NAME, gui_args)? else {
        return Ok(());
    };

    let inbox = IpcInbox::new();
    let _ipc_thread = spawn_ipc_listener(DEFAULT_PIPE_NAME, inbox.clone())?;

    let settings_path = paths.config.join(SETTINGS_FILENAME);
    let settings = AppSettings::load(&settings_path).unwrap_or_default();
    let _locale = init_locale(settings.locale.as_deref());
    let mut controller = AppController::with_detected_theme(settings);

    let initial_targets = extract_forward_targets(gui_args);
    if !initial_targets.is_empty() {
        controller.open_forwarded_targets(&initial_targets);
    }

    launch_gpui_app(controller, inbox);
    Ok(())
}

fn enable_per_monitor_dpi() {
    use windows::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
    };
    // SAFETY: Setting process DPI awareness before creating any HWND is safe and idempotent.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}
