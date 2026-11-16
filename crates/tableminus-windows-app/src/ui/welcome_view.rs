//! TablePlus-style Welcome / Connection Launcher screen, 2-step Create Connection modal,
//! and startup session router.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

use herogpui::components::{
    Button, ContextMenu, FieldVariant, Icon, IconName, IconSize, Input, InputState, InputType,
    MenuItem, SearchField, Size as HeroSize, Switch, Variant,
};
use herogpui::core::with_alpha;
use herogpui::gpui::{
    AnyElement, App, AppContext as _, AsyncApp, ClickEvent, Context, Entity, FontWeight, Hsla,
    ImageSource, InteractiveElement, IntoElement, MouseButton, ParentElement, PathPromptOptions,
    SharedString, StatefulInteractiveElement, Styled, WeakEntity, Window, div, img, px, rgb,
};
use herogpui::prelude::FluentBuilder;
use herogpui::theme::ActiveTheme;
use secrecy::SecretString;
use tableminus_gfx::glyphs::EngineKind;
use tableminus_state::library::{LibraryConnection, LibraryConnectionId, LibraryGroupId};
use tableminus_windows_platform::{
    EnvironmentKind, StartupSessionDecision, clear_active_workspace_session,
    decide_startup_session, storage_paths,
};
use tablepro_core::credentials::{SecretKind, SecretVault};
use tablepro_core::{
    AiPolicy, AuthMode as CoreAuthMode, ExternalAccess, NetworkOptions, SafeModeLevel, TlsConfig,
    TlsMode as CoreTlsMode,
};
use tablepro_storage::{ConnectionColor, ConnectionList, SavedConnection};
use uuid::Uuid;

use super::TableMinusWorkspace;
use super::engine_icons::engine_icon;
use super::shell_state::{push_error_toast, push_success_toast};
use super::theme::{
    COMPACT_GAP_PX, CONTROL_RADIUS_PX, HAIRLINE_BORDER_PX, OVERLAY_EXIT_MS, PANEL_RADIUS_PX,
    TEXT_BADGE_PX, TEXT_BODY_PX, TEXT_META_PX, animate_backdrop_fade, animate_fade_in,
    animate_modal_enter, animate_modal_exit, animate_slide_fade_in, animate_staggered_enter,
    animate_step_slide, effective_duration_ms, elevated_overlay_bg, hairline_border_color,
    muted_surface_bg, panel_surface_bg,
};
use crate::app::ScreenMode;
use crate::branding::tableminus_logo_image;
use crate::services::{default_port_for_engine, driver_id_from_engine};
use crate::views::library::{AuthMode, ConnectionFormDraft, TlsMode, engine_from_driver_id};

const MODAL_ENGINES: [EngineKind; 9] = [
    EngineKind::Postgresql,
    EngineKind::Mysql,
    EngineKind::Mariadb,
    EngineKind::Sqlite,
    EngineKind::Duckdb,
    EngineKind::Mssql,
    EngineKind::Mongodb,
    EngineKind::Redis,
    EngineKind::Clickhouse,
];

#[derive(Debug, Clone, Copy)]
struct DriverFormSpec {
    category_tag: &'static str,
    short_kind: &'static str,
    is_file: bool,
    supports_tls: bool,
    name_placeholder: &'static str,
    host_placeholder: &'static str,
    user_label: &'static str,
    user_placeholder: &'static str,
    password_label: &'static str,
    password_placeholder: &'static str,
    database_label: &'static str,
    database_placeholder: &'static str,
    default_database: &'static str,
    helper_note: &'static str,
}

const PG_SPEC: DriverFormSpec = DriverFormSpec {
    category_tag: "Relational · Default port 5432",
    short_kind: "Relational · 5432",
    is_file: false,
    supports_tls: true,
    name_placeholder: "Production Postgres",
    host_placeholder: "localhost",
    user_label: "User",
    user_placeholder: "postgres",
    password_label: "Password",
    password_placeholder: "••••••••",
    database_label: "Database",
    database_placeholder: "postgres",
    default_database: "",
    helper_note: "Standard PostgreSQL TCP connection with optional TLS & Safe Mode.",
};

const MYSQL_SPEC: DriverFormSpec = DriverFormSpec {
    category_tag: "Relational · Default port 3306",
    short_kind: "Relational · 3306",
    is_file: false,
    supports_tls: true,
    name_placeholder: "App MySQL",
    host_placeholder: "localhost",
    user_label: "User",
    user_placeholder: "root",
    password_label: "Password",
    password_placeholder: "••••••••",
    database_label: "Database",
    database_placeholder: "mysql",
    default_database: "",
    helper_note: "Supports MySQL 5.7 / 8.x+ with TLS encryption and Safe Mode.",
};

const MARIADB_SPEC: DriverFormSpec = DriverFormSpec {
    category_tag: "Relational · Default port 3306",
    short_kind: "Relational · 3306",
    is_file: false,
    supports_tls: true,
    name_placeholder: "MariaDB Primary",
    host_placeholder: "localhost",
    user_label: "User",
    user_placeholder: "root",
    password_label: "Password",
    password_placeholder: "••••••••",
    database_label: "Database",
    database_placeholder: "mariadb",
    default_database: "",
    helper_note: "MariaDB 10.x+ wire-compatible TCP connection with optional TLS.",
};

const SQLITE_SPEC: DriverFormSpec = DriverFormSpec {
    category_tag: "Embedded · Local database file",
    short_kind: "Local File · Embedded",
    is_file: true,
    supports_tls: false,
    name_placeholder: "Local SQLite DB",
    host_placeholder: "",
    user_label: "User",
    user_placeholder: "",
    password_label: "Encryption Key (optional)",
    password_placeholder: "Leave blank for unencrypted file",
    database_label: "Database File",
    database_placeholder: "C:\\path\\to\\database.sqlite",
    default_database: "",
    helper_note: "Opens .sqlite, .sqlite3, or .db files directly with zero server setup.",
};

const DUCKDB_SPEC: DriverFormSpec = DriverFormSpec {
    category_tag: "Embedded Analytics · Local OLAP file",
    short_kind: "Analytics · Embedded",
    is_file: true,
    supports_tls: false,
    name_placeholder: "Analytics DuckDB",
    host_placeholder: "",
    user_label: "User",
    user_placeholder: "",
    password_label: "Encryption Key (optional)",
    password_placeholder: "Leave blank for standard file",
    database_label: "Database File",
    database_placeholder: "C:\\path\\to\\analytics.duckdb (or :memory:)",
    default_database: "",
    helper_note: "In-process OLAP engine — select a .duckdb file or enter :memory:.",
};

const MSSQL_SPEC: DriverFormSpec = DriverFormSpec {
    category_tag: "Relational · Default port 1433",
    short_kind: "Relational · 1433",
    is_file: false,
    supports_tls: true,
    name_placeholder: "SQL Server Instance",
    host_placeholder: "localhost",
    user_label: "Login / User",
    user_placeholder: "sa",
    password_label: "Password",
    password_placeholder: "••••••••",
    database_label: "Database",
    database_placeholder: "master",
    default_database: "",
    helper_note: "Microsoft SQL Server TDS connection over port 1433.",
};

const MONGODB_SPEC: DriverFormSpec = DriverFormSpec {
    category_tag: "Document Store · Default port 27017",
    short_kind: "Document · 27017",
    is_file: false,
    supports_tls: true,
    name_placeholder: "MongoDB Cluster",
    host_placeholder: "localhost",
    user_label: "Username (optional)",
    user_placeholder: "admin",
    password_label: "Password",
    password_placeholder: "••••••••",
    database_label: "Auth / Default Database",
    database_placeholder: "admin",
    default_database: "",
    helper_note: "Connects to MongoDB collections; leave credentials blank for local unauth.",
};

const REDIS_SPEC: DriverFormSpec = DriverFormSpec {
    category_tag: "In-Memory Store · Default port 6379",
    short_kind: "In-Memory · 6379",
    is_file: false,
    supports_tls: true,
    name_placeholder: "Redis Cache",
    host_placeholder: "localhost",
    user_label: "ACL User (optional)",
    user_placeholder: "default",
    password_label: "Auth Password",
    password_placeholder: "Optional AUTH secret",
    database_label: "Database Index",
    database_placeholder: "0",
    default_database: "0",
    helper_note: "Redis 6+ ACL user is optional; logical database index defaults to 0.",
};

const CLICKHOUSE_SPEC: DriverFormSpec = DriverFormSpec {
    category_tag: "Columnar OLAP · Default port 8123",
    short_kind: "Columnar · 8123",
    is_file: false,
    supports_tls: true,
    name_placeholder: "ClickHouse Analytics",
    host_placeholder: "localhost",
    user_label: "User",
    user_placeholder: "default",
    password_label: "Password",
    password_placeholder: "••••••••",
    database_label: "Database",
    database_placeholder: "default",
    default_database: "",
    helper_note: "Connects via ClickHouse HTTP interface (port 8123 or 8443 with TLS).",
};

const CLUSTER_SPEC: DriverFormSpec = DriverFormSpec {
    category_tag: "Wide-Column Store",
    short_kind: "Wide-Column",
    is_file: false,
    supports_tls: true,
    name_placeholder: "Cluster Connection",
    host_placeholder: "localhost",
    user_label: "User",
    user_placeholder: "cassandra",
    password_label: "Password",
    password_placeholder: "••••••••",
    database_label: "Keyspace",
    database_placeholder: "system",
    default_database: "",
    helper_note: "Cluster node connection with optional TLS.",
};

const fn driver_form_spec(engine: EngineKind) -> DriverFormSpec {
    match engine {
        EngineKind::Postgresql => PG_SPEC,
        EngineKind::Mysql => MYSQL_SPEC,
        EngineKind::Mariadb => MARIADB_SPEC,
        EngineKind::Sqlite => SQLITE_SPEC,
        EngineKind::Duckdb => DUCKDB_SPEC,
        EngineKind::Mssql => MSSQL_SPEC,
        EngineKind::Mongodb => MONGODB_SPEC,
        EngineKind::Redis => REDIS_SPEC,
        EngineKind::Clickhouse => CLICKHOUSE_SPEC,
        EngineKind::Cassandra | EngineKind::Scylladb => CLUSTER_SPEC,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum CreateConnectionStep {
    #[default]
    EnginePicker,
    ConnectionForm,
}

/// Persistent UI state for the TablePlus-style Welcome / Connection Launcher screen.
pub struct WelcomeViewState {
    /// Whether the Welcome screen is currently active instead of the workspace.
    pub active: bool,
    /// Currently highlighted connection ID in the Welcome list.
    pub selected_connection_id: Option<String>,
    /// Connection ID currently being connected (used for subtle loading state).
    pub connecting_connection_id: Option<String>,
    /// Live search query filtering the saved/recent connections list.
    pub search_query: String,
    search_input: Entity<InputState>,
    hydrated: bool,
    create_modal_open: bool,
    create_modal_closing: bool,
    create_step: CreateConnectionStep,
    step_transition_forward: bool,
    modal_anim_nonce: u64,
    welcome_anim_nonce: u64,
    engine_search_query: String,
    engine_search_input: Entity<InputState>,
    selected_engine: EngineKind,
    form_name_input: Entity<InputState>,
    form_host_input: Entity<InputState>,
    form_port_input: Entity<InputState>,
    form_user_input: Entity<InputState>,
    form_password_input: Entity<InputState>,
    form_database_input: Entity<InputState>,
    form_color: Option<ConnectionColor>,
    form_tag: EnvironmentKind,
    form_safe_mode: bool,
    form_tls_enabled: bool,
    form_test_result: Option<Result<String, String>>,
}

impl WelcomeViewState {
    /// Initializes Welcome screen state owned by [`TableMinusWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, cx: &mut Context<TableMinusWorkspace>) -> Self {
        Self {
            active: false,
            selected_connection_id: None,
            connecting_connection_id: None,
            search_query: String::new(),
            search_input: cx.new(|cx| InputState::new(cx)),
            hydrated: false,
            create_modal_open: false,
            create_modal_closing: false,
            create_step: CreateConnectionStep::EnginePicker,
            step_transition_forward: true,
            modal_anim_nonce: 0,
            welcome_anim_nonce: 0,
            engine_search_query: String::new(),
            engine_search_input: cx.new(|cx| InputState::new(cx)),
            selected_engine: EngineKind::Postgresql,
            form_name_input: cx.new(|cx| InputState::new(cx)),
            form_host_input: cx.new(|cx| InputState::with_value(cx, "localhost")),
            form_port_input: cx.new(|cx| InputState::with_value(cx, "5432")),
            form_user_input: cx.new(|cx| InputState::new(cx)),
            form_password_input: cx.new(|cx| InputState::new(cx)),
            form_database_input: cx.new(|cx| InputState::new(cx)),
            form_color: None,
            form_tag: EnvironmentKind::Local,
            form_safe_mode: false,
            form_tls_enabled: false,
            form_test_result: None,
        }
    }

    /// Shows the Welcome / Connection Launcher screen and marks the list for refresh.
    pub fn open(&mut self) {
        self.active = true;
        self.connecting_connection_id = None;
        self.hydrated = false;
        self.welcome_anim_nonce = self.welcome_anim_nonce.wrapping_add(1);
    }

    /// Hides the Welcome screen when entering the active workspace.
    pub fn close(&mut self) {
        self.active = false;
        self.connecting_connection_id = None;
        self.create_modal_open = false;
        self.create_modal_closing = false;
    }

    /// Forces the saved connections list to re-hydrate from disk on the next render.
    pub fn mark_dirty(&mut self) {
        self.hydrated = false;
    }

    /// Opens the 2-step "Create Database / Connection" modal at Step 1 (Engine Picker).
    pub fn open_create_modal(&mut self, cx: &mut App) {
        self.create_modal_open = true;
        self.create_modal_closing = false;
        self.create_step = CreateConnectionStep::EnginePicker;
        self.step_transition_forward = true;
        self.modal_anim_nonce = self.modal_anim_nonce.wrapping_add(1);
        self.engine_search_query.clear();
        set_input_value(&self.engine_search_input, "", cx);
        self.form_test_result = None;
    }

    /// Closes the 2-step "Create Database / Connection" modal immediately.
    pub fn close_create_modal(&mut self) {
        self.create_modal_open = false;
        self.create_modal_closing = false;
        self.form_test_result = None;
    }
}

/// Evaluates startup session state and either restores the previous active workspace
/// directly or activates the Welcome / Connection Launcher screen.
pub fn initialize_startup_welcome_or_session(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) {
    hydrate_controller_library(ws);

    let decision = decide_startup_session(None, ws.controller.settings.restore_last_session);
    match decision {
        StartupSessionDecision::ShowWelcome => {
            ws.welcome_ui.open();
        }
        StartupSessionDecision::RestoreSession(snapshot) => {
            let Some(conn_id) = snapshot.active_connection_id.as_deref() else {
                ws.welcome_ui.open();
                return;
            };

            if let Err(err) = validate_saved_connection_target(ws, conn_id) {
                ws.controller.restoring_connection_id = None;
                ws.welcome_ui.open();
                push_error_toast("Session Restore Failed", Some(SharedString::from(err)), cx);
                return;
            }

            match ws.controller.connect_library_item_blocking(conn_id) {
                Ok(()) => {
                    ws.welcome_ui.close();
                    ws.switch_screen(ScreenMode::Workspace, cx);
                }
                Err(err) => {
                    ws.controller.restoring_connection_id = None;
                    ws.welcome_ui.open();
                    push_error_toast(
                        "Session Restore Failed",
                        Some(SharedString::from(err.to_string())),
                        cx,
                    );
                }
            }
        }
    }
}

/// Closes the active connection cleanly, clears the active session flag on disk,
/// and returns the shell to the Welcome / Connection Launcher screen.
pub fn close_active_connection_to_welcome(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let _ = ws.controller.disconnect_active_workspace();
    ws.controller.connections.close_all_connections();
    let layout_scope = ws.controller.grid_model.layout_scope.clone();
    let page_limit = ws.controller.grid_model.page.limit;
    ws.controller.grid_model =
        tableminus_state::data_grid::DataGridModel::new(layout_scope, page_limit);
    ws.controller.tabs = tableminus_state::tabs::TabStripModel::default();
    ws.console_log.clear();
    ws.console_log.visible = false;
    let _ = clear_active_workspace_session(None);
    ws.welcome_ui.open();
    ws.switch_screen(ScreenMode::Workspace, cx);
    cx.notify();
}

/// Renders the TablePlus-style two-pane Welcome / Connection Launcher screen
/// and the optional 2-step Create Connection modal overlay.
pub fn render_welcome_screen(
    ws: &mut TableMinusWorkspace,
    _window: &mut Window,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    if !ws.welcome_ui.hydrated {
        ws.welcome_ui.hydrated = true;
        hydrate_controller_library(ws);
    }

    let colors = cx.colors().clone();
    let mut root = div()
        .id("welcome-screen-root")
        .relative()
        .flex_1()
        .size_full()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .flex()
        .flex_row()
        .bg(ws.content_palette.window_background)
        .text_color(colors.foreground)
        .child(render_welcome_left_pane(ws, cx))
        .child(render_welcome_right_pane(ws, cx));

    if ws.welcome_ui.create_modal_open {
        root = root.child(render_create_connection_modal(ws, cx));
    }

    root.into_any_element()
}

fn render_welcome_left_pane(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let total_saved = ws.controller.library.storage().connections.len();
    let pane = div()
        .id("welcome-left-pane")
        .w(px(288.0))
        .h_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .justify_between()
        .px(px(24.0))
        .py(px(28.0))
        .bg(panel_surface_bg(cx))
        .border_r(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .child(render_welcome_brand_hero(
            ws.controller.strings.app_title,
            total_saved,
            cx,
        ))
        .child(render_welcome_left_actions(cx));

    animate_slide_fade_in(
        pane,
        ("welcome-left-enter", ws.welcome_ui.welcome_anim_nonce),
        180,
        8.0,
        cx,
    )
}

fn render_welcome_brand_hero(app_title: &'static str, total_saved: usize, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    let version_label = concat!("v", env!("CARGO_PKG_VERSION"));
    let count_label = match total_saved {
        0 => "No saved connections".to_owned(),
        1 => "1 saved connection".to_owned(),
        n => format!("{n} saved connections"),
    };

    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(10.0))
        .pt(px(20.0))
        .child(
            div()
                .w(px(80.0))
                .h(px(80.0))
                .rounded(px(20.0))
                .bg(with_alpha(colors.surface_secondary, 0.72))
                .border(px(HAIRLINE_BORDER_PX))
                .border_color(hairline_border_color(cx))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    img(ImageSource::Image(tableminus_logo_image()))
                        .w(px(64.0))
                        .h(px(64.0))
                        .flex_shrink_0(),
                ),
        )
        .child(
            div()
                .pt(px(4.0))
                .text_size(px(20.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.foreground)
                .child(app_title),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .px(px(8.0))
                        .py(px(2.0))
                        .rounded(px(99.0))
                        .bg(with_alpha(colors.surface_secondary, 0.85))
                        .border(px(HAIRLINE_BORDER_PX))
                        .border_color(hairline_border_color(cx))
                        .text_size(px(TEXT_BADGE_PX))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.muted)
                        .child(version_label),
                )
                .child(
                    div()
                        .text_size(px(TEXT_META_PX))
                        .text_color(colors.muted)
                        .child(count_label),
                ),
        )
        .into_any_element()
}

fn render_welcome_left_actions(cx: &mut Context<TableMinusWorkspace>) -> AnyElement {
    let colors = cx.colors().clone();
    let create_btn = render_welcome_action_button(
        "welcome-create-connection",
        Variant::Primary,
        IconName::Plus,
        "Create Connection",
        cx.listener(|ws, _, _window, cx| open_create_connection_modal(ws, cx)),
    );
    let backup_btn = render_welcome_action_button(
        "welcome-backup-export",
        Variant::Secondary,
        IconName::Download,
        "Backup / Export",
        cx.listener(|ws, _, _window, cx| trigger_backup_export(ws, cx)),
    );
    let restore_btn = render_welcome_action_button(
        "welcome-restore-import",
        Variant::Secondary,
        IconName::Upload,
        "Restore / Import",
        cx.listener(|ws, _, _window, cx| trigger_restore_import(ws, cx)),
    );

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(create_btn)
        .child(backup_btn)
        .child(restore_btn)
        .child(
            div()
                .pt(px(8.0))
                .flex()
                .justify_center()
                .text_size(px(TEXT_BADGE_PX))
                .text_color(colors.muted)
                .child("Click a connection to open · Ctrl+K for commands"),
        )
        .into_any_element()
}

fn render_welcome_action_button(
    id: &'static str,
    variant: Variant,
    icon: IconName,
    label: &'static str,
    on_press: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .variant(variant)
        .size(HeroSize::Md)
        .full_width(true)
        .on_press(on_press)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .child(Icon::new(icon).size(IconSize::Sm))
                .child(label),
        )
}

fn render_welcome_right_pane(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let query = ws.welcome_ui.search_query.trim().to_ascii_lowercase();
    let connections = filtered_welcome_connections(ws, &query);

    let colors = cx.colors().clone();
    let add_btn = Button::new("welcome-top-add-connection")
        .variant(Variant::Secondary)
        .size(HeroSize::Sm)
        .on_press(cx.listener(|ws, _, _window, cx| {
            open_create_connection_modal(ws, cx);
        }))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.0))
                .child(Icon::new(IconName::Plus).size(IconSize::Xs))
                .child("New"),
        );

    let search_field = SearchField::new(&ws.welcome_ui.search_input)
        .placeholder("Search connections by name, host, database, or driver...")
        .variant(FieldVariant::Secondary)
        .full_width()
        .on_change(cx.listener(|ws, val: &str, _, cx| {
            ws.welcome_ui.search_query = val.to_owned();
            cx.notify();
        }));

    let count_pill = div()
        .px(px(8.0))
        .py(px(3.0))
        .rounded(px(99.0))
        .bg(with_alpha(colors.surface_secondary, 0.7))
        .border(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .text_size(px(TEXT_BADGE_PX))
        .text_color(colors.muted)
        .flex_shrink_0()
        .child(format!("{}", connections.len()));

    let top_bar = div()
        .w_full()
        .px(px(20.0))
        .py(px(14.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .border_b(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .child(add_btn)
        .child(div().flex_1().min_w(px(0.0)).child(search_field))
        .child(count_pill);

    let mut list_scroll = div()
        .id("welcome-connections-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .px(px(16.0))
        .py(px(12.0))
        .flex()
        .flex_col()
        .gap(px(COMPACT_GAP_PX));

    if connections.is_empty() {
        list_scroll = list_scroll.child(render_welcome_empty_state(query.is_empty(), cx));
    } else {
        for (idx, conn) in connections.iter().enumerate() {
            list_scroll = list_scroll.child(render_welcome_connection_row(ws, conn, idx, cx));
        }
    }

    div()
        .id("welcome-right-pane")
        .flex_1()
        .h_full()
        .min_w(px(0.0))
        .min_h_0()
        .flex()
        .flex_col()
        .child(top_bar)
        .child(list_scroll)
        .into_any_element()
}

fn filtered_welcome_connections(ws: &TableMinusWorkspace, query: &str) -> Vec<LibraryConnection> {
    let mut connections: Vec<LibraryConnection> = ws
        .controller
        .library
        .storage()
        .connections
        .values()
        .filter(|conn| {
            if query.is_empty() {
                return true;
            }
            conn.name.to_ascii_lowercase().contains(query)
                || conn.host.to_ascii_lowercase().contains(query)
                || conn.database.to_ascii_lowercase().contains(query)
                || conn.driver_id.to_ascii_lowercase().contains(query)
        })
        .cloned()
        .collect();

    connections.sort_by(|a, b| {
        b.is_favorite
            .cmp(&a.is_favorite)
            .then_with(|| a.order.cmp(&b.order))
            .then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
    });
    connections
}

fn render_welcome_empty_state(
    no_connections_at_all: bool,
    cx: &Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let empty_title = if no_connections_at_all {
        "No saved connections yet"
    } else {
        "No matching connections"
    };
    let empty_hint = if no_connections_at_all {
        "Click Create Connection or Restore / Import to get started."
    } else {
        "Try a different search term or clear the filter."
    };
    div()
        .flex_1()
        .min_h(px(220.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .child(
            Icon::new(IconName::Database)
                .size(IconSize::Lg)
                .color(colors.muted),
        )
        .child(
            div()
                .text_size(px(14.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.foreground)
                .child(empty_title),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(colors.muted)
                .child(empty_hint),
        )
        .into_any_element()
}

fn render_welcome_connection_row(
    ws: &TableMinusWorkspace,
    conn: &LibraryConnection,
    idx: usize,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let engine = engine_from_driver_id(&conn.driver_id);
    let (_badge_code, badge_bg) = engine_badge_style(engine);
    let subtitle = format_connection_subtitle(conn, engine);
    let conn_id = conn.id.0.clone();
    let is_selected = ws
        .welcome_ui
        .selected_connection_id
        .as_deref()
        .is_some_and(|id| id == conn_id);
    let is_connecting = ws
        .welcome_ui
        .connecting_connection_id
        .as_deref()
        .is_some_and(|id| id == conn_id);

    let row_bg = if is_selected {
        with_alpha(colors.accent.color, 0.13)
    } else {
        with_alpha(colors.surface.background, 0.42)
    };
    let border_col = if is_selected {
        with_alpha(colors.accent.color, 0.55)
    } else {
        hairline_border_color(cx)
    };

    let click_id = conn_id.clone();
    let row_el = div()
        .id(SharedString::from(format!("welcome-conn-row-{idx}")))
        .w_full()
        .px(px(14.0))
        .py(px(11.0))
        .rounded(px(PANEL_RADIUS_PX))
        .border(px(HAIRLINE_BORDER_PX))
        .border_color(border_col)
        .bg(row_bg)
        .hover(|style| {
            style
                .bg(with_alpha(colors.accent.color, 0.10))
                .border_color(with_alpha(colors.accent.color, 0.42))
        })
        .cursor_pointer()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .on_click(cx.listener(move |ws, _event, _window, cx| {
            connect_welcome_connection(ws, &click_id, cx);
        }))
        .child(render_engine_badge(engine, badge_bg, false))
        .child(render_connection_row_meta(
            &conn.name,
            &subtitle,
            conn.is_favorite,
            cx,
        ))
        .child(render_connection_row_trailing(engine, is_connecting, cx));

    let with_ctx = wrap_connection_row_context_menu(row_el, &conn_id, conn.is_favorite, idx, cx);
    animate_staggered_enter(
        div().w_full().child(with_ctx),
        (
            "welcome-row-anim",
            (ws.welcome_ui.welcome_anim_nonce << 8) | (idx as u64),
        ),
        idx,
        cx,
    )
}

fn render_connection_row_meta(
    name: &str,
    subtitle: &str,
    is_favorite: bool,
    cx: &App,
) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .truncate()
                        .text_size(px(TEXT_BODY_PX))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(name.to_owned()),
                )
                .when(is_favorite, |d| {
                    d.child(
                        Icon::new(IconName::Star)
                            .size(IconSize::Xs)
                            .color(rgb(0xF59E0B)),
                    )
                }),
        )
        .child(
            div()
                .truncate()
                .text_size(px(TEXT_META_PX))
                .text_color(colors.muted)
                .child(subtitle.to_owned()),
        )
        .into_any_element()
}

fn render_connection_row_trailing(engine: EngineKind, is_connecting: bool, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    let pill_bg = with_alpha(
        if is_connecting {
            colors.accent.color
        } else {
            colors.surface_secondary
        },
        if is_connecting { 0.16 } else { 0.85 },
    );
    let pill_border = if is_connecting {
        with_alpha(colors.accent.color, 0.45)
    } else {
        hairline_border_color(cx)
    };
    div()
        .flex_shrink_0()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .px(px(8.0))
                .py(px(3.0))
                .rounded(px(99.0))
                .bg(pill_bg)
                .border(px(HAIRLINE_BORDER_PX))
                .border_color(pill_border)
                .text_size(px(TEXT_BADGE_PX))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if is_connecting {
                    colors.accent.color
                } else {
                    colors.muted
                })
                .child(if is_connecting {
                    "Connecting…"
                } else {
                    engine.display_name()
                }),
        )
        .child(
            Icon::new(IconName::ChevronRight)
                .size(IconSize::Xs)
                .color(colors.muted),
        )
        .into_any_element()
}

fn wrap_connection_row_context_menu(
    row_el: impl IntoElement,
    conn_id: &str,
    is_favorite: bool,
    idx: usize,
    cx: &mut Context<TableMinusWorkspace>,
) -> ContextMenu {
    let ctx_connect_id = conn_id.to_owned();
    let ctx_fav_id = conn_id.to_owned();
    let fav_label = if is_favorite {
        "Remove from Favorites"
    } else {
        "Add to Favorites"
    };
    let entity_ctx = cx.entity().clone();
    ContextMenu::new(
        SharedString::from(format!("welcome-conn-ctx-{idx}")),
        row_el,
        vec![
            MenuItem::new("connect", "Connect").icon(IconName::ArrowRight.path()),
            MenuItem::new("favorite", fav_label).icon(IconName::Star.path()),
        ],
    )
    .on_action(
        move |action_id: &SharedString, _w, cx| match action_id.as_ref() {
            "connect" => {
                let id = ctx_connect_id.clone();
                entity_ctx.update(cx, |ws, cx| {
                    connect_welcome_connection(ws, &id, cx);
                });
            }
            "favorite" => {
                let id = ctx_fav_id.clone();
                entity_ctx.update(cx, |ws, cx| {
                    ws.controller.toggle_library_favorite(&id);
                    cx.notify();
                });
            }
            _ => {}
        },
    )
}

fn render_engine_badge_sized(
    engine: EngineKind,
    badge_bg: Hsla,
    size_px: f32,
    icon_px: f32,
    circular: bool,
) -> AnyElement {
    div()
        .w(px(size_px))
        .h(px(size_px))
        .rounded(px(if circular { size_px * 0.5 } else { 8.0 }))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(badge_bg)
        .child(engine_icon(engine, px(icon_px), rgb(0xFFFFFF).into()))
        .into_any_element()
}

fn render_engine_badge(engine: EngineKind, badge_bg: Hsla, circular: bool) -> AnyElement {
    render_engine_badge_sized(engine, badge_bg, 36.0, 19.0, circular)
}

fn render_create_connection_modal(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let closing = ws.welcome_ui.create_modal_closing;
    let nonce = ws.welcome_ui.modal_anim_nonce;
    let card_content = match ws.welcome_ui.create_step {
        CreateConnectionStep::EnginePicker => render_engine_picker_step(ws, cx),
        CreateConnectionStep::ConnectionForm => render_connection_form_step(ws, cx),
    };

    let card = div()
        .id("welcome-create-modal-card")
        .w(px(660.0))
        .max_w(px(700.0))
        .max_h(px(700.0))
        .rounded(px(12.0))
        .bg(elevated_overlay_bg(cx))
        .border(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .shadow_lg()
        .overflow_hidden()
        .flex()
        .flex_col()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .child(card_content);

    let animated_card = if closing {
        animate_modal_exit(card, ("welcome-modal-exit", nonce), cx)
    } else {
        animate_modal_enter(card, ("welcome-modal-enter", nonce), cx)
    };

    let backdrop = div()
        .id("welcome-create-modal-backdrop")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(with_alpha(colors.background, 0.62))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|ws, _, _, cx| {
                cx.stop_propagation();
                begin_close_create_modal(ws, cx);
            }),
        )
        .child(animated_card);

    animate_backdrop_fade(
        backdrop,
        (
            "welcome-modal-backdrop-anim",
            (nonce << 1) | u64::from(closing),
        ),
        closing,
        cx,
    )
}

fn render_engine_picker_step(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let query = ws
        .welcome_ui
        .engine_search_query
        .trim()
        .to_ascii_lowercase();
    let filtered: Vec<EngineKind> = MODAL_ENGINES
        .iter()
        .copied()
        .filter(|e| {
            if query.is_empty() {
                return true;
            }
            let spec = driver_form_spec(*e);
            e.display_name().to_ascii_lowercase().contains(&query)
                || spec.short_kind.to_ascii_lowercase().contains(&query)
        })
        .collect();

    let header = render_modal_header(
        "Create a New Connection",
        Some("Choose one of 9 supported database engines"),
        "Step 1 of 2",
        None,
        cx,
    );
    let search = SearchField::new(&ws.welcome_ui.engine_search_input)
        .placeholder("Filter drivers (e.g. Postgres, SQLite, Redis, DuckDB)...")
        .variant(FieldVariant::Secondary)
        .full_width()
        .on_change(cx.listener(|ws, val: &str, _, cx| {
            ws.welcome_ui.engine_search_query = val.to_owned();
            cx.notify();
        }));

    let mut grid = div()
        .id("welcome-engine-picker-grid")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .px(px(22.0))
        .py(px(14.0))
        .flex()
        .flex_row()
        .flex_wrap()
        .gap(px(10.0));

    if filtered.is_empty() {
        grid = grid.child(render_engine_picker_empty(cx));
    } else {
        for (idx, engine) in filtered.into_iter().enumerate() {
            grid = grid.child(render_engine_picker_card(ws, engine, idx, cx));
        }
    }

    let step_body = div()
        .w_full()
        .flex()
        .flex_col()
        .text_color(colors.foreground)
        .child(header)
        .child(div().px(px(22.0)).pt(px(14.0)).child(search))
        .child(grid)
        .child(render_engine_picker_footer(cx));

    if ws.welcome_ui.step_transition_forward {
        step_body.into_any_element()
    } else {
        animate_step_slide(
            step_body,
            ("welcome-step-picker", ws.welcome_ui.modal_anim_nonce),
            false,
            cx,
        )
    }
}

fn render_engine_picker_empty(cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .w_full()
        .py(px(40.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .child(
            Icon::new(IconName::Search)
                .size(IconSize::Md)
                .color(colors.muted),
        )
        .child(
            div()
                .text_size(px(TEXT_BODY_PX))
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.foreground)
                .child("No matching database driver"),
        )
        .child(
            div()
                .text_size(px(TEXT_META_PX))
                .text_color(colors.muted)
                .child("Try searching PostgreSQL, MySQL, SQLite, MongoDB, Redis, or ClickHouse."),
        )
        .into_any_element()
}

fn render_engine_picker_footer(cx: &mut Context<TableMinusWorkspace>) -> AnyElement {
    div()
        .w_full()
        .px(px(22.0))
        .py(px(14.0))
        .bg(with_alpha(muted_surface_bg(cx), 0.5))
        .border_t(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            Button::new("welcome-modal-import-conn")
                .variant(Variant::Ghost)
                .size(HeroSize::Md)
                .on_press(cx.listener(|ws, _, _, cx| {
                    ws.welcome_ui.close_create_modal();
                    trigger_restore_import(ws, cx);
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(Icon::new(IconName::Upload).size(IconSize::Xs))
                        .child("Import Connection"),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    Button::new("welcome-modal-cancel")
                        .variant(Variant::Secondary)
                        .size(HeroSize::Md)
                        .on_press(cx.listener(|ws, _, _, cx| {
                            begin_close_create_modal(ws, cx);
                        }))
                        .child("Cancel"),
                )
                .child(
                    Button::new("welcome-modal-next")
                        .variant(Variant::Primary)
                        .size(HeroSize::Md)
                        .on_press(cx.listener(|ws, _, _, cx| {
                            advance_to_form_step(ws, cx);
                        }))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.0))
                                .child("Continue")
                                .child(Icon::new(IconName::ArrowRight).size(IconSize::Xs)),
                        ),
                ),
        )
        .into_any_element()
}

fn render_engine_picker_card(
    ws: &TableMinusWorkspace,
    engine: EngineKind,
    idx: usize,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let is_selected = ws.welcome_ui.selected_engine == engine;
    let (_badge_code, badge_bg) = engine_badge_style(engine);
    let spec = driver_form_spec(engine);
    let border_col = if is_selected {
        colors.accent.color
    } else {
        hairline_border_color(cx)
    };
    let card_bg = if is_selected {
        with_alpha(colors.accent.color, 0.12)
    } else {
        with_alpha(colors.surface_secondary, 0.55)
    };

    let card = div()
        .id(SharedString::from(format!(
            "welcome-engine-card-{}",
            engine.display_name()
        )))
        .relative()
        .w(px(198.0))
        .px(px(12.0))
        .py(px(12.0))
        .rounded(px(10.0))
        .border(px(if is_selected { 1.5 } else { HAIRLINE_BORDER_PX }))
        .border_color(border_col)
        .bg(card_bg)
        .hover(|s| {
            s.bg(with_alpha(colors.accent.color, 0.10))
                .border_color(with_alpha(colors.accent.color, 0.5))
        })
        .cursor_pointer()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(
                move |ws, event: &herogpui::gpui::MouseDownEvent, _window, cx| {
                    ws.welcome_ui.selected_engine = engine;
                    if event.click_count >= 2 {
                        advance_to_form_step(ws, cx);
                    } else {
                        cx.notify();
                    }
                },
            ),
        )
        .child(render_engine_badge_sized(
            engine, badge_bg, 36.0, 19.0, false,
        ))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .truncate()
                        .text_size(px(TEXT_BODY_PX))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(engine.display_name()),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(TEXT_BADGE_PX))
                        .text_color(colors.muted)
                        .child(spec.short_kind),
                ),
        )
        .when(is_selected, |d| {
            d.child(render_selected_check_badge(colors.accent.color))
        });

    animate_staggered_enter(
        card,
        (
            "welcome-engine-card-anim",
            (ws.welcome_ui.modal_anim_nonce << 8) | (idx as u64),
        ),
        idx,
        cx,
    )
}

fn render_selected_check_badge(accent: Hsla) -> AnyElement {
    div()
        .w(px(18.0))
        .h(px(18.0))
        .rounded(px(9.0))
        .bg(accent)
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            Icon::new(IconName::Check)
                .size(IconSize::Xs)
                .color(rgb(0xFFFFFF)),
        )
        .into_any_element()
}

fn render_connection_form_step(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let engine = ws.welcome_ui.selected_engine;
    let spec = driver_form_spec(engine);
    let title = format!("New {} Connection", engine.display_name());

    let options_section = div()
        .w_full()
        .p(px(14.0))
        .rounded(px(10.0))
        .bg(with_alpha(muted_surface_bg(cx), 0.52))
        .border(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(render_color_and_tag_row(ws, cx))
        .child(render_switches_row(ws, spec.supports_tls, cx));

    let mut form_scroll = div()
        .id("welcome-create-form-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .px(px(22.0))
        .py(px(16.0))
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(render_form_field(
            "Connection Name",
            Input::new(&ws.welcome_ui.form_name_input)
                .placeholder(spec.name_placeholder)
                .full_width(),
            cx,
        ))
        .child(render_connection_credentials_section(ws, spec, engine, cx))
        .child(options_section);

    if let Some(ref test_res) = ws.welcome_ui.form_test_result {
        form_scroll = form_scroll.child(render_test_result_banner(test_res, cx));
    }

    let step_body = div()
        .w_full()
        .flex()
        .flex_col()
        .text_color(colors.foreground)
        .child(render_modal_header(
            &title,
            Some(spec.category_tag),
            "Step 2 of 2",
            Some(engine),
            cx,
        ))
        .child(form_scroll)
        .child(render_form_footer(ws, cx));

    animate_step_slide(
        step_body,
        ("welcome-step-form", ws.welcome_ui.modal_anim_nonce),
        ws.welcome_ui.step_transition_forward,
        cx,
    )
}

fn render_connection_credentials_section(
    ws: &TableMinusWorkspace,
    spec: DriverFormSpec,
    engine: EngineKind,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let card = div()
        .w_full()
        .p(px(14.0))
        .rounded(px(10.0))
        .bg(with_alpha(muted_surface_bg(cx), 0.52))
        .border(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .flex()
        .flex_col()
        .gap(px(12.0));

    let populated = if spec.is_file {
        card.child(render_file_path_row(ws, spec, cx))
            .child(render_form_field(
                spec.password_label,
                Input::new(&ws.welcome_ui.form_password_input)
                    .input_type(InputType::Password)
                    .placeholder(spec.password_placeholder)
                    .full_width(),
                cx,
            ))
    } else {
        card.child(render_host_port_row(ws, spec, engine, cx))
            .child(render_user_password_row(ws, spec, cx))
            .child(render_form_field(
                spec.database_label,
                Input::new(&ws.welcome_ui.form_database_input)
                    .placeholder(spec.database_placeholder)
                    .full_width(),
                cx,
            ))
    };

    populated
        .child(
            div()
                .text_size(px(TEXT_BADGE_PX))
                .text_color(colors.muted)
                .child(spec.helper_note),
        )
        .into_any_element()
}

fn render_modal_header(
    title: &str,
    subtitle: Option<&str>,
    step_label: &'static str,
    engine: Option<EngineKind>,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let leading_badge = if let Some(eng) = engine {
        let (_code, badge_bg) = engine_badge_style(eng);
        render_engine_badge_sized(eng, badge_bg, 34.0, 18.0, false)
    } else {
        div()
            .w(px(34.0))
            .h(px(34.0))
            .rounded(px(8.0))
            .bg(with_alpha(colors.accent.color, 0.14))
            .border(px(HAIRLINE_BORDER_PX))
            .border_color(with_alpha(colors.accent.color, 0.35))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .child(
                Icon::new(IconName::Database)
                    .size(IconSize::Sm)
                    .color(colors.accent.color),
            )
            .into_any_element()
    };

    div()
        .w_full()
        .px(px(22.0))
        .py(px(15.0))
        .border_b(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .child(leading_badge)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .child(
                            div()
                                .truncate()
                                .text_size(px(15.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(colors.foreground)
                                .child(title.to_owned()),
                        )
                        .when_some(subtitle, |d, sub| {
                            d.child(
                                div()
                                    .truncate()
                                    .text_size(px(TEXT_META_PX))
                                    .text_color(colors.muted)
                                    .child(sub.to_owned()),
                            )
                        }),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .px(px(8.0))
                        .py(px(3.0))
                        .rounded(px(99.0))
                        .bg(with_alpha(colors.surface_secondary, 0.85))
                        .border(px(HAIRLINE_BORDER_PX))
                        .border_color(hairline_border_color(cx))
                        .text_size(px(TEXT_BADGE_PX))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.muted)
                        .child(step_label),
                )
                .child(
                    Button::new("welcome-modal-close-x")
                        .variant(Variant::Ghost)
                        .size(HeroSize::Sm)
                        .on_press(cx.listener(|ws, _, _, cx| {
                            begin_close_create_modal(ws, cx);
                        }))
                        .child(Icon::new(IconName::X).size(IconSize::Sm)),
                ),
        )
        .into_any_element()
}

fn render_form_field(
    label: &'static str,
    input: Input,
    cx: &Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .child(
            div()
                .text_size(px(TEXT_META_PX))
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.muted)
                .child(label),
        )
        .child(div().w_full().child(input))
        .into_any_element()
}

fn render_file_path_row(
    ws: &TableMinusWorkspace,
    spec: DriverFormSpec,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .child(
            div()
                .text_size(px(TEXT_META_PX))
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.muted)
                .child(spec.database_label),
        )
        .child(
            div()
                .w_full()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    div().flex_1().min_w(px(0.0)).child(
                        Input::new(&ws.welcome_ui.form_database_input)
                            .placeholder(spec.database_placeholder)
                            .full_width(),
                    ),
                )
                .child(
                    Button::new("welcome-form-select-file")
                        .variant(Variant::Secondary)
                        .size(HeroSize::Md)
                        .on_press(cx.listener(|_ws, _, _, cx| {
                            trigger_select_database_file(cx);
                        }))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(5.0))
                                .child(Icon::new(IconName::Folder).size(IconSize::Xs))
                                .child("Browse…"),
                        ),
                ),
        )
        .into_any_element()
}

fn render_host_port_row(
    ws: &TableMinusWorkspace,
    spec: DriverFormSpec,
    engine: EngineKind,
    cx: &Context<TableMinusWorkspace>,
) -> AnyElement {
    let default_port = default_port_for_engine(engine).to_string();
    div()
        .w_full()
        .flex()
        .flex_row()
        .gap(px(10.0))
        .child(
            div().flex_1().min_w(px(0.0)).child(render_form_field(
                "Host",
                Input::new(&ws.welcome_ui.form_host_input)
                    .placeholder(spec.host_placeholder)
                    .full_width(),
                cx,
            )),
        )
        .child(
            div().w(px(110.0)).flex_shrink_0().child(render_form_field(
                "Port",
                Input::new(&ws.welcome_ui.form_port_input)
                    .placeholder(default_port)
                    .full_width(),
                cx,
            )),
        )
        .into_any_element()
}

fn render_user_password_row(
    ws: &TableMinusWorkspace,
    spec: DriverFormSpec,
    cx: &Context<TableMinusWorkspace>,
) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_row()
        .gap(px(10.0))
        .child(
            div().flex_1().min_w(px(0.0)).child(render_form_field(
                spec.user_label,
                Input::new(&ws.welcome_ui.form_user_input)
                    .placeholder(spec.user_placeholder)
                    .full_width(),
                cx,
            )),
        )
        .child(
            div().flex_1().min_w(px(0.0)).child(render_form_field(
                spec.password_label,
                Input::new(&ws.welcome_ui.form_password_input)
                    .input_type(InputType::Password)
                    .placeholder(spec.password_placeholder)
                    .full_width(),
                cx,
            )),
        )
        .into_any_element()
}

fn render_color_and_tag_row(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let swatches = render_color_swatches(ws.welcome_ui.form_color, cx);

    let mut tag_pills = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(2.0))
        .p(px(3.0))
        .rounded(px(8.0))
        .bg(colors.background)
        .border(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx));

    for (idx, &env) in EnvironmentKind::ALL.iter().enumerate() {
        let active = ws.welcome_ui.form_tag == env;
        tag_pills = tag_pills.child(
            div()
                .id(SharedString::from(format!("welcome-env-tag-{idx}")))
                .px(px(10.0))
                .py(px(4.0))
                .rounded(px(6.0))
                .cursor_pointer()
                .bg(if active {
                    colors.accent.color
                } else {
                    with_alpha(colors.surface_secondary, 0.0)
                })
                .hover(|s| {
                    if active {
                        s
                    } else {
                        s.bg(with_alpha(colors.surface_secondary, 0.8))
                    }
                })
                .text_size(px(TEXT_BADGE_PX))
                .font_weight(if active {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(if active {
                    rgb(0xFFFFFF).into()
                } else {
                    colors.muted
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |ws, _, _, cx| {
                        ws.welcome_ui.form_tag = env;
                        cx.notify();
                    }),
                )
                .child(env_label(env)),
        );
    }

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(TEXT_META_PX))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.muted)
                        .child("Status color"),
                )
                .child(swatches),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(TEXT_META_PX))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.muted)
                        .child("Environment"),
                )
                .child(tag_pills),
        )
        .into_any_element()
}

fn render_color_swatches(
    selected: Option<ConnectionColor>,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let mut swatches = div().flex().flex_row().items_center().gap(px(6.0)).child(
        div()
            .id("welcome-color-none")
            .w(px(20.0))
            .h(px(20.0))
            .rounded(px(10.0))
            .border(px(if selected.is_none() { 2.0 } else { 1.0 }))
            .border_color(if selected.is_none() {
                colors.accent.color
            } else {
                hairline_border_color(cx)
            })
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|ws, _, _, cx| {
                    ws.welcome_ui.form_color = None;
                    cx.notify();
                }),
            )
            .when(selected.is_none(), |d| {
                d.child(
                    div()
                        .w(px(6.0))
                        .h(px(6.0))
                        .rounded(px(3.0))
                        .bg(colors.accent.color),
                )
            }),
    );

    for (idx, &col) in ConnectionColor::ALL.iter().enumerate() {
        let active = selected == Some(col);
        swatches = swatches.child(
            div()
                .id(SharedString::from(format!("welcome-color-{idx}")))
                .w(px(20.0))
                .h(px(20.0))
                .rounded(px(10.0))
                .bg(connection_color_hsla(col))
                .border(px(if active { 2.0 } else { 1.0 }))
                .border_color(if active {
                    colors.foreground
                } else {
                    hairline_border_color(cx)
                })
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |ws, _, _, cx| {
                        ws.welcome_ui.form_color = Some(col);
                        cx.notify();
                    }),
                )
                .when(active, |d| {
                    d.child(
                        div()
                            .w(px(6.0))
                            .h(px(6.0))
                            .rounded(px(3.0))
                            .bg(rgb(0xFFFFFF)),
                    )
                }),
        );
    }
    swatches.into_any_element()
}

fn render_switches_row(
    ws: &TableMinusWorkspace,
    show_tls: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let mut row = div()
        .w_full()
        .pt(px(2.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(24.0))
        .child(
            Switch::new("welcome-create-safe-mode")
                .size(HeroSize::Sm)
                .is_selected(ws.welcome_ui.form_safe_mode)
                .label("Safe mode (confirm writes)")
                .on_change(cx.listener(|ws, selected: &bool, _, cx| {
                    ws.welcome_ui.form_safe_mode = *selected;
                    cx.notify();
                })),
        );

    if show_tls {
        row = row.child(
            Switch::new("welcome-create-tls")
                .size(HeroSize::Sm)
                .is_selected(ws.welcome_ui.form_tls_enabled)
                .label("Require TLS / SSL")
                .on_change(cx.listener(|ws, selected: &bool, _, cx| {
                    ws.welcome_ui.form_tls_enabled = *selected;
                    cx.notify();
                })),
        );
    }

    row.into_any_element()
}

fn render_test_result_banner(
    result: &Result<String, String>,
    cx: &Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let (ok, msg) = match result {
        Ok(m) => (true, m.as_str()),
        Err(e) => (false, e.as_str()),
    };
    let tint: Hsla = if ok {
        rgb(0x10B981).into()
    } else {
        rgb(0xEF4444).into()
    };
    let icon = if ok {
        IconName::CircleCheck
    } else {
        IconName::TriangleAlert
    };
    let banner = div()
        .w_full()
        .px(px(12.0))
        .py(px(9.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(with_alpha(tint, 0.12))
        .border(px(HAIRLINE_BORDER_PX))
        .border_color(with_alpha(tint, 0.45))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(Icon::new(icon).size(IconSize::Sm).color(tint))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(TEXT_META_PX))
                .text_color(colors.foreground)
                .child(msg.to_owned()),
        );
    animate_fade_in(banner, "welcome-test-banner-anim", 140, cx)
}

fn render_form_footer(
    _ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    div()
        .w_full()
        .px(px(22.0))
        .py(px(14.0))
        .bg(with_alpha(muted_surface_bg(cx), 0.5))
        .border_t(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            Button::new("welcome-form-back")
                .variant(Variant::Secondary)
                .size(HeroSize::Md)
                .on_press(cx.listener(|ws, _, _, cx| {
                    ws.welcome_ui.step_transition_forward = false;
                    ws.welcome_ui.modal_anim_nonce = ws.welcome_ui.modal_anim_nonce.wrapping_add(1);
                    ws.welcome_ui.create_step = CreateConnectionStep::EnginePicker;
                    ws.welcome_ui.form_test_result = None;
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(5.0))
                        .child(Icon::new(IconName::ArrowLeft).size(IconSize::Xs))
                        .child("Back"),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    Button::new("welcome-form-test")
                        .variant(Variant::Secondary)
                        .size(HeroSize::Md)
                        .on_press(cx.listener(|ws, _, _, cx| {
                            test_modal_connection(ws, cx);
                        }))
                        .child("Test Connection"),
                )
                .child(
                    Button::new("welcome-form-save")
                        .variant(Variant::Secondary)
                        .size(HeroSize::Md)
                        .on_press(cx.listener(|ws, _, _, cx| {
                            if save_modal_connection(ws, cx).is_some() {
                                begin_close_create_modal(ws, cx);
                            }
                        }))
                        .child("Save"),
                )
                .child(
                    Button::new("welcome-form-connect")
                        .variant(Variant::Primary)
                        .size(HeroSize::Md)
                        .on_press(cx.listener(|ws, _, _, cx| {
                            if let Some(conn_id) = save_modal_connection(ws, cx) {
                                ws.welcome_ui.close_create_modal();
                                connect_welcome_connection(ws, &conn_id, cx);
                            }
                        }))
                        .child("Connect"),
                ),
        )
        .into_any_element()
}

fn open_create_connection_modal(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) {
    ws.welcome_ui.open_create_modal(cx);
    cx.notify();
}

fn begin_close_create_modal(ws: &mut TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) {
    if !ws.welcome_ui.create_modal_open || ws.welcome_ui.create_modal_closing {
        return;
    }
    let exit_ms = effective_duration_ms(OVERLAY_EXIT_MS, cx);
    if exit_ms == 0 {
        ws.welcome_ui.close_create_modal();
        cx.notify();
        return;
    }
    ws.welcome_ui.create_modal_closing = true;
    let close_nonce = ws.welcome_ui.modal_anim_nonce;
    cx.notify();
    cx.spawn(
        async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
            cx.background_executor()
                .timer(Duration::from_millis(exit_ms))
                .await;
            let _ = this.update(cx, |ws, cx| {
                if ws.welcome_ui.create_modal_closing
                    && ws.welcome_ui.modal_anim_nonce == close_nonce
                {
                    ws.welcome_ui.close_create_modal();
                    cx.notify();
                }
            });
        },
    )
    .detach();
}

fn advance_to_form_step(ws: &mut TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) {
    let engine = ws.welcome_ui.selected_engine;
    let spec = driver_form_spec(engine);
    let default_port = default_port_for_engine(engine);
    set_input_value(&ws.welcome_ui.form_name_input, "", cx);
    set_input_value(&ws.welcome_ui.form_host_input, "localhost", cx);
    set_input_value(
        &ws.welcome_ui.form_port_input,
        &default_port.to_string(),
        cx,
    );
    set_input_value(&ws.welcome_ui.form_user_input, "", cx);
    set_input_value(&ws.welcome_ui.form_password_input, "", cx);
    set_input_value(
        &ws.welcome_ui.form_database_input,
        spec.default_database,
        cx,
    );
    ws.welcome_ui.form_test_result = None;
    ws.welcome_ui.step_transition_forward = true;
    ws.welcome_ui.modal_anim_nonce = ws.welcome_ui.modal_anim_nonce.wrapping_add(1);
    ws.welcome_ui.create_step = CreateConnectionStep::ConnectionForm;
    cx.notify();
}

fn trigger_select_database_file(cx: &mut Context<TableMinusWorkspace>) {
    let rx = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Select Database File".into()),
    });
    cx.spawn(
        async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let value = path.to_string_lossy().into_owned();
            let _ = this.update(cx, |ws, cx| {
                set_input_value(&ws.welcome_ui.form_database_input, &value, cx);
                if ws
                    .welcome_ui
                    .form_name_input
                    .read(cx)
                    .value()
                    .trim()
                    .is_empty()
                    && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                {
                    set_input_value(&ws.welcome_ui.form_name_input, stem, cx);
                }
                cx.notify();
            });
        },
    )
    .detach();
}

fn build_modal_draft(ws: &TableMinusWorkspace, cx: &App) -> ConnectionFormDraft {
    let engine = ws.welcome_ui.selected_engine;
    let default_port = default_port_for_engine(engine);
    let port = ws
        .welcome_ui
        .form_port_input
        .read(cx)
        .value()
        .trim()
        .parse::<u16>()
        .unwrap_or(default_port);

    ConnectionFormDraft {
        name: ws
            .welcome_ui
            .form_name_input
            .read(cx)
            .value()
            .trim()
            .to_owned(),
        engine,
        host: ws
            .welcome_ui
            .form_host_input
            .read(cx)
            .value()
            .trim()
            .to_owned(),
        port,
        database: ws
            .welcome_ui
            .form_database_input
            .read(cx)
            .value()
            .trim()
            .to_owned(),
        username: ws
            .welcome_ui
            .form_user_input
            .read(cx)
            .value()
            .trim()
            .to_owned(),
        auth_mode: AuthMode::Password,
        tls_mode: if ws.welcome_ui.form_tls_enabled {
            TlsMode::Require
        } else {
            TlsMode::Prefer
        },
        ssh_enabled: false,
        ssh_host: String::new(),
        ssh_port: 22,
        ssh_user: String::new(),
        ssh_key_path: String::new(),
        url_input: String::new(),
        last_error: None,
    }
}

fn test_modal_connection(ws: &mut TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) {
    let draft = build_modal_draft(ws, cx);
    let pwd_str = ws
        .welcome_ui
        .form_password_input
        .read(cx)
        .value()
        .to_owned();
    let password = (!pwd_str.is_empty()).then(|| SecretString::from(pwd_str));
    ws.controller.connection_form = draft;
    match ws.controller.test_connection_blocking(password) {
        Ok(()) => {
            ws.welcome_ui.form_test_result = Some(Ok("Connection succeeded.".to_owned()));
            push_success_toast("Connection Test Passed", None, cx);
        }
        Err(err) => {
            let msg = err.to_string();
            ws.welcome_ui.form_test_result = Some(Err(msg.clone()));
            push_error_toast("Connection Test Failed", Some(SharedString::from(msg)), cx);
        }
    }
    cx.notify();
}

fn save_modal_connection(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> Option<String> {
    let draft = build_modal_draft(ws, cx);
    let display_name = if draft.name.is_empty() {
        if draft.database.is_empty() {
            format!("{} Connection", draft.engine.display_name())
        } else {
            format!("{} ({})", draft.engine.display_name(), draft.database)
        }
    } else {
        draft.name.clone()
    };

    let uuid = Uuid::new_v4();
    let conn_id = uuid.to_string();
    let saved = SavedConnection {
        id: uuid,
        name: display_name,
        driver_id: driver_id_from_engine(draft.engine).to_owned(),
        host: draft.host.clone(),
        port: draft.port,
        database: draft.database.clone(),
        username: draft.username.clone(),
        tls: {
            let mut tls = TlsConfig::disabled();
            tls.mode = if ws.welcome_ui.form_tls_enabled {
                CoreTlsMode::Require
            } else {
                CoreTlsMode::Prefer
            };
            tls
        },
        safe_mode: if ws.welcome_ui.form_safe_mode {
            SafeModeLevel::ConfirmWrites
        } else {
            ws.controller.active_safe_mode()
        },
        external_access: ExternalAccess::ReadOnly,
        ai_policy: AiPolicy::Ask,
        query_timeout_secs: None,
        auth_mode: CoreAuthMode::Password,
        ssh: None,
        last_opened_at: None,
        color: ws.welcome_ui.form_color,
        group_id: None,
        order: 0,
        tags: Vec::new(),
        icon: None,
        options: NetworkOptions::default(),
    };

    let Some(services) = ws.controller.services.clone() else {
        push_error_toast(
            "Save Failed",
            Some("Services are not initialized.".into()),
            cx,
        );
        return None;
    };
    if let Err(err) = services.connection_store.upsert_blocking(saved.clone()) {
        push_error_toast("Save Failed", Some(SharedString::from(err.to_string())), cx);
        return None;
    }

    let pwd = ws
        .welcome_ui
        .form_password_input
        .read(cx)
        .value()
        .to_owned();
    if !pwd.is_empty() {
        let label = saved.secret_target(SecretKind::DatabasePassword);
        let creds = services.credentials.clone();
        let _ = services.runtime.block_on(async move {
            creds
                .store(
                    saved.id,
                    SecretKind::DatabasePassword,
                    &SecretString::from(pwd),
                    &label,
                )
                .await
        });
    }

    ws.controller
        .set_connection_environment(&conn_id, ws.welcome_ui.form_tag);
    hydrate_controller_library(ws);
    ws.welcome_ui.selected_connection_id = Some(conn_id.clone());
    ws.welcome_ui.mark_dirty();
    push_success_toast("Connection Saved", Some(SharedString::from(saved.name)), cx);
    Some(conn_id)
}

fn connect_welcome_connection(
    ws: &mut TableMinusWorkspace,
    conn_id: &str,
    cx: &mut Context<TableMinusWorkspace>,
) {
    ws.welcome_ui.selected_connection_id = Some(conn_id.to_owned());
    ws.welcome_ui.connecting_connection_id = Some(conn_id.to_owned());

    if let Err(err) = validate_saved_connection_target(ws, conn_id) {
        ws.welcome_ui.connecting_connection_id = None;
        push_error_toast("Connection Unavailable", Some(SharedString::from(err)), cx);
        cx.notify();
        return;
    }

    match ws.controller.connect_library_item_blocking(conn_id) {
        Ok(()) => {
            ws.welcome_ui.close();
            ws.switch_screen(ScreenMode::Workspace, cx);
            ws.save_session_snapshot_now(cx);
        }
        Err(err) => {
            ws.welcome_ui.connecting_connection_id = None;
            ws.controller.restoring_connection_id = None;
            push_error_toast(
                ws.controller.strings.connection_failed,
                Some(SharedString::from(err.to_string())),
                cx,
            );
            cx.notify();
        }
    }
}

fn validate_saved_connection_target(ws: &TableMinusWorkspace, conn_id: &str) -> Result<(), String> {
    let key = LibraryConnectionId(conn_id.to_owned());
    let Some(conn) = ws.controller.library.storage().connections.get(&key) else {
        return Err(format!("Saved connection '{conn_id}' no longer exists."));
    };
    let engine = engine_from_driver_id(&conn.driver_id);
    if matches!(engine, EngineKind::Sqlite | EngineKind::Duckdb) {
        let db_path = conn.database.trim();
        if !db_path.is_empty() && db_path != ":memory:" && !Path::new(db_path).exists() {
            return Err(format!("Database file not found: {db_path}"));
        }
    }
    Ok(())
}

fn trigger_backup_export(ws: &mut TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) {
    let Some(services) = ws.controller.services.clone() else {
        push_error_toast(
            "Backup Failed",
            Some("Services are not initialized.".into()),
            cx,
        );
        return;
    };
    let connections = match services.connection_store.load_blocking() {
        Ok(list) => list.to_vec(),
        Err(err) => {
            push_error_toast(
                "Backup Failed",
                Some(SharedString::from(err.to_string())),
                cx,
            );
            return;
        }
    };

    let start_dir = storage_paths()
        .map(|p| p.data)
        .unwrap_or_else(|_| PathBuf::from("."));
    let rx = cx.prompt_for_new_path(&start_dir, Some("tableminus_connections_backup.json"));
    cx.spawn(
        async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(path))) = rx.await else {
                return;
            };
            let result = serde_json::to_vec_pretty(&connections)
                .map_err(|err| err.to_string())
                .and_then(|bytes| std::fs::write(&path, bytes).map_err(|err| err.to_string()));
            let _ = this.update(cx, |ws, cx| {
                match result {
                    Ok(()) => {
                        push_success_toast(
                            "Connections Exported",
                            Some(SharedString::from(format!(
                                "Saved {} connection(s) to {}",
                                connections.len(),
                                path.display()
                            ))),
                            cx,
                        );
                    }
                    Err(err) => {
                        push_error_toast("Backup Failed", Some(SharedString::from(err)), cx);
                    }
                }
                let _ = ws;
                cx.notify();
            });
        },
    )
    .detach();
}

fn trigger_restore_import(_ws: &mut TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) {
    let rx = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some(
            "Select Connections Backup (.json) or Local Database (.sqlite / .db / .duckdb)".into(),
        ),
    });

    cx.spawn(
        async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |ws, cx| {
                let Some(services) = ws.controller.services.clone() else {
                    push_error_toast(
                        "Restore Failed",
                        Some("Services are not initialized.".into()),
                        cx,
                    );
                    return;
                };
                if let Some(engine) = local_db_engine_from_path(&path) {
                    import_local_db_file(ws, &path, engine, cx);
                    cx.notify();
                    return;
                }
                let parsed = std::fs::read(&path)
                    .map_err(|err| err.to_string())
                    .and_then(|bytes| parse_backup_connections(&bytes));

                match parsed {
                    Ok(connections) => {
                        let mut imported = 0usize;
                        for conn in connections {
                            if services.connection_store.upsert_blocking(conn).is_ok() {
                                imported += 1;
                            }
                        }
                        hydrate_controller_library(ws);
                        ws.welcome_ui.mark_dirty();
                        push_success_toast(
                            "Connections Restored",
                            Some(SharedString::from(format!(
                                "Imported {imported} connection(s) from {}",
                                path.display()
                            ))),
                            cx,
                        );
                    }
                    Err(err) => {
                        push_error_toast("Restore Failed", Some(SharedString::from(err)), cx);
                    }
                }
                cx.notify();
            });
        },
    )
    .detach();
}

fn import_local_db_file(
    ws: &mut TableMinusWorkspace,
    path: &Path,
    engine: EngineKind,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let Some(services) = ws.controller.services.clone() else {
        return;
    };
    let display_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Local Database")
        .to_owned();
    let uuid = Uuid::new_v4();
    let id = uuid.to_string();
    let saved = SavedConnection {
        id: uuid,
        name: display_name.clone(),
        driver_id: driver_id_from_engine(engine).to_owned(),
        host: String::new(),
        port: 0,
        database: path.to_string_lossy().into_owned(),
        username: String::new(),
        tls: TlsConfig::disabled(),
        safe_mode: ws.controller.active_safe_mode(),
        external_access: ExternalAccess::ReadOnly,
        ai_policy: AiPolicy::Ask,
        query_timeout_secs: None,
        auth_mode: CoreAuthMode::Password,
        ssh: None,
        last_opened_at: None,
        color: Some(ConnectionColor::Teal),
        group_id: None,
        order: 0,
        tags: Vec::new(),
        icon: None,
        options: Default::default(),
    };
    match services.connection_store.upsert_blocking(saved) {
        Ok(_) => {
            hydrate_controller_library(ws);
            ws.welcome_ui.mark_dirty();
            connect_welcome_connection(ws, &id, cx);
            push_success_toast(
                "Database Imported",
                Some(SharedString::from(format!("Opened {display_name}"))),
                cx,
            );
        }
        Err(err) => {
            push_error_toast(
                "Import Failed",
                Some(SharedString::from(err.to_string())),
                cx,
            );
        }
    }
}

fn local_db_engine_from_path(path: &Path) -> Option<EngineKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "sqlite" | "sqlite3" | "db" => Some(EngineKind::Sqlite),
        "duckdb" => Some(EngineKind::Duckdb),
        _ => None,
    }
}

fn parse_backup_connections(bytes: &[u8]) -> Result<Vec<SavedConnection>, String> {
    if let Ok(list) = serde_json::from_slice::<Vec<SavedConnection>>(bytes) {
        return Ok(list);
    }
    #[derive(serde::Deserialize)]
    struct WrappedConnections {
        connections: Vec<SavedConnection>,
    }
    serde_json::from_slice::<WrappedConnections>(bytes)
        .map(|w| w.connections)
        .map_err(|err| format!("Invalid connection backup JSON: {err}"))
}

fn hydrate_controller_library(ws: &mut TableMinusWorkspace) {
    let Some(services) = ws.controller.services.clone() else {
        return;
    };
    let Ok(list) = services.connection_store.list_blocking() else {
        return;
    };
    apply_connection_list(ws, &list);
}

fn apply_connection_list(ws: &mut TableMinusWorkspace, list: &ConnectionList) {
    let tag_names: HashMap<Uuid, String> = list
        .tags
        .iter()
        .map(|tag| (tag.id, tag.name.as_str().to_owned()))
        .collect();

    for group in list.groups.iter() {
        ws.controller.upsert_library_folder(
            &group.id.to_string(),
            group.name.as_str(),
            group.order as usize,
        );
    }

    for saved in list.connections.iter() {
        let key = LibraryConnectionId(saved.id.to_string());
        let tags: BTreeSet<String> = saved
            .tags
            .iter()
            .filter_map(|tag_id| tag_names.get(tag_id).cloned())
            .collect();
        ws.controller.library.upsert_connection(LibraryConnection {
            id: key,
            name: saved.name.clone(),
            driver_id: saved.driver_id.clone(),
            host: saved.host.clone(),
            port: (saved.port != 0).then_some(saved.port),
            database: saved.database.clone(),
            username: saved.username.clone(),
            group_id: saved.group_id.map(|g| LibraryGroupId(g.to_string())),
            tags,
            is_favorite: list.is_favorite(saved.id),
            order: saved.order as usize,
        });
    }
}

fn format_connection_subtitle(conn: &LibraryConnection, engine: EngineKind) -> String {
    if matches!(engine, EngineKind::Sqlite | EngineKind::Duckdb) {
        let db = conn.database.trim();
        if !db.is_empty() {
            return db.to_owned();
        }
        return "Local database file".to_owned();
    }
    let host = if conn.host.trim().is_empty() {
        "localhost"
    } else {
        conn.host.trim()
    };
    let host_port = match conn.port {
        Some(port) if port != 0 => format!("{host}:{port}"),
        _ => host.to_owned(),
    };
    let db = conn.database.trim();
    if db.is_empty() {
        host_port
    } else {
        format!("{host_port} / {db}")
    }
}

fn engine_badge_style(engine: EngineKind) -> (&'static str, Hsla) {
    match engine {
        EngineKind::Sqlite => ("Sl", rgb(0x3B82F6).into()),
        EngineKind::Postgresql => ("Pg", rgb(0x336791).into()),
        EngineKind::Mysql => ("My", rgb(0x00758F).into()),
        EngineKind::Mariadb => ("Ma", rgb(0xC0765A).into()),
        EngineKind::Mssql => ("Ss", rgb(0xCC2927).into()),
        EngineKind::Duckdb => ("Du", rgb(0xD97706).into()),
        EngineKind::Redis => ("Rd", rgb(0xDC382D).into()),
        EngineKind::Mongodb => ("Mg", rgb(0x10AA50).into()),
        EngineKind::Clickhouse => ("Ch", rgb(0xEAB308).into()),
        EngineKind::Cassandra => ("Cs", rgb(0x1287B1).into()),
        EngineKind::Scylladb => ("Sc", rgb(0x52C4D0).into()),
    }
}

fn connection_color_hsla(color: ConnectionColor) -> Hsla {
    match color {
        ConnectionColor::Blue => rgb(0x3B82F6).into(),
        ConnectionColor::Teal => rgb(0x14B8A6).into(),
        ConnectionColor::Green => rgb(0x10B981).into(),
        ConnectionColor::Yellow => rgb(0xF59E0B).into(),
        ConnectionColor::Orange => rgb(0xF97316).into(),
        ConnectionColor::Red => rgb(0xEF4444).into(),
        ConnectionColor::Purple => rgb(0x8B5CF6).into(),
        ConnectionColor::Pink => rgb(0xEC4899).into(),
        ConnectionColor::Slate => rgb(0x64748B).into(),
    }
}

const fn env_label(env: EnvironmentKind) -> &'static str {
    match env {
        EnvironmentKind::Local => "local",
        EnvironmentKind::Development => "dev",
        EnvironmentKind::Staging => "staging",
        EnvironmentKind::Production => "production",
    }
}

fn set_input_value(input: &Entity<InputState>, value: &str, cx: &mut App) {
    let owned = value.to_owned();
    input.update(cx, |state, _cx| {
        state.set_value(owned);
    });
}
