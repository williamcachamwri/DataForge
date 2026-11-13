//! DataForge Windows desktop application library.

pub mod accessibility;
pub mod app;
pub mod crash;
pub mod dialogs;
pub mod error;
pub mod i18n;
pub mod ipc;
pub mod shell_layout;
pub mod views;
pub mod window;

pub use app::{AppController, ScreenMode};
pub use error::{AppError, Result};
pub use shell_layout::{Rect, ShellHitTarget, ShellLayout, ShellLayoutConfig};
pub use window::MainWindow;
