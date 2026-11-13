//! DataForge Windows desktop application library.

pub mod accessibility;
pub mod app;
pub mod branding;
pub mod cli_flags;
pub mod controller_ai_mcp;
pub mod controller_connection;
pub mod controller_editor;
pub mod controller_grid;
pub mod controller_library;
pub mod controller_types;
pub mod crash;
pub mod dialogs;
pub mod error;
pub mod i18n;
pub mod ipc;
pub mod services;
pub mod shell_layout;
pub mod ui;
pub mod views;
pub mod window;

pub use app::{AppController, ScreenMode};
pub use error::{AppError, Result};
pub use shell_layout::{Rect, ShellHitTarget, ShellLayout, ShellLayoutConfig};
pub use window::MainWindow;
