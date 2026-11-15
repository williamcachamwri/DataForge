//! Native Win32/D2D prompt dialogs for SSH verification, auth, and Safe Mode.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    IDYES, MB_DEFBUTTON2, MB_ICONQUESTION, MB_ICONWARNING, MB_YESNO, MessageBoxW,
};
use windows::core::PCWSTR;

use crate::i18n::Strings;

/// Prompt dialog variants required by TableMinus connection and safety flows.
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
    let (title, body, warning) = format_dialog_content(kind, strings);
    confirm_box(parent, title, &body, warning)
}

pub(crate) fn format_dialog_content(
    kind: &DialogKind,
    strings: &Strings,
) -> (&'static str, String, bool) {
    match kind {
        DialogKind::SshHostKey {
            host,
            port,
            algorithm,
            fingerprint,
        } => {
            let body = strings
                .ssh_host_key_prompt
                .replace("{host}", host)
                .replace("{port}", &port.to_string())
                .replace("{algorithm}", algorithm)
                .replace("{fingerprint}", fingerprint);
            (strings.ssh_host_key_title, body, false)
        }
        DialogKind::SafeModeConfirmation { summary } => {
            let body = if summary.trim().is_empty() {
                strings.safe_mode_prompt.to_owned()
            } else {
                format!("{}\n\n{summary}", strings.safe_mode_prompt)
            };
            (strings.safe_mode_title, body, true)
        }
        DialogKind::CloseTransactionGuard { connection_name } => {
            let body = if connection_name.trim().is_empty() {
                strings.open_tx_prompt.to_owned()
            } else {
                format!("[{connection_name}] {}", strings.open_tx_prompt)
            };
            (strings.open_tx_title, body, true)
        }
        DialogKind::SshPassphrase { key_label } => {
            let body = strings
                .ssh_passphrase_prompt
                .replace("{key_label}", key_label);
            (strings.ssh_passphrase_title, body, false)
        }
        DialogKind::InteractiveAuth { prompt, .. } => {
            (strings.interactive_auth_title, prompt.clone(), false)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Locale;

    #[test]
    fn native_dialogs_format_across_all_locales_without_unsubstituted_tokens() {
        for locale in Locale::ALL {
            let s = Strings::for_locale(locale);

            let (title, body, warn) = format_dialog_content(
                &DialogKind::SshHostKey {
                    host: "db.example.com".to_owned(),
                    port: 2222,
                    algorithm: "ed25519".to_owned(),
                    fingerprint: "SHA256:abc".to_owned(),
                },
                &s,
            );
            assert_eq!(title, s.ssh_host_key_title);
            assert!(!warn);
            assert!(body.contains("db.example.com:2222"));
            assert!(body.contains("ed25519"));
            assert!(body.contains("SHA256:abc"));
            assert!(!body.contains("{host}") && !body.contains("{port}"));

            let (title, body, warn) = format_dialog_content(
                &DialogKind::SshPassphrase {
                    key_label: "id_ed25519".to_owned(),
                },
                &s,
            );
            assert_eq!(title, s.ssh_passphrase_title);
            assert!(!warn);
            assert!(body.contains("id_ed25519"));
            assert!(!body.contains("{key_label}"));

            let (title, body, warn) = format_dialog_content(
                &DialogKind::SafeModeConfirmation {
                    summary: "DELETE FROM users".to_owned(),
                },
                &s,
            );
            assert_eq!(title, s.safe_mode_title);
            assert!(warn);
            assert!(body.contains(s.safe_mode_prompt));
            assert!(body.contains("DELETE FROM users"));

            let (title, body, warn) = format_dialog_content(
                &DialogKind::CloseTransactionGuard {
                    connection_name: "prod".to_owned(),
                },
                &s,
            );
            assert_eq!(title, s.open_tx_title);
            assert!(warn);
            assert!(body.contains("[prod]"));
            assert!(body.contains(s.open_tx_prompt));

            let (title, body, warn) = format_dialog_content(
                &DialogKind::CloseTransactionGuard {
                    connection_name: String::new(),
                },
                &s,
            );
            assert_eq!(title, s.open_tx_title);
            assert!(warn);
            assert_eq!(body, s.open_tx_prompt);

            let (title, body, warn) = format_dialog_content(
                &DialogKind::InteractiveAuth {
                    prompt: "OTP code".to_owned(),
                    masked: true,
                },
                &s,
            );
            assert_eq!(title, s.interactive_auth_title);
            assert!(!warn);
            assert_eq!(body, "OTP code");
        }
    }
}
