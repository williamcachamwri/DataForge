//! Lightweight localization catalog for DataForge UI strings.

/// Supported UI locales.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Locale {
    /// English (United States) — default.
    #[default]
    EnUs,
}

/// Resolved UI string catalog for the active locale.
#[derive(Debug, Clone)]
pub struct Strings {
    pub app_title: &'static str,
    pub connections_library: &'static str,
    pub settings: &'static str,
    pub tables: &'static str,
    pub views: &'static str,
    pub functions: &'static str,
    pub filter_placeholder: &'static str,
    pub data_tab: &'static str,
    pub structure_tab: &'static str,
    pub columns_button: &'static str,
    pub filters_button: &'static str,
    pub new_query_tab: &'static str,
    pub retry_connection: &'static str,
    pub cancel_connection: &'static str,
    pub safe_mode_title: &'static str,
    pub safe_mode_prompt: &'static str,
    pub open_tx_title: &'static str,
    pub open_tx_prompt: &'static str,
    pub ssh_host_key_title: &'static str,
    pub ssh_passphrase_title: &'static str,
    pub interactive_auth_title: &'static str,
    pub confirm_label: &'static str,
    pub cancel_label: &'static str,
    pub ai_assistant_title: &'static str,
    pub row_inspector_title: &'static str,
    pub no_selection: &'static str,
}

impl Strings {
    /// Returns the string catalog for `locale`.
    #[must_use]
    pub const fn for_locale(locale: Locale) -> Self {
        match locale {
            Locale::EnUs => Self {
                app_title: "DataForge",
                connections_library: "Connections",
                settings: "Settings",
                tables: "Tables",
                views: "Views",
                functions: "Functions",
                filter_placeholder: "Filter objects...",
                data_tab: "Data",
                structure_tab: "Structure",
                columns_button: "Columns",
                filters_button: "Filters",
                new_query_tab: "Query",
                retry_connection: "Retry",
                cancel_connection: "Cancel",
                safe_mode_title: "Safe Mode Confirmation",
                safe_mode_prompt: "This action modifies data in Safe Mode. Continue?",
                open_tx_title: "Open Transaction",
                open_tx_prompt: "An uncommitted transaction is open. Roll back and close?",
                ssh_host_key_title: "Verify SSH Host Key",
                ssh_passphrase_title: "SSH Key Passphrase",
                interactive_auth_title: "Authentication Required",
                confirm_label: "Confirm",
                cancel_label: "Cancel",
                ai_assistant_title: "AI Assistant",
                row_inspector_title: "Row Inspector",
                no_selection: "No row selected",
            },
        }
    }
}

/// Initializes i18n locale selection from the environment or user preference.
#[must_use]
pub fn init_locale(preferred: Option<&str>) -> Locale {
    let _ = preferred;
    Locale::EnUs
}
