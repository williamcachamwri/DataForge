//! Native Win32/D2D prompt dialogs for SSH verification, auth, and Safe Mode.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    IDYES, MB_DEFBUTTON2, MB_ICONQUESTION, MB_ICONWARNING, MB_YESNO, MessageBoxW,
};
use windows::core::PCWSTR;

use crate::i18n::Strings;

/// Prompt dialog variants required by DataForge connection and safety flows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogKind {
    /// Verify an unknown or changed SSH host key fingerprint.
    SshHostKey {
        host: String,
        port: u16,
        algorithm: String,
        fingerprint: String,
    },
    /// Prompt for an encrypted SSH private key passphrase.
    SshPassphrase { key_label: String },
    /// Interactive challenge-response authentication prompt.
    InteractiveAuth { prompt: String, masked: bool },
    /// Safe Mode confirmation before executing a mutating action.
    SafeModeConfirmation { summary: String },
    /// Confirmation before closing a window with an open transaction.
    CloseTransactionGuard { connection_name: String },
}

/// Result returned by a native prompt dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogOutcome {
    /// User confirmed (with optional entered secret/text).
    Accepted(Option<String>),
    /// User explicitly declined.
    Rejected,
    /// User cancelled or closed the dialog.
    Cancelled,
}

/// Displays a native Win32 dialog for `kind` and returns the user's [`DialogOutcome`].
#[must_use]
pub fn show_native_dialog(
    parent: Option<HWND>,
    kind: &DialogKind,
    strings: &Strings,
) -> DialogOutcome {
    match kind {
        DialogKind::SshHostKey {
            host,
            port,
            algorithm,
            fingerprint,
        } => {
            let body = format!(
                "The authenticity of host '{host}:{port}' cannot be established.\n\n\
                 {algorithm} key fingerprint:\n{fingerprint}\n\n\
                 Trust this host key and continue?"
            );
            confirm_box(parent, strings.ssh_host_key_title, &body, false)
        }
        DialogKind::SafeModeConfirmation { summary } => {
            let body = format!("{}\n\n{summary}", strings.safe_mode_prompt);
            confirm_box(parent, strings.safe_mode_title, &body, true)
        }
        DialogKind::CloseTransactionGuard { connection_name } => {
            let body = format!("[{connection_name}] {}", strings.open_tx_prompt);
            confirm_box(parent, strings.open_tx_title, &body, true)
        }
        DialogKind::SshPassphrase { key_label } => {
            let body = format!(
                "Passphrase required for SSH key '{key_label}'.\n\n\
                 Proceed with empty passphrase or cancel to abort?"
            );
            confirm_box(parent, strings.ssh_passphrase_title, &body, false)
        }
        DialogKind::InteractiveAuth { prompt, .. } => {
            confirm_box(parent, strings.interactive_auth_title, prompt, false)
        }
    }
}

fn confirm_box(parent: Option<HWND>, title: &str, body: &str, warning: bool) -> DialogOutcome {
    let title_w = to_wide_null(title);
    let body_w = to_wide_null(body);
    let icon = if warning {
        MB_ICONWARNING
    } else {
        MB_ICONQUESTION
    };
    let flags = MB_YESNO | icon | MB_DEFBUTTON2;
    // SAFETY: `body_w` and `title_w` are NUL-terminated UTF-16 buffers outliving `MessageBoxW`.
    let res = unsafe {
        MessageBoxW(
            parent,
            PCWSTR(body_w.as_ptr()),
            PCWSTR(title_w.as_ptr()),
            flags,
        )
    };
    if res == IDYES {
        DialogOutcome::Accepted(None)
    } else {
        DialogOutcome::Rejected
    }
}

fn to_wide_null(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
