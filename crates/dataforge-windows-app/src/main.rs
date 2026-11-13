//! DataForge Windows desktop application entrypoint (`dataforge.exe`).

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use dataforge_windows_app::app::AppController;
use dataforge_windows_app::crash::init_logging_and_crash_reports;
use dataforge_windows_app::error::Result;
use dataforge_windows_app::i18n::init_locale;
use dataforge_windows_app::ipc::{
    IpcInbox, acquire_or_forward, extract_forward_targets, spawn_ipc_listener,
};
use dataforge_windows_app::window::MainWindow;
use dataforge_windows_platform::{
    AppSettings, DEFAULT_MUTEX_NAME, DEFAULT_PIPE_NAME, SETTINGS_FILENAME, WindowsCliHost,
    storage_paths,
};
use tablepro_cli::{Dispatch, classify, run as run_cli};

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
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
                let mut stderr = std::io::stderr().lock();
                let _ = writeln!(stderr, "dataforge: {err}");
                ExitCode::FAILURE
            }
        },
    }
}

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
    let paths = storage_paths()?;
    init_logging_and_crash_reports(&paths.state.join("logs"))?;
    let _locale = init_locale(None);

    let Some(_lock) = acquire_or_forward(DEFAULT_MUTEX_NAME, DEFAULT_PIPE_NAME, gui_args)? else {
        return Ok(());
    };

    let inbox = IpcInbox::new();
    let _ipc_thread = spawn_ipc_listener(DEFAULT_PIPE_NAME, inbox.clone())?;

    let settings_path = paths.config.join(SETTINGS_FILENAME);
    let settings = AppSettings::load(&settings_path).unwrap_or_default();
    let mut controller = AppController::with_detected_theme(settings);

    let initial_targets = extract_forward_targets(gui_args);
    if !initial_targets.is_empty() {
        controller.open_forwarded_targets(&initial_targets);
    }

    MainWindow::run(controller, inbox)
}
