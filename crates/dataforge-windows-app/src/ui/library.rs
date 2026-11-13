//! Full-screen Connections Library, URL parser, foreign import, and connection form.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use dataforge_gfx::glyphs::EngineKind;
use dataforge_services::import_bridge::{SUPPORTED_IMPORT_APPS, parse_connection_url};
use dataforge_state::library::{
    ForeignImportReport, LibraryConnection, LibraryConnectionId, LibraryGroupId, LibraryOutlineRow,
    LibraryRowID, LibrarySection,
};
use dataforge_windows_platform::foreign_app_dirs;
use herogpui::components::{
    Alert, Badge, BadgeLabel, BadgeVariant, Button, Card, CardContent, CardDescription, CardHeader,
    CardTitle, CardVariant, Checkbox, Chip, ChipLabel, ChipVariant, Color, DisclosureGroup,
    FieldVariant, Form, Icon, IconName, IconSize, Input, InputState, InputType, InteractiveState,
    ListBox, ListBoxItem, PickerItem, SearchField, Select, SelectionMode, SelectionValue, Size,
    Spinner, SpinnerSize, Switch, TabItem, Tabs, TabsSize, TabsVariant, Variant,
};
use herogpui::core::with_alpha;
use herogpui::gpui::prelude::FluentBuilder as _;
use herogpui::gpui::{
    AnyElement, App, AppContext as _, AsyncApp, Context, Entity, FontFeatures, FontWeight, Hsla,
    InteractiveElement, IntoElement, ParentElement, PathPromptOptions, SharedString,
    StatefulInteractiveElement, Styled, WeakEntity, Window, div, px,
};
use herogpui::theme::ActiveTheme;
use secrecy::{ExposeSecret, SecretString};
use tablepro_core::credentials::{CredentialInteraction, SecretKind, SecretLookup, SecretVault};
use tablepro_core::{
    AiPolicy, AuthMode as CoreAuthMode, ClientIdentity, ExternalAccess, NetworkOptions,
    SafeModeLevel, TlsConfig, TlsMode as CoreTlsMode,
};
use tablepro_foreign_import::{ForeignApp, ForeignPlatform, Inventory, PasswordSource};
use tablepro_session::connect::{Connector as _, RequestTarget};
use tablepro_ssh::SshAuth;
use tablepro_storage::{
    ConnectionColor, ConnectionList, SavedConnection, SavedSshAuth, SavedSshConfig,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::DataForgeWorkspace;
use super::engine_icons::engine_icon;
use super::shell_state::ToastSeverity;
use super::theme::{
    COMPACT_GAP_PX, HAIRLINE_BORDER_PX, MICRO_GAP_PX, hairline_border_color, is_reduced_motion,
    panel_surface_bg,
};
use crate::app::ScreenMode;
use crate::services::{build_connect_request, default_port_for_engine, driver_id_from_engine};
use crate::views::library::{AuthMode, ConnectionFormDraft, TlsMode, engine_from_driver_id};

const TAB_CONFIGURE: &str = "configure";
const TAB_IMPORT: &str = "import";
const LIST_FILTER_ALL: &str = "all";
const LIST_FILTER_FAVORITES: &str = "favorites";

#[derive(Debug, Clone)]
struct ConnectionTestResult {
    server_label: String,
    latency_ms: u128,
    schema_count: usize,
}

#[derive(Debug, Clone)]
struct ListRowMeta {
    conn_id: String,
    name: String,
    subtitle: String,
    engine: EngineKind,
    is_favorite: bool,
    color: Option<ConnectionColor>,
}

/// Persistent UI state for the Connections Library and Connection Form screen.
pub struct LibraryScreenState {
    pub url_draft: String,
    search_input: Entity<InputState>,
    url_input: Entity<InputState>,
    name_input: Entity<InputState>,
    host_input: Entity<InputState>,
    port_input: Entity<InputState>,
    database_input: Entity<InputState>,
    username_input: Entity<InputState>,
    password_input: Entity<InputState>,
    tls_ca_input: Entity<InputState>,
    tls_cert_input: Entity<InputState>,
    tls_key_input: Entity<InputState>,
    tls_server_name_input: Entity<InputState>,
    ssh_host_input: Entity<InputState>,
    ssh_port_input: Entity<InputState>,
    ssh_user_input: Entity<InputState>,
    ssh_key_input: Entity<InputState>,
    ssh_passphrase_input: Entity<InputState>,
    auth_database_input: Entity<InputState>,
    replica_set_input: Entity<InputState>,
    timeout_secs_input: Entity<InputState>,
    tags_input: Entity<InputState>,
    selected_connection_id: Option<String>,
    selected_color: Option<ConnectionColor>,
    read_only: bool,
    dns_seed_list: bool,
    channel_binding: bool,
    has_stored_db_password: bool,
    has_stored_ssh_passphrase: bool,
    active_tab: SharedString,
    filter_favorites_only: bool,
    expanded_disclosures: HashSet<SharedString>,
    test_in_flight: bool,
    test_generation: u64,
    test_outcome: Option<Result<ConnectionTestResult, String>>,
    hydrated_from_store: bool,
    saved_records: HashMap<String, SavedConnection>,
    inventories: Vec<(ForeignApp, Inventory)>,
    inventories_scanned: bool,
    last_import_report: Option<Result<ForeignImportReport, String>>,
    delete_armed_for: Option<String>,
    last_synced_draft: ConnectionFormDraft,
}

impl LibraryScreenState {
    /// Initializes library screen state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, cx: &mut Context<DataForgeWorkspace>) -> Self {
        let defaults = ConnectionFormDraft::default();
        Self {
            url_draft: defaults.url_input.clone(),
            search_input: cx.new(|cx| InputState::new(cx)),
            url_input: cx.new(|cx| InputState::with_value(cx, defaults.url_input.clone())),
            name_input: cx.new(|cx| InputState::with_value(cx, defaults.name.clone())),
            host_input: cx.new(|cx| InputState::with_value(cx, defaults.host.clone())),
            port_input: cx.new(|cx| InputState::with_value(cx, defaults.port.to_string())),
            database_input: cx.new(|cx| InputState::with_value(cx, defaults.database.clone())),
            username_input: cx.new(|cx| InputState::with_value(cx, defaults.username.clone())),
            password_input: cx.new(|cx| InputState::new(cx)),
            tls_ca_input: cx.new(|cx| InputState::new(cx)),
            tls_cert_input: cx.new(|cx| InputState::new(cx)),
            tls_key_input: cx.new(|cx| InputState::new(cx)),
            tls_server_name_input: cx.new(|cx| InputState::new(cx)),
            ssh_host_input: cx.new(|cx| InputState::with_value(cx, defaults.ssh_host.clone())),
            ssh_port_input: cx.new(|cx| InputState::with_value(cx, defaults.ssh_port.to_string())),
            ssh_user_input: cx.new(|cx| InputState::with_value(cx, defaults.ssh_user.clone())),
            ssh_key_input: cx.new(|cx| InputState::with_value(cx, defaults.ssh_key_path.clone())),
            ssh_passphrase_input: cx.new(|cx| InputState::new(cx)),
            auth_database_input: cx.new(|cx| InputState::new(cx)),
            replica_set_input: cx.new(|cx| InputState::new(cx)),
            timeout_secs_input: cx.new(|cx| InputState::new(cx)),
            tags_input: cx.new(|cx| InputState::new(cx)),
            selected_connection_id: None,
            selected_color: None,
            read_only: false,
            dns_seed_list: false,
            channel_binding: false,
            has_stored_db_password: false,
            has_stored_ssh_passphrase: false,
            active_tab: SharedString::from(TAB_CONFIGURE),
            filter_favorites_only: false,
            expanded_disclosures: HashSet::new(),
            test_in_flight: false,
            test_generation: 0,
            test_outcome: None,
            hydrated_from_store: false,
            saved_records: HashMap::new(),
            inventories: Vec::new(),
            inventories_scanned: false,
            last_import_report: None,
            delete_armed_for: None,
            last_synced_draft: defaults,
        }
    }

    /// Synchronizes input entities from an updated [`ConnectionFormDraft`].
    pub fn sync_from_controller(&mut self, draft: &ConnectionFormDraft, cx: &mut App) {
        self.url_draft.clone_from(&draft.url_input);
        set_input_text(&self.url_input, &draft.url_input, cx);
        set_input_text(&self.name_input, &draft.name, cx);
        set_input_text(&self.host_input, &draft.host, cx);
        let port_str = if draft.port == 0 {
            String::new()
        } else {
            draft.port.to_string()
        };
        set_input_text(&self.port_input, &port_str, cx);
        set_input_text(&self.database_input, &draft.database, cx);
        set_input_text(&self.username_input, &draft.username, cx);
        set_input_text(&self.ssh_host_input, &draft.ssh_host, cx);
        set_input_text(&self.ssh_port_input, &draft.ssh_port.to_string(), cx);
        set_input_text(&self.ssh_user_input, &draft.ssh_user, cx);
        set_input_text(&self.ssh_key_input, &draft.ssh_key_path, cx);
        self.last_synced_draft = draft.clone();
    }

    fn sync_extended_from_saved(
        &mut self,
        saved: Option<&SavedConnection>,
        tags: Option<&BTreeSet<String>>,
        cx: &mut App,
    ) {
        self.selected_color = saved.and_then(|s| s.color);
        self.read_only = saved.is_some_and(|s| s.safe_mode == SafeModeLevel::ReadOnly);
        self.dns_seed_list = saved.is_some_and(|s| s.options.dns_seed_list);
        self.channel_binding = saved.is_some_and(|s| s.options.channel_binding);
        let ca = saved
            .and_then(|s| s.tls.ca_file.as_ref())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let cert = saved
            .and_then(|s| s.tls.client_identity.as_ref())
            .map(|id| id.certificate.to_string_lossy().into_owned())
            .unwrap_or_default();
        let key = saved
            .and_then(|s| s.tls.client_identity.as_ref())
            .map(|id| id.key.to_string_lossy().into_owned())
            .unwrap_or_default();
        let sni = saved
            .and_then(|s| s.tls.server_name.as_deref())
            .unwrap_or_default();
        let auth_db = saved
            .and_then(|s| s.options.auth_database.as_deref())
            .unwrap_or_default();
        let replica = saved
            .and_then(|s| s.options.replica_set.as_deref())
            .unwrap_or_default();
        let timeout = saved
            .and_then(|s| s.query_timeout_secs)
            .map(|t| t.to_string())
            .unwrap_or_default();
        let tags_str = tags
            .map(|set| set.iter().cloned().collect::<Vec<_>>().join(", "))
            .unwrap_or_default();

        set_input_text(&self.tls_ca_input, &ca, cx);
        set_input_text(&self.tls_cert_input, &cert, cx);
        set_input_text(&self.tls_key_input, &key, cx);
        set_input_text(&self.tls_server_name_input, sni, cx);
        set_input_text(&self.auth_database_input, auth_db, cx);
        set_input_text(&self.replica_set_input, replica, cx);
        set_input_text(&self.timeout_secs_input, &timeout, cx);
        set_input_text(&self.tags_input, &tags_str, cx);
    }

    fn reset_to_new(&mut self, draft: &mut ConnectionFormDraft, cx: &mut App) {
        *draft = ConnectionFormDraft::default();
        self.selected_connection_id = None;
        self.selected_color = None;
        self.read_only = false;
        self.dns_seed_list = false;
        self.channel_binding = false;
        self.has_stored_db_password = false;
        self.has_stored_ssh_passphrase = false;
        self.delete_armed_for = None;
        self.test_in_flight = false;
        self.test_outcome = None;
        self.active_tab = SharedString::from(TAB_CONFIGURE);
        set_input_text(&self.password_input, "", cx);
        set_input_text(&self.ssh_passphrase_input, "", cx);
        self.sync_extended_from_saved(None, None, cx);
        self.sync_from_controller(draft, cx);
    }
}

/// Renders the Connections Library and Connection Form screen.
#[must_use]
pub fn render_library_screen(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    hydrate_library_if_needed(ws);
    if ws.controller.connection_form != ws.library_ui.last_synced_draft {
        let draft = ws.controller.connection_form.clone();
        ws.library_ui.sync_from_controller(&draft, cx);
    }
    if ws.library_ui.active_tab.as_ref() == TAB_IMPORT {
        ensure_inventories_scanned(ws);
    }

    let colors = cx.colors().clone();
    div()
        .id("connections-library-screen")
        .flex_1()
        .size_full()
        .min_h_0()
        .flex()
        .flex_row()
        .bg(ws.content_palette.window_background)
        .text_color(colors.foreground)
        .child(render_library_rail(ws, cx))
        .child(render_detail_pane(ws, cx))
        .into_any_element()
}

fn hydrate_library_if_needed(ws: &mut DataForgeWorkspace) {
    if ws.library_ui.hydrated_from_store {
        return;
    }
    ws.library_ui.hydrated_from_store = true;
    let Some(services) = ws.controller.services.clone() else {
        return;
    };
    let Ok(list) = services.connection_store.list_blocking() else {
        return;
    };
    apply_connection_list_to_state(ws, &list);
}

fn apply_connection_list_to_state(ws: &mut DataForgeWorkspace, list: &ConnectionList) {
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
        let id_str = saved.id.to_string();
        ws.library_ui
            .saved_records
            .insert(id_str.clone(), saved.clone());
        let key = LibraryConnectionId(id_str);
        if ws
            .controller
            .library
            .storage()
            .connections
            .contains_key(&key)
        {
            continue;
        }
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

fn ensure_inventories_scanned(ws: &mut DataForgeWorkspace) {
    if ws.library_ui.inventories_scanned {
        return;
    }
    rescan_foreign_inventories(ws);
}

fn rescan_foreign_inventories(ws: &mut DataForgeWorkspace) {
    ws.library_ui.inventories_scanned = true;
    match foreign_app_dirs() {
        Ok(dirs) => {
            ws.library_ui.inventories = ws.controller.scan_foreign_inventories(&dirs);
        }
        Err(_) => {
            ws.library_ui.inventories = SUPPORTED_IMPORT_APPS
                .iter()
                .map(|&app| (app, Inventory::default()))
                .collect();
        }
    }
}

fn render_library_rail_title_row(
    total_count: usize,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let count_badge = Badge::new()
        .variant(BadgeVariant::Soft)
        .color(Color::Default)
        .size(Size::Sm)
        .sx(|d| d.font_features(tabular_font_features()))
        .child(BadgeLabel::new().child(total_count.to_string()));

    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(COMPACT_GAP_PX))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(MICRO_GAP_PX + 2.0))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Connections"),
                )
                .child(count_badge),
        )
        .child(
            Button::new("library-new-btn")
                .variant(Variant::Secondary)
                .size(Size::Sm)
                .label("New")
                .child(Icon::new(IconName::Plus).size(IconSize::Sm))
                .on_press(cx.listener(|ws, _, _, cx| {
                    ws.library_ui
                        .reset_to_new(&mut ws.controller.connection_form, cx);
                    cx.notify();
                })),
        )
        .into_any_element()
}

fn render_library_rail(
    ws: &mut DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let total_count = ws.controller.library.storage().connections.len();
    let filter_key = if ws.library_ui.filter_favorites_only {
        LIST_FILTER_FAVORITES
    } else {
        LIST_FILTER_ALL
    };
    let filter_tabs = Tabs::new(
        "library-rail-filter-tabs",
        vec![
            TabItem::new(LIST_FILTER_ALL, "All"),
            TabItem::new(LIST_FILTER_FAVORITES, "Favorites"),
        ],
        LIST_FILTER_ALL,
    )
    .selected_key(filter_key)
    .variant(TabsVariant::Primary)
    .size(TabsSize::Sm)
    .on_selection_change(cx.listener(|ws, key: &SharedString, _, cx| {
        ws.library_ui.filter_favorites_only = key.as_ref() == LIST_FILTER_FAVORITES;
        cx.notify();
    }));

    let search = SearchField::new(&ws.library_ui.search_input)
        .placeholder("Filter connections...")
        .variant(FieldVariant::Secondary)
        .full_width()
        .on_change(cx.listener(|ws, val: &str, _, cx| {
            ws.controller.library.set_search_query(val);
            cx.notify();
        }));

    div()
        .id("library-left-rail")
        .w(px(312.0))
        .h_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .bg(panel_surface_bg(cx))
        .border_r(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(COMPACT_GAP_PX))
                .p(px(12.0))
                .border_b(px(HAIRLINE_BORDER_PX))
                .border_color(hairline_border_color(cx))
                .child(render_library_rail_title_row(total_count, cx))
                .child(search)
                .child(filter_tabs),
        )
        .child(render_library_rail_body(ws, cx))
        .into_any_element()
}

fn render_library_rail_body(
    ws: &mut DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let storage = ws.controller.library.storage();
    if storage.connections.is_empty() {
        return render_empty_library_state(cx);
    }

    let (items, row_meta) = build_listbox_items(ws);
    if items.is_empty() {
        return render_no_matching_connections(ws, cx);
    }

    let selected_keys: HashSet<SharedString> = ws
        .library_ui
        .selected_connection_id
        .as_deref()
        .map(|sel| {
            row_meta
                .iter()
                .filter(|(_, meta)| meta.conn_id == sel)
                .map(|(row_key, _)| row_key.clone())
                .collect()
        })
        .unwrap_or_default();

    let colors = cx.colors().clone();
    let row_meta = Arc::new(row_meta);
    let row_meta_for_render = row_meta.clone();

    let list_box = ListBox::new("library-connections-listbox", items)
        .selection_mode(SelectionMode::Single)
        .selected_keys(selected_keys)
        .item_content(move |key: &SharedString, state: InteractiveState| {
            render_listbox_row(key, state, &row_meta_for_render, &colors)
        })
        .on_selection_change(cx.listener(|ws, keys: &HashSet<SharedString>, _, cx| {
            let Some(first_key) = keys.iter().next() else {
                return;
            };
            if let Some(conn_id) = extract_connection_id(first_key.as_ref()) {
                select_saved_connection(ws, &conn_id, cx);
            }
        }))
        .on_action(cx.listener(|ws, key: &SharedString, _, cx| {
            if let Some(conn_id) = extract_connection_id(key.as_ref()) {
                select_saved_connection(ws, &conn_id, cx);
                connect_saved_connection_blocking(ws, &conn_id, cx);
            }
        }))
        .sx(|d| d.flex_1().w_full().min_h_0());

    div()
        .id("library-rail-scroll")
        .flex_1()
        .min_h_0()
        .p(px(8.0))
        .overflow_y_scroll()
        .child(list_box)
        .into_any_element()
}

fn render_empty_library_state(cx: &mut Context<DataForgeWorkspace>) -> AnyElement {
    let colors = cx.colors().clone();
    let card = Card::new()
        .variant(CardVariant::Secondary)
        .child(
            CardHeader::new()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(COMPACT_GAP_PX))
                        .child(
                            Icon::new(IconName::Database)
                                .size(IconSize::Md)
                                .color(colors.muted),
                        )
                        .child(CardTitle::new().child("No Saved Connections")),
                )
                .child(CardDescription::new().child(
                    "Configure a server connection, open a local SQLite or DuckDB file, or import existing profiles.",
                )),
        )
        .child(
            CardContent::new()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(COMPACT_GAP_PX))
                        .child(
                            Button::new("empty-new-conn")
                                .variant(Variant::Primary)
                                .size(Size::Sm)
                                .full_width(true)
                                .label("New Connection")
                                .child(Icon::new(IconName::Plus).size(IconSize::Sm))
                                .on_press(cx.listener(|ws, _, _, cx| {
                                    ws.library_ui
                                        .reset_to_new(&mut ws.controller.connection_form, cx);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("empty-open-file")
                                .variant(Variant::Secondary)
                                .size(Size::Sm)
                                .full_width(true)
                                .label("Open SQLite / DuckDB File...")
                                .child(Icon::new(IconName::FolderOpen).size(IconSize::Sm))
                                .on_press(cx.listener(|_ws, _, _, cx| {
                                    trigger_open_local_file(cx);
                                })),
                        )
                        .child(
                            Button::new("empty-open-import")
                                .variant(Variant::Tertiary)
                                .size(Size::Sm)
                                .full_width(true)
                                .label("Import From Other Clients")
                                .child(Icon::new(IconName::Download).size(IconSize::Sm))
                                .on_press(cx.listener(|ws, _, _, cx| {
                                    ws.library_ui.active_tab = SharedString::from(TAB_IMPORT);
                                    ensure_inventories_scanned(ws);
                                    cx.notify();
                                })),
                        ),
                ),
        );

    div().flex_1().p(px(12.0)).child(card).into_any_element()
}

fn render_no_matching_connections(
    ws: &mut DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let has_query = !ws.controller.library.search_query.trim().is_empty();
    let msg = if ws.library_ui.filter_favorites_only && !has_query {
        "No favorite connections yet. Star a saved connection to pin it here."
    } else {
        "No connections match your current filter."
    };

    div()
        .flex_1()
        .p(px(12.0))
        .child(
            Card::new()
                .variant(CardVariant::Secondary)
                .child(
                    CardHeader::new()
                        .child(CardTitle::new().child("No Matching Connections"))
                        .child(CardDescription::new().child(msg)),
                )
                .child(
                    CardContent::new().child(
                        Button::new("clear-library-filter")
                            .variant(Variant::Secondary)
                            .size(Size::Sm)
                            .label("Show All Connections")
                            .on_press(cx.listener(|ws, _, _, cx| {
                                ws.library_ui.filter_favorites_only = false;
                                set_input_text(&ws.library_ui.search_input, "", cx);
                                ws.controller.library.set_search_query("");
                                cx.notify();
                            })),
                    ),
                ),
        )
        .into_any_element()
}

fn build_listbox_items(
    ws: &DataForgeWorkspace,
) -> (Vec<ListBoxItem>, HashMap<SharedString, ListRowMeta>) {
    let mut items = Vec::new();
    let mut meta_map = HashMap::new();
    let outline = ws.controller.library.outline();
    let conns = &ws.controller.library.storage().connections;
    let saved = &ws.library_ui.saved_records;

    for row in outline {
        match &row.row_id {
            LibraryRowID::GroupHeader(_) => {
                if !ws.library_ui.filter_favorites_only && has_group_children(row, outline) {
                    items.push(ListBoxItem::section(row.label.clone()));
                }
            }
            LibraryRowID::Connection { section, id } => {
                if ws.library_ui.filter_favorites_only
                    && !matches!(section, LibrarySection::Favorites)
                {
                    continue;
                }
                let Some(conn) = conns.get(id) else {
                    continue;
                };
                if matches!(section, LibrarySection::Favorites)
                    && items.is_empty()
                    && !ws.library_ui.filter_favorites_only
                {
                    items.push(ListBoxItem::section("Favorites"));
                }
                if matches!(section, LibrarySection::Ungrouped)
                    && items
                        .iter()
                        .any(|item| matches!(item, ListBoxItem::Section(_)))
                    && !has_ungrouped_header(&items)
                {
                    items.push(ListBoxItem::section("Other Connections"));
                }

                let row_key = SharedString::from(encode_row_key(section, &id.0));
                let engine = engine_from_driver_id(&conn.driver_id);
                let subtitle = format_connection_subtitle(conn, engine);
                let color = saved.get(&id.0).and_then(|s| s.color);
                meta_map.insert(
                    row_key.clone(),
                    ListRowMeta {
                        conn_id: id.0.clone(),
                        name: conn.name.clone(),
                        subtitle: subtitle.clone(),
                        engine,
                        is_favorite: conn.is_favorite,
                        color,
                    },
                );
                items.push(ListBoxItem::new(row_key, conn.name.clone()).description(subtitle));
            }
        }
    }

    (items, meta_map)
}

fn has_group_children(header: &LibraryOutlineRow, outline: &[LibraryOutlineRow]) -> bool {
    let LibraryRowID::GroupHeader(target_gid) = &header.row_id else {
        return false;
    };
    outline.iter().any(|row| {
        matches!(
            &row.row_id,
            LibraryRowID::Connection { section: LibrarySection::Group(gid), .. } if gid == target_gid
        )
    })
}

fn has_ungrouped_header(items: &[ListBoxItem]) -> bool {
    items.iter().any(
        |item| matches!(item, ListBoxItem::Section(title) if title.as_ref() == "Other Connections"),
    )
}

fn encode_row_key(section: &LibrarySection, id: &str) -> String {
    match section {
        LibrarySection::Favorites => format!("fav|{id}"),
        LibrarySection::Group(gid) => format!("grp:{}|{id}", gid.0),
        LibrarySection::Ungrouped => format!("ung|{id}"),
    }
}

fn extract_connection_id(key: &str) -> Option<String> {
    key.split_once('|').map(|(_, id)| id.to_owned())
}

fn format_connection_subtitle(conn: &LibraryConnection, engine: EngineKind) -> String {
    if matches!(engine, EngineKind::Sqlite | EngineKind::Duckdb) {
        let name = Path::new(&conn.database)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(conn.database.as_str());
        if name.trim().is_empty() {
            return engine.display_name().to_owned();
        }
        return format!("{} · {name}", engine.display_name());
    }
    let port_part = conn.port.map(|p| format!(":{p}")).unwrap_or_default();
    if conn.database.trim().is_empty() {
        format!("{}{port_part}", conn.host)
    } else {
        format!("{}{port_part}/{}", conn.host, conn.database)
    }
}

fn render_listbox_row(
    key: &SharedString,
    state: InteractiveState,
    meta_map: &HashMap<SharedString, ListRowMeta>,
    colors: &herogpui::theme::ThemeColors,
) -> AnyElement {
    let Some(meta) = meta_map.get(key) else {
        return div().child(key.clone()).into_any_element();
    };
    let icon_color = if state.is_selected {
        colors.accent.color
    } else {
        colors.foreground
    };
    let sub_color = colors.muted;
    let display_name = truncate_middle(&meta.name, 26);

    let mut row = div()
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(COMPACT_GAP_PX))
        .child(engine_icon(meta.engine, px(16.0), icon_color))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(MICRO_GAP_PX + 2.0))
                        .when_some(meta.color, |d, c| d.child(color_dot(c)))
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .truncate()
                                .child(display_name),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(sub_color)
                        .font_features(tabular_font_features())
                        .truncate()
                        .child(meta.subtitle.clone()),
                ),
        );

    if meta.is_favorite {
        row = row.child(
            Icon::new(IconName::Star)
                .size(IconSize::Sm)
                .color(colors.warning.color),
        );
    }

    row.into_any_element()
}

fn render_detail_pane(
    ws: &mut DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let body = if ws.library_ui.active_tab.as_ref() == TAB_IMPORT {
        render_foreign_importers_tab(ws, cx)
    } else {
        render_configure_tab(ws, cx)
    };

    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_col()
        .child(render_detail_header(ws, cx))
        .child(
            div()
                .id("library-detail-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(20.0))
                .child(
                    div()
                        .w_full()
                        .max_w(px(760.0))
                        .mx_auto()
                        .flex()
                        .flex_col()
                        .gap(px(16.0))
                        .child(body),
                ),
        )
        .into_any_element()
}

fn render_detail_header_actions(
    conn_id: &str,
    is_favorite: bool,
    delete_armed: bool,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let star_id = conn_id.to_owned();
    let del_id = conn_id.to_owned();

    div()
        .flex()
        .items_center()
        .gap(px(COMPACT_GAP_PX))
        .child(
            Button::new("library-fav-toggle")
                .variant(if is_favorite {
                    Variant::Secondary
                } else {
                    Variant::Tertiary
                })
                .size(Size::Sm)
                .label(if is_favorite { "Favorited" } else { "Favorite" })
                .child(
                    Icon::new(IconName::Star)
                        .size(IconSize::Sm)
                        .color(if is_favorite {
                            colors.warning.color
                        } else {
                            colors.muted
                        }),
                )
                .on_press(cx.listener(move |ws, _, _, cx| {
                    toggle_saved_favorite(ws, &star_id, cx);
                })),
        )
        .child(
            Button::new("library-delete-btn")
                .variant(if delete_armed {
                    Variant::Danger
                } else {
                    Variant::Tertiary
                })
                .size(Size::Sm)
                .label(if delete_armed {
                    "Confirm Delete"
                } else {
                    "Delete"
                })
                .child(Icon::new(IconName::Trash2).size(IconSize::Sm))
                .on_press(cx.listener(move |ws, _, _, cx| {
                    handle_delete_press(ws, &del_id, cx);
                })),
        )
        .into_any_element()
}

fn render_detail_header(
    ws: &mut DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let draft = &ws.controller.connection_form;
    let engine = draft.engine;
    let selected_id = ws.library_ui.selected_connection_id.clone();
    let is_favorite = selected_id.as_deref().is_some_and(|id| {
        ws.controller
            .library
            .storage()
            .connections
            .get(&LibraryConnectionId(id.to_owned()))
            .is_some_and(|c| c.is_favorite)
    });

    let title = if draft.name.trim().is_empty() {
        format!("New {} Connection", engine.display_name())
    } else {
        draft.name.clone()
    };

    let tabs = Tabs::new(
        "library-detail-tabs",
        vec![
            TabItem::new(TAB_CONFIGURE, "Configure"),
            TabItem::new(TAB_IMPORT, "Foreign Importers"),
        ],
        TAB_CONFIGURE,
    )
    .selected_key(ws.library_ui.active_tab.clone())
    .variant(TabsVariant::Secondary)
    .size(TabsSize::Sm)
    .on_selection_change(cx.listener(|ws, key: &SharedString, _, cx| {
        ws.library_ui.active_tab = key.clone();
        if key.as_ref() == TAB_IMPORT {
            ensure_inventories_scanned(ws);
        }
        cx.notify();
    }));

    let mut left_group = div()
        .flex()
        .items_center()
        .gap(px(COMPACT_GAP_PX + 2.0))
        .min_w_0()
        .child(engine_icon(engine, px(22.0), colors.accent.color))
        .when_some(ws.library_ui.selected_color, |d, c| d.child(color_dot(c)))
        .child(
            div()
                .text_base()
                .font_weight(FontWeight::SEMIBOLD)
                .truncate()
                .child(title),
        )
        .child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .color(Color::Default)
                .size(Size::Sm)
                .child(ChipLabel::new().child(engine.display_name())),
        );

    if let Some(conn_id) = selected_id {
        let delete_armed = ws.library_ui.delete_armed_for.as_deref() == Some(conn_id.as_str());
        left_group = left_group.child(render_detail_header_actions(
            &conn_id,
            is_favorite,
            delete_armed,
            cx,
        ));
    }

    div()
        .w_full()
        .px(px(20.0))
        .py(px(12.0))
        .bg(panel_surface_bg(cx))
        .border_b(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(left_group)
        .child(tabs)
        .into_any_element()
}

fn render_configure_tab(
    ws: &mut DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let mut root = div().flex().flex_col().gap(px(16.0));
    if let Some(err) = ws.controller.connection_form.last_error.clone() {
        root = root.child(
            Alert::new("Connection Error")
                .status(Color::Danger)
                .description(err),
        );
    }

    let form = Form::new()
        .on_submit(cx.listener(|ws, _, _, cx| {
            trigger_connect_current(ws, cx);
        }))
        .child(render_identity_card(ws, cx))
        .child(render_safety_card(ws, cx))
        .child(render_advanced_disclosures(ws, cx));

    root = root.child(render_url_parser_card(ws, cx)).child(form);

    if let Some(test_alert) = render_test_alert(ws, cx) {
        root = root.child(test_alert);
    }

    root.child(render_action_bar(ws, cx)).into_any_element()
}

fn render_url_parser_card(
    ws: &mut DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    Card::new()
        .variant(CardVariant::Secondary)
        .child(
            CardHeader::new()
                .child(CardTitle::new().child("Quick Connection URL"))
                .child(CardDescription::new().child(
                    "Paste a URI (postgres://, mysql://, sqlite://, mongodb+srv://, redis://) to populate the form.",
                )),
        )
        .child(
            CardContent::new().child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(COMPACT_GAP_PX))
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&ws.library_ui.url_input)
                                .input_type(InputType::Url)
                                .variant(FieldVariant::Primary)
                                .placeholder("postgres://user:password@localhost:5432/app?sslmode=require")
                                .is_clearable(true)
                                .full_width()
                                .on_change(cx.listener(|ws, val: &str, _, _| {
                                    ws.library_ui.url_draft = val.to_owned();
                                    ws.controller.connection_form.url_input = val.to_owned();
                                    ws.library_ui.last_synced_draft.url_input = val.to_owned();
                                }))
                                .on_submit(cx.listener(|ws, val: &str, _, cx| {
                                    trigger_parse_url(ws, val, cx);
                                })),
                        ),
                    )
                    .child(
                        Button::new("library-parse-url-btn")
                            .variant(Variant::Secondary)
                            .label("Parse URL")
                            .child(Icon::new(IconName::Link2).size(IconSize::Sm))
                            .on_press(cx.listener(|ws, _, _, cx| {
                                let url = ws.library_ui.url_input.read(cx).value().to_owned();
                                trigger_parse_url(ws, &url, cx);
                            })),
                    ),
            ),
        )
        .into_any_element()
}

fn render_identity_card(
    ws: &mut DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let draft = &ws.controller.connection_form;
    let is_file_engine = matches!(draft.engine, EngineKind::Sqlite | EngineKind::Duckdb);
    let endpoint_block = if is_file_engine {
        render_file_endpoint_fields(ws, cx)
    } else {
        render_network_endpoint_fields(ws, cx)
    };

    Card::new()
        .variant(CardVariant::Default)
        .child(
            CardHeader::new()
                .child(CardTitle::new().child("General & Endpoint"))
                .child(CardDescription::new().child(
                    "Choose a database engine, target endpoint, and authentication credentials.",
                )),
        )
        .child(
            CardContent::new().child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .child(
                        Input::new(&ws.library_ui.name_input)
                            .variant(FieldVariant::Secondary)
                            .label("Connection Name")
                            .placeholder("Production Primary")
                            .full_width()
                            .on_change(cx.listener(|ws, val: &str, _, cx| {
                                ws.controller.connection_form.name = val.to_owned();
                                ws.library_ui.last_synced_draft.name = val.to_owned();
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap(px(12.0))
                            .child(div().flex_1().min_w_0().child(render_engine_select(ws, cx)))
                            .child(
                                div()
                                    .w(px(210.0))
                                    .flex_shrink_0()
                                    .child(render_color_select(ws, cx)),
                            ),
                    )
                    .child(endpoint_block),
            ),
        )
        .into_any_element()
}

fn render_engine_select(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let current_engine = ws.controller.connection_form.engine;
    let items: Vec<PickerItem> = EngineKind::ALL
        .iter()
        .enumerate()
        .map(|(idx, kind)| PickerItem::new(idx.to_string(), kind.display_name()))
        .collect();
    let selected_idx = EngineKind::ALL
        .iter()
        .position(|k| *k == current_engine)
        .unwrap_or(0);
    let icon_color = colors.foreground;

    Select::new("library-engine-select", items)
        .label("Database Engine")
        .variant(FieldVariant::Secondary)
        .full_width(true)
        .value(Some(SharedString::from(selected_idx.to_string())))
        .item_leading(move |key: &SharedString, _selected: bool| {
            let idx = key.parse::<usize>().ok()?;
            let engine = EngineKind::ALL.get(idx).copied()?;
            Some(engine_icon(engine, px(16.0), icon_color))
        })
        .value_content(move |sv: SelectionValue<'_>| {
            let idx = sv.selected_indices.first().copied().unwrap_or(selected_idx);
            let engine = EngineKind::ALL.get(idx).copied().unwrap_or(current_engine);
            div()
                .flex()
                .items_center()
                .gap(px(COMPACT_GAP_PX))
                .child(engine_icon(engine, px(16.0), icon_color))
                .child(engine.display_name())
                .into_any_element()
        })
        .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _, cx| {
            let Some(idx) = key.as_deref().and_then(|s| s.parse::<usize>().ok()) else {
                return;
            };
            let Some(&next_engine) = EngineKind::ALL.get(idx) else {
                return;
            };
            apply_engine_change(ws, next_engine, cx);
        }))
        .into_any_element()
}

fn render_color_select(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let mut items = vec![PickerItem::new("0", "No Tag Color")];
    for (idx, color) in ConnectionColor::ALL.iter().enumerate() {
        items.push(PickerItem::new(
            (idx + 1).to_string(),
            color_display_name(*color),
        ));
    }
    let selected_idx = ws
        .library_ui
        .selected_color
        .and_then(|c| ConnectionColor::ALL.iter().position(|item| *item == c))
        .map_or(0, |idx| idx + 1);

    Select::new("library-color-select", items)
        .label("Color Tag")
        .variant(FieldVariant::Secondary)
        .full_width(true)
        .value(Some(SharedString::from(selected_idx.to_string())))
        .item_leading(|key: &SharedString, _selected: bool| {
            let idx = key.parse::<usize>().ok()?;
            if idx == 0 {
                return None;
            }
            let color = ConnectionColor::ALL.get(idx - 1).copied()?;
            Some(color_dot(color))
        })
        .value_content(move |sv: SelectionValue<'_>| {
            let idx = sv.selected_indices.first().copied().unwrap_or(selected_idx);
            let chosen = idx
                .checked_sub(1)
                .and_then(|pos| ConnectionColor::ALL.get(pos).copied());
            let label = chosen.map_or("No Tag Color", color_display_name);
            div()
                .flex()
                .items_center()
                .gap(px(COMPACT_GAP_PX))
                .when_some(chosen, |d, c| d.child(color_dot(c)))
                .child(label)
                .into_any_element()
        })
        .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _, cx| {
            let Some(idx) = key.as_deref().and_then(|s| s.parse::<usize>().ok()) else {
                ws.library_ui.selected_color = None;
                cx.notify();
                return;
            };
            ws.library_ui.selected_color = idx
                .checked_sub(1)
                .and_then(|pos| ConnectionColor::ALL.get(pos).copied());
            ws.library_ui.delete_armed_for = None;
            cx.notify();
        }))
        .into_any_element()
}

fn render_file_endpoint_fields(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .flex()
                .items_end()
                .gap(px(COMPACT_GAP_PX))
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(&ws.library_ui.database_input)
                            .variant(FieldVariant::Secondary)
                            .label("Database File Path")
                            .placeholder("C:\\data\\analytics.sqlite")
                            .full_width()
                            .on_change(cx.listener(|ws, val: &str, _, _| {
                                ws.controller.connection_form.database = val.to_owned();
                                ws.library_ui.last_synced_draft.database = val.to_owned();
                            })),
                    ),
                )
                .child(
                    Button::new("library-browse-db-file")
                        .variant(Variant::Secondary)
                        .label("Browse...")
                        .child(Icon::new(IconName::FolderOpen).size(IconSize::Sm))
                        .on_press(cx.listener(|_ws, _, _, cx| {
                            trigger_browse_path(BrowseTarget::DatabaseFile, cx);
                        })),
                ),
        )
        .into_any_element()
}

fn render_host_port_row(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    div()
        .flex()
        .items_start()
        .gap(px(12.0))
        .child(
            div().flex_1().min_w_0().child(
                Input::new(&ws.library_ui.host_input)
                    .variant(FieldVariant::Secondary)
                    .label("Host")
                    .placeholder("localhost")
                    .full_width()
                    .on_change(cx.listener(|ws, val: &str, _, _| {
                        ws.controller.connection_form.host = val.to_owned();
                        ws.library_ui.last_synced_draft.host = val.to_owned();
                    })),
            ),
        )
        .child(
            div().w(px(130.0)).flex_shrink_0().child(
                Input::new(&ws.library_ui.port_input)
                    .input_type(InputType::Number)
                    .variant(FieldVariant::Secondary)
                    .label("Port")
                    .placeholder("5432")
                    .full_width()
                    .on_change(cx.listener(|ws, val: &str, _, _| {
                        if let Ok(port) = val.trim().parse::<u16>() {
                            ws.controller.connection_form.port = port;
                            ws.library_ui.last_synced_draft.port = port;
                        }
                    })),
            ),
        )
        .into_any_element()
}

fn render_credentials_row(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let mut password_input = Input::new(&ws.library_ui.password_input)
        .input_type(InputType::Password)
        .variant(FieldVariant::Secondary)
        .label("Password")
        .placeholder(if ws.library_ui.has_stored_db_password {
            "•••••••• (stored)"
        } else {
            "Optional password"
        })
        .full_width();
    if ws.library_ui.has_stored_db_password {
        password_input = password_input
            .description("Stored in Windows Credential Manager (leave blank to keep)");
    }

    div()
        .flex()
        .items_start()
        .gap(px(12.0))
        .child(
            div().flex_1().min_w_0().child(
                Input::new(&ws.library_ui.username_input)
                    .variant(FieldVariant::Secondary)
                    .label("Username")
                    .placeholder("postgres")
                    .full_width()
                    .on_change(cx.listener(|ws, val: &str, _, _| {
                        ws.controller.connection_form.username = val.to_owned();
                        ws.library_ui.last_synced_draft.username = val.to_owned();
                    })),
            ),
        )
        .child(div().flex_1().min_w_0().child(password_input))
        .into_any_element()
}

fn render_network_endpoint_fields(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let draft = &ws.controller.connection_form;
    let is_sspi = draft.auth_mode == AuthMode::SspiIntegrated;
    let db_label = match draft.engine {
        EngineKind::Cassandra | EngineKind::Scylladb => "Keyspace",
        EngineKind::Redis => "Database Index",
        _ => "Database",
    };

    let auth_or_creds = if is_sspi {
        Alert::new("Windows Integrated Authentication (SSPI / Kerberos)")
            .status(Color::Default)
            .description(
                "Uses your active Windows domain / Kerberos ticket instead of a database password.",
            )
            .into_any_element()
    } else {
        render_credentials_row(ws, cx)
    };

    div()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(render_host_port_row(ws, cx))
        .child(
            div()
                .flex()
                .items_start()
                .gap(px(12.0))
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(&ws.library_ui.database_input)
                            .variant(FieldVariant::Secondary)
                            .label(db_label)
                            .placeholder("postgres")
                            .full_width()
                            .on_change(cx.listener(|ws, val: &str, _, _| {
                                ws.controller.connection_form.database = val.to_owned();
                                ws.library_ui.last_synced_draft.database = val.to_owned();
                            })),
                    ),
                )
                .child(
                    div()
                        .w(px(230.0))
                        .flex_shrink_0()
                        .child(render_auth_mode_select(ws, cx)),
                ),
        )
        .child(auth_or_creds)
        .into_any_element()
}

fn render_auth_mode_select(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let modes = [
        (AuthMode::Password, "Username & Password"),
        (AuthMode::SspiIntegrated, "Windows Integrated (SSPI)"),
        (AuthMode::Certificate, "TLS Client Certificate"),
    ];
    let items: Vec<PickerItem> = modes
        .iter()
        .enumerate()
        .map(|(i, (_, label))| PickerItem::new(i.to_string(), *label))
        .collect();
    let current = modes
        .iter()
        .position(|(m, _)| *m == ws.controller.connection_form.auth_mode)
        .unwrap_or(0);

    Select::new("library-auth-mode-select", items)
        .label("Authentication")
        .variant(FieldVariant::Secondary)
        .full_width(true)
        .value(Some(SharedString::from(current.to_string())))
        .on_selection_change(cx.listener(move |ws, key: &Option<SharedString>, _, cx| {
            let Some(idx) = key.as_deref().and_then(|s| s.parse::<usize>().ok()) else {
                return;
            };
            if let Some((mode, _)) = modes.get(idx) {
                ws.controller.connection_form.auth_mode = *mode;
                ws.library_ui.last_synced_draft.auth_mode = *mode;
                cx.notify();
            }
        }))
        .into_any_element()
}

fn render_safety_card(ws: &DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) -> AnyElement {
    let draft = &ws.controller.connection_form;
    let is_network = !matches!(draft.engine, EngineKind::Sqlite | EngineKind::Duckdb);
    let tls_on = draft.tls_mode != TlsMode::Disable;

    let mut row = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(20.0))
        .child(
            Switch::new("library-read-only-switch")
                .size(Size::Sm)
                .is_selected(ws.library_ui.read_only)
                .label("Read-Only")
                .description("Block all mutating statements")
                .on_change(cx.listener(|ws, on: &bool, _, cx| {
                    ws.library_ui.read_only = *on;
                    cx.notify();
                })),
        )
        .child(
            Switch::new("library-safe-mode-switch")
                .size(Size::Sm)
                .is_selected(ws.controller.safe_mode_enabled)
                .label("Safe Mode")
                .description("Confirm UPDATE / DELETE / DROP")
                .on_change(cx.listener(|ws, on: &bool, _, cx| {
                    ws.controller.safe_mode_enabled = *on;
                    cx.notify();
                })),
        );

    if is_network {
        row = row
            .child(
                Switch::new("library-tls-quick-switch")
                    .size(Size::Sm)
                    .is_selected(tls_on)
                    .label("TLS / SSL")
                    .description("Encrypt server transport")
                    .on_change(cx.listener(|ws, on: &bool, _, cx| {
                        let next = if *on {
                            TlsMode::Require
                        } else {
                            TlsMode::Disable
                        };
                        ws.controller.connection_form.tls_mode = next;
                        ws.library_ui.last_synced_draft.tls_mode = next;
                        if *on {
                            ws.library_ui
                                .expanded_disclosures
                                .insert(SharedString::from("tls"));
                        }
                        cx.notify();
                    })),
            )
            .child(
                Switch::new("library-ssh-quick-switch")
                    .size(Size::Sm)
                    .is_selected(draft.ssh_enabled)
                    .label("SSH Tunnel")
                    .description("Forward over bastion host")
                    .on_change(cx.listener(|ws, on: &bool, _, cx| {
                        ws.controller.connection_form.ssh_enabled = *on;
                        ws.library_ui.last_synced_draft.ssh_enabled = *on;
                        if *on {
                            ws.library_ui
                                .expanded_disclosures
                                .insert(SharedString::from("ssh"));
                        }
                        cx.notify();
                    })),
            );
    }

    Card::new()
        .variant(CardVariant::Default)
        .child(CardContent::new().child(row))
        .into_any_element()
}

fn render_advanced_disclosures(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let is_network = !matches!(
        ws.controller.connection_form.engine,
        EngineKind::Sqlite | EngineKind::Duckdb
    );
    let mut group = DisclosureGroup::new("library-advanced-disclosures")
        .allows_multiple_expanded(true)
        .expanded_keys(ws.library_ui.expanded_disclosures.iter().cloned())
        .on_expanded_change(cx.listener(|ws, keys: &HashSet<SharedString>, _, cx| {
            ws.library_ui.expanded_disclosures = keys.clone();
            cx.notify();
        }));

    if is_network {
        group = group
            .item(
                "tls",
                "TLS / SSL & Client Certificates",
                render_tls_panel(ws, cx),
            )
            .item("ssh", "SSH Tunnel", render_ssh_panel(ws, cx));
    }
    group = group.item(
        "options",
        "Tags, Timeouts & Extra Parameters",
        render_options_panel(ws, cx),
    );

    Card::new()
        .variant(CardVariant::Default)
        .child(CardContent::new().child(group))
        .into_any_element()
}

fn render_tls_panel(ws: &DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) -> AnyElement {
    let modes = [
        (TlsMode::Disable, "Disable (Cleartext)"),
        (TlsMode::Prefer, "Prefer TLS if supported"),
        (TlsMode::Require, "Require TLS"),
        (TlsMode::VerifyCa, "Verify CA Certificate"),
        (TlsMode::VerifyFull, "Verify Full Hostname & CA"),
    ];
    let items: Vec<PickerItem> = modes
        .iter()
        .enumerate()
        .map(|(i, (_, label))| PickerItem::new(i.to_string(), *label))
        .collect();
    let current = modes
        .iter()
        .position(|(m, _)| *m == ws.controller.connection_form.tls_mode)
        .unwrap_or(1);

    let tls_mode_select = Select::new("library-tls-mode-select", items)
        .label("TLS Verification Mode")
        .variant(FieldVariant::Secondary)
        .full_width(true)
        .value(Some(SharedString::from(current.to_string())))
        .on_selection_change(cx.listener(move |ws, key: &Option<SharedString>, _, cx| {
            let Some(idx) = key.as_deref().and_then(|s| s.parse::<usize>().ok()) else {
                return;
            };
            if let Some((mode, _)) = modes.get(idx) {
                ws.controller.connection_form.tls_mode = *mode;
                ws.library_ui.last_synced_draft.tls_mode = *mode;
                cx.notify();
            }
        }));

    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .pt(px(4.0))
        .child(tls_mode_select)
        .child(path_browse_row(
            &ws.library_ui.tls_ca_input,
            "CA Certificate Path",
            "C:\\certs\\root-ca.pem",
            "library-browse-ca",
            BrowseTarget::TlsCaFile,
            cx,
        ))
        .child(
            div()
                .flex()
                .items_start()
                .gap(px(10.0))
                .child(div().flex_1().min_w_0().child(path_browse_row(
                    &ws.library_ui.tls_cert_input,
                    "Client Certificate",
                    "C:\\certs\\client.crt",
                    "library-browse-cert",
                    BrowseTarget::TlsClientCert,
                    cx,
                )))
                .child(div().flex_1().min_w_0().child(path_browse_row(
                    &ws.library_ui.tls_key_input,
                    "Client Private Key",
                    "C:\\certs\\client.key",
                    "library-browse-key",
                    BrowseTarget::TlsClientKey,
                    cx,
                ))),
        )
        .child(
            Input::new(&ws.library_ui.tls_server_name_input)
                .variant(FieldVariant::Secondary)
                .label("TLS Server Name (SNI Override)")
                .placeholder("db.internal.example.com")
                .full_width(),
        )
        .into_any_element()
}

fn render_ssh_bastion_fields(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .flex()
                .items_start()
                .gap(px(10.0))
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(&ws.library_ui.ssh_host_input)
                            .variant(FieldVariant::Secondary)
                            .label("SSH Bastion Host")
                            .placeholder("bastion.example.com")
                            .full_width()
                            .on_change(cx.listener(|ws, val: &str, _, _| {
                                ws.controller.connection_form.ssh_host = val.to_owned();
                                ws.library_ui.last_synced_draft.ssh_host = val.to_owned();
                            })),
                    ),
                )
                .child(
                    div().w(px(120.0)).flex_shrink_0().child(
                        Input::new(&ws.library_ui.ssh_port_input)
                            .input_type(InputType::Number)
                            .variant(FieldVariant::Secondary)
                            .label("SSH Port")
                            .placeholder("22")
                            .full_width()
                            .on_change(cx.listener(|ws, val: &str, _, _| {
                                if let Ok(port) = val.trim().parse::<u16>() {
                                    ws.controller.connection_form.ssh_port = port;
                                    ws.library_ui.last_synced_draft.ssh_port = port;
                                }
                            })),
                    ),
                ),
        )
        .child(
            Input::new(&ws.library_ui.ssh_user_input)
                .variant(FieldVariant::Secondary)
                .label("SSH Username")
                .placeholder("ubuntu")
                .full_width()
                .on_change(cx.listener(|ws, val: &str, _, _| {
                    ws.controller.connection_form.ssh_user = val.to_owned();
                    ws.library_ui.last_synced_draft.ssh_user = val.to_owned();
                })),
        )
        .into_any_element()
}

fn render_ssh_panel(ws: &DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) -> AnyElement {
    let draft = &ws.controller.connection_form;
    let mut pass_input = Input::new(&ws.library_ui.ssh_passphrase_input)
        .input_type(InputType::Password)
        .variant(FieldVariant::Secondary)
        .label("Private Key Passphrase")
        .placeholder(if ws.library_ui.has_stored_ssh_passphrase {
            "•••••••• (stored)"
        } else {
            "Optional key passphrase"
        })
        .full_width();
    if ws.library_ui.has_stored_ssh_passphrase {
        pass_input =
            pass_input.description("Stored in Windows Credential Manager (leave blank to keep)");
    }

    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .pt(px(4.0))
        .child(
            Checkbox::new("library-ssh-enabled-check")
                .is_selected(draft.ssh_enabled)
                .label("Enable SSH tunnel for this connection")
                .on_change(cx.listener(|ws, on: &bool, _, cx| {
                    ws.controller.connection_form.ssh_enabled = *on;
                    ws.library_ui.last_synced_draft.ssh_enabled = *on;
                    cx.notify();
                })),
        )
        .child(render_ssh_bastion_fields(ws, cx))
        .child(
            div()
                .flex()
                .items_end()
                .gap(px(COMPACT_GAP_PX))
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(&ws.library_ui.ssh_key_input)
                            .variant(FieldVariant::Secondary)
                            .label("SSH Private Key Path")
                            .placeholder("C:\\Users\\you\\.ssh\\id_ed25519")
                            .full_width()
                            .on_change(cx.listener(|ws, val: &str, _, _| {
                                ws.controller.connection_form.ssh_key_path = val.to_owned();
                                ws.library_ui.last_synced_draft.ssh_key_path = val.to_owned();
                            })),
                    ),
                )
                .child(
                    Button::new("library-browse-ssh-key")
                        .variant(Variant::Secondary)
                        .label("Browse...")
                        .on_press(cx.listener(|_ws, _, _, cx| {
                            trigger_browse_path(BrowseTarget::SshKeyFile, cx);
                        })),
                ),
        )
        .child(pass_input)
        .into_any_element()
}

fn render_options_panel(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .pt(px(4.0))
        .child(
            div()
                .flex()
                .items_start()
                .gap(px(10.0))
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(&ws.library_ui.tags_input)
                            .variant(FieldVariant::Secondary)
                            .label("Tags (comma separated)")
                            .placeholder("production, analytics")
                            .full_width(),
                    ),
                )
                .child(
                    div().w(px(170.0)).flex_shrink_0().child(
                        Input::new(&ws.library_ui.timeout_secs_input)
                            .input_type(InputType::Number)
                            .variant(FieldVariant::Secondary)
                            .label("Query Timeout (sec)")
                            .placeholder("Default")
                            .full_width(),
                    ),
                ),
        )
        .child(
            div()
                .flex()
                .items_start()
                .gap(px(10.0))
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(&ws.library_ui.auth_database_input)
                            .variant(FieldVariant::Secondary)
                            .label("Auth Database (MongoDB)")
                            .placeholder("admin")
                            .full_width(),
                    ),
                )
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(&ws.library_ui.replica_set_input)
                            .variant(FieldVariant::Secondary)
                            .label("Replica Set (MongoDB)")
                            .placeholder("rs0")
                            .full_width(),
                    ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(16.0))
                .child(
                    Checkbox::new("library-dns-seed-check")
                        .is_selected(ws.library_ui.dns_seed_list)
                        .label("DNS Seed List (mongodb+srv)")
                        .on_change(cx.listener(|ws, on: &bool, _, cx| {
                            ws.library_ui.dns_seed_list = *on;
                            cx.notify();
                        })),
                )
                .child(
                    Checkbox::new("library-channel-binding-check")
                        .is_selected(ws.library_ui.channel_binding)
                        .label("Require SCRAM Channel Binding")
                        .on_change(cx.listener(|ws, on: &bool, _, cx| {
                            ws.library_ui.channel_binding = *on;
                            cx.notify();
                        })),
                ),
        )
        .into_any_element()
}

fn render_test_alert(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> Option<AnyElement> {
    if ws.library_ui.test_in_flight {
        let indicator = if is_reduced_motion(cx) {
            Icon::new(IconName::LoaderCircle)
                .size(IconSize::Sm)
                .into_any_element()
        } else {
            Spinner::new("library-test-spinner")
                .size(SpinnerSize::Sm)
                .color(Color::Accent)
                .into_any_element()
        };
        return Some(
            Alert::new("Testing Connection...")
                .status(Color::Accent)
                .indicator(indicator)
                .description(format!(
                    "Establishing handshake with {} ({})...",
                    ws.controller.connection_form.engine.display_name(),
                    target_summary(&ws.controller.connection_form),
                ))
                .into_any_element(),
        );
    }

    let outcome = ws.library_ui.test_outcome.as_ref()?;
    let alert = match outcome {
        Ok(res) => {
            let schema_word = plural_word(res.schema_count, "schema", "schemas");
            let desc = format!(
                "{} · {} ms latency · {} {schema_word} discovered",
                res.server_label, res.latency_ms, res.schema_count
            );
            Alert::new("Connection Succeeded")
                .status(Color::Success)
                .description(desc)
        }
        Err(err) => Alert::new("Connection Test Failed")
            .status(Color::Danger)
            .description(err.clone()),
    };

    Some(alert.into_any_element())
}

fn render_action_bar(ws: &DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) -> AnyElement {
    let testing = ws.library_ui.test_in_flight;
    let test_btn = Button::new("library-test-btn")
        .variant(Variant::Secondary)
        .is_pending(testing)
        .label(if testing {
            "Testing..."
        } else {
            "Test Connection"
        })
        .on_press(cx.listener(|ws, _, _, cx| {
            trigger_test_connection(ws, cx);
        }));

    let right_buttons = div()
        .flex()
        .items_center()
        .gap(px(COMPACT_GAP_PX))
        .child(
            Button::new("library-reset-btn")
                .variant(Variant::Tertiary)
                .label("Reset")
                .on_press(cx.listener(|ws, _, _, cx| {
                    ws.library_ui
                        .reset_to_new(&mut ws.controller.connection_form, cx);
                    cx.notify();
                })),
        )
        .child(
            Button::new("library-save-btn")
                .variant(Variant::Secondary)
                .label("Save")
                .child(Icon::new(IconName::Check).size(IconSize::Sm))
                .on_press(cx.listener(|ws, _, _, cx| {
                    save_current_connection(ws, cx);
                })),
        )
        .child(
            Button::new("library-connect-btn")
                .variant(Variant::Primary)
                .label("Connect")
                .child(Icon::new(IconName::Play).size(IconSize::Sm))
                .on_press(cx.listener(|ws, _, _, cx| {
                    trigger_connect_current(ws, cx);
                })),
        );

    Card::new()
        .variant(CardVariant::Secondary)
        .child(
            CardContent::new().child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .child(test_btn)
                    .child(right_buttons),
            ),
        )
        .into_any_element()
}

fn render_foreign_importers_tab(
    ws: &mut DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let mut root = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            Card::new()
                .variant(CardVariant::Secondary)
                .child(
                    CardHeader::new()
                        .child(CardTitle::new().child("Import From Existing Database Clients"))
                        .child(CardDescription::new().child(
                            "Scans local application profiles on this PC without modifying your existing clients. Duplicate endpoints are skipped automatically.",
                        )),
                )
                .child(
                    CardContent::new().child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(COMPACT_GAP_PX))
                            .child(
                                Button::new("library-rescan-importers")
                                    .variant(Variant::Secondary)
                                    .size(Size::Sm)
                                    .label("Rescan Locations")
                                    .child(Icon::new(IconName::RefreshCw).size(IconSize::Sm))
                                    .on_press(cx.listener(|ws, _, _, cx| {
                                        rescan_foreign_inventories(ws);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("library-back-configure")
                                    .variant(Variant::Tertiary)
                                    .size(Size::Sm)
                                    .label("Back to Form")
                                    .on_press(cx.listener(|ws, _, _, cx| {
                                        ws.library_ui.active_tab = SharedString::from(TAB_CONFIGURE);
                                        cx.notify();
                                    })),
                            ),
                    ),
                ),
        );

    if let Some(report) = &ws.library_ui.last_import_report {
        let alert = match report {
            Ok(rep) => {
                let conn_word = plural_word(rep.imported_count, "connection", "connections");
                let dup_word = plural_word(rep.skipped_duplicates, "duplicate", "duplicates");
                Alert::new(format!("Imported from {}", rep.source_app.display_name()))
                    .status(Color::Success)
                    .description(format!(
                        "{} {conn_word} added ({} {dup_word} skipped).",
                        rep.imported_count, rep.skipped_duplicates
                    ))
            }
            Err(err) => Alert::new("Import Failed")
                .status(Color::Danger)
                .description(err.clone()),
        };
        root = root.child(alert);
    }

    for &(app, inv) in &ws.library_ui.inventories {
        root = root.child(render_importer_card(app, inv, cx));
    }

    root.into_any_element()
}

fn importer_card_copy(
    app: ForeignApp,
    inv: &Inventory,
) -> (String, &'static str, &'static str, bool, bool) {
    let windows_supported = app.runs_on(ForeignPlatform::Windows);
    let needs_file_picker = app.picked_file_extensions().is_some();
    let count_summary = if !windows_supported {
        "macOS-only client store (no local Windows profile)".to_owned()
    } else if needs_file_picker {
        "Export a .ncx file from Navicat to import saved connections.".to_owned()
    } else if inv.connections == 0 && inv.saved_queries == 0 {
        "No connections detected in default app directory.".to_owned()
    } else {
        format!(
            "{} {} · {} {}",
            inv.connections,
            plural_word(inv.connections, "connection", "connections"),
            inv.saved_queries,
            plural_word(inv.saved_queries, "saved query", "saved queries"),
        )
    };
    let pwd_note = match app.password_source() {
        PasswordSource::Keychain => "Keychain passwords",
        PasswordSource::AppFiles => "App-managed files",
    };
    let btn_label = if needs_file_picker {
        "Choose .ncx Export..."
    } else {
        "Import"
    };
    let can_import =
        windows_supported && (needs_file_picker || inv.connections > 0 || inv.saved_queries > 0);
    (
        count_summary,
        pwd_note,
        btn_label,
        windows_supported,
        can_import,
    )
}

fn render_importer_card(
    app: ForeignApp,
    inv: Inventory,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let (count_summary, pwd_note, btn_label, windows_supported, can_import) =
        importer_card_copy(app, &inv);
    let btn_id = format!("import-app-{}", app.id());

    Card::new()
        .variant(CardVariant::Default)
        .child(
            CardContent::new().child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(COMPACT_GAP_PX))
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(app.display_name()),
                                    )
                                    .child(
                                        Chip::new()
                                            .variant(ChipVariant::Soft)
                                            .color(if inv.connections > 0 {
                                                Color::Accent
                                            } else {
                                                Color::Default
                                            })
                                            .size(Size::Sm)
                                            .child(ChipLabel::new().child(pwd_note)),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.colors().muted)
                                    .font_features(tabular_font_features())
                                    .child(count_summary),
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(btn_id))
                            .variant(if can_import {
                                Variant::Primary
                            } else {
                                Variant::Secondary
                            })
                            .size(Size::Sm)
                            .is_disabled(!windows_supported)
                            .label(btn_label)
                            .child(Icon::new(IconName::Download).size(IconSize::Sm))
                            .on_press(cx.listener(move |ws, _, _, cx| {
                                trigger_foreign_import(ws, app, cx);
                            })),
                    ),
            ),
        )
        .into_any_element()
}

fn path_browse_row(
    entity: &Entity<InputState>,
    label: &str,
    placeholder: &str,
    button_id: &'static str,
    target: BrowseTarget,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    div()
        .flex()
        .items_end()
        .gap(px(COMPACT_GAP_PX))
        .child(
            div().flex_1().min_w_0().child(
                Input::new(entity)
                    .variant(FieldVariant::Secondary)
                    .label(label.to_owned())
                    .placeholder(placeholder.to_owned())
                    .full_width(),
            ),
        )
        .child(
            Button::new(button_id)
                .variant(Variant::Secondary)
                .label("Browse...")
                .on_press(cx.listener(move |_ws, _, _, cx| {
                    trigger_browse_path(target, cx);
                })),
        )
        .into_any_element()
}

fn select_saved_connection(
    ws: &mut DataForgeWorkspace,
    conn_id: &str,
    cx: &mut Context<DataForgeWorkspace>,
) {
    if let Err(err) = ws.controller.hydrate_connection_form_from_library(conn_id) {
        ws.notify_toast(
            ToastSeverity::Error,
            "Failed to load connection",
            Some(err.to_string().into()),
            cx,
        );
        return;
    }

    ws.library_ui.selected_connection_id = Some(conn_id.to_owned());
    ws.library_ui.delete_armed_for = None;
    ws.library_ui.test_outcome = None;
    ws.library_ui.test_in_flight = false;
    ws.library_ui.has_stored_db_password = false;
    ws.library_ui.has_stored_ssh_passphrase = false;
    set_input_text(&ws.library_ui.password_input, "", cx);
    set_input_text(&ws.library_ui.ssh_passphrase_input, "", cx);

    let draft = ws.controller.connection_form.clone();
    ws.library_ui.sync_from_controller(&draft, cx);
    let saved = ws.library_ui.saved_records.get(conn_id).cloned();
    let tags = ws
        .controller
        .library
        .storage()
        .connections
        .get(&LibraryConnectionId(conn_id.to_owned()))
        .map(|c| c.tags.clone());
    ws.library_ui
        .sync_extended_from_saved(saved.as_ref(), tags.as_ref(), cx);

    probe_stored_secrets(ws, conn_id, cx);
    cx.notify();
}

fn probe_stored_secrets(
    ws: &DataForgeWorkspace,
    conn_id: &str,
    cx: &mut Context<DataForgeWorkspace>,
) {
    let Ok(uuid) = Uuid::parse_str(conn_id) else {
        return;
    };
    let Some(services) = ws.controller.services.clone() else {
        return;
    };
    let expected_id = conn_id.to_owned();
    cx.spawn(
        async move |this: WeakEntity<DataForgeWorkspace>, cx: &mut AsyncApp| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let creds = services.credentials.clone();
            drop(services.runtime.spawn(async move {
                let db_stored = creds
                    .load(uuid, SecretKind::DatabasePassword)
                    .await
                    .is_ok_and(|lookup| lookup.is_stored());
                let ssh_stored = creds
                    .load(uuid, SecretKind::SshPassphrase)
                    .await
                    .is_ok_and(|lookup| lookup.is_stored());
                let _ = tx.send((db_stored, ssh_stored));
            }));
            if let Ok((db_stored, ssh_stored)) = rx.await {
                let _ = this.update(cx, |ws, cx| {
                    if ws.library_ui.selected_connection_id.as_deref() == Some(expected_id.as_str())
                    {
                        ws.library_ui.has_stored_db_password = db_stored;
                        ws.library_ui.has_stored_ssh_passphrase = ssh_stored;
                        cx.notify();
                    }
                });
            }
        },
    )
    .detach();
}

fn apply_engine_change(
    ws: &mut DataForgeWorkspace,
    next_engine: EngineKind,
    cx: &mut Context<DataForgeWorkspace>,
) {
    let prev_default = default_port_for_engine(ws.controller.connection_form.engine);
    let next_default = default_port_for_engine(next_engine);
    ws.controller.connection_form.engine = next_engine;
    ws.library_ui.last_synced_draft.engine = next_engine;

    if ws.controller.connection_form.port == prev_default || ws.controller.connection_form.port == 0
    {
        ws.controller.connection_form.port = next_default;
        ws.library_ui.last_synced_draft.port = next_default;
        let next_str = if next_default == 0 {
            String::new()
        } else {
            next_default.to_string()
        };
        set_input_text(&ws.library_ui.port_input, &next_str, cx);
    }
    ws.library_ui.test_outcome = None;
    cx.notify();
}

fn trigger_parse_url(
    ws: &mut DataForgeWorkspace,
    raw_url: &str,
    cx: &mut Context<DataForgeWorkspace>,
) {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        ws.notify_toast(
            ToastSeverity::Warning,
            "Enter a connection URL first",
            None,
            cx,
        );
        return;
    }

    match parse_connection_url(trimmed) {
        Ok(parsed) => {
            if let Err(err) = ws.controller.parse_and_apply_connection_url(trimmed) {
                ws.notify_toast(
                    ToastSeverity::Error,
                    "Failed to parse URL",
                    Some(err.to_string().into()),
                    cx,
                );
                return;
            }
            if let Some(pwd) = parsed.password.as_ref() {
                set_input_text(&ws.library_ui.password_input, pwd.expose_secret(), cx);
            }
            if let Some(mode) = parsed.tls.mode {
                let ui_mode = match mode {
                    CoreTlsMode::Disable => TlsMode::Disable,
                    CoreTlsMode::Prefer => TlsMode::Prefer,
                    CoreTlsMode::Require => TlsMode::Require,
                    CoreTlsMode::VerifyFull | CoreTlsMode::Strict => TlsMode::VerifyFull,
                };
                ws.controller.connection_form.tls_mode = ui_mode;
            }
            if let Some(ca) = parsed.tls.ca_file.as_ref() {
                set_input_text(&ws.library_ui.tls_ca_input, &ca.to_string_lossy(), cx);
            }
            if let Some(cert) = parsed.tls.client_certificate.as_ref() {
                set_input_text(&ws.library_ui.tls_cert_input, &cert.to_string_lossy(), cx);
            }
            if let Some(key) = parsed.tls.client_key.as_ref() {
                set_input_text(&ws.library_ui.tls_key_input, &key.to_string_lossy(), cx);
            }
            if let Some(sni) = parsed.tls.server_name.as_deref() {
                set_input_text(&ws.library_ui.tls_server_name_input, sni, cx);
            }
            ws.library_ui.read_only = parsed.read_only;
            ws.library_ui.dns_seed_list = parsed.options.dns_seed_list;
            ws.library_ui.channel_binding = parsed.options.channel_binding;
            set_input_text(
                &ws.library_ui.auth_database_input,
                parsed.options.auth_database.as_deref().unwrap_or_default(),
                cx,
            );
            set_input_text(
                &ws.library_ui.replica_set_input,
                parsed.options.replica_set.as_deref().unwrap_or_default(),
                cx,
            );

            let draft = ws.controller.connection_form.clone();
            ws.library_ui.sync_from_controller(&draft, cx);
            let note = if parsed.unread.is_empty() {
                None
            } else {
                Some(SharedString::from(format!(
                    "Ignored unrecognized parameters: {}",
                    parsed.unread.join(", ")
                )))
            };
            ws.notify_toast(ToastSeverity::Success, "Connection URL applied", note, cx);
            cx.notify();
        }
        Err(err) => {
            ws.controller.connection_form.last_error = Some(err.to_string());
            ws.library_ui.last_synced_draft.last_error = Some(err.to_string());
            ws.notify_toast(
                ToastSeverity::Error,
                "Invalid connection URL",
                Some(err.to_string().into()),
                cx,
            );
            cx.notify();
        }
    }
}

fn save_current_connection(ws: &mut DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) {
    let conn_id = ws
        .library_ui
        .selected_connection_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let tags = parse_tags_input(&ws.library_ui, cx);
    let existing = ws
        .controller
        .library
        .storage()
        .connections
        .get(&LibraryConnectionId(conn_id.clone()))
        .cloned();
    let is_favorite = existing.as_ref().is_some_and(|c| c.is_favorite);
    let group_id = existing
        .as_ref()
        .and_then(|c| c.group_id.as_ref().map(|g| g.0.clone()));

    ws.controller
        .save_form_to_library(&conn_id, group_id.as_deref(), tags, is_favorite);
    ws.library_ui.selected_connection_id = Some(conn_id.clone());
    ws.library_ui.delete_armed_for = None;

    let Some(saved) = build_saved_connection_record(ws, &conn_id, is_favorite, cx) else {
        ws.notify_toast(
            ToastSeverity::Success,
            "Connection updated in library",
            None,
            cx,
        );
        cx.notify();
        return;
    };

    ws.library_ui
        .saved_records
        .insert(conn_id.clone(), saved.clone());
    persist_saved_connection_and_secrets(ws, saved, is_favorite, cx);
    cx.notify();
}

fn build_saved_connection_record(
    ws: &DataForgeWorkspace,
    conn_id: &str,
    _is_favorite: bool,
    cx: &App,
) -> Option<SavedConnection> {
    let uuid = Uuid::parse_str(conn_id).ok()?;
    let prev = ws.library_ui.saved_records.get(conn_id);
    let draft = &ws.controller.connection_form;
    let display_name = if draft.name.trim().is_empty() {
        format!("{} ({})", draft.engine.display_name(), draft.database)
    } else {
        draft.name.trim().to_owned()
    };

    let tls = build_tls_config(&ws.library_ui, draft, cx).ok()?;
    let ssh_passphrase_typed = !ws
        .library_ui
        .ssh_passphrase_input
        .read(cx)
        .value()
        .is_empty();
    let has_passphrase = ssh_passphrase_typed || ws.library_ui.has_stored_ssh_passphrase;
    let ssh = if draft.ssh_enabled && !draft.ssh_host.trim().is_empty() {
        let auth = if draft.ssh_key_path.trim().is_empty() {
            SavedSshAuth::Agent
        } else {
            SavedSshAuth::PrivateKey {
                path: Some(PathBuf::from(draft.ssh_key_path.trim())),
                has_passphrase,
            }
        };
        Some(SavedSshConfig {
            host: draft.ssh_host.trim().to_owned(),
            port: Some(draft.ssh_port.max(1)),
            username: (!draft.ssh_user.trim().is_empty()).then(|| draft.ssh_user.trim().to_owned()),
            jump_hosts: prev
                .and_then(|p| p.ssh.as_ref())
                .map(|s| s.jump_hosts.clone())
                .unwrap_or_default(),
            auth,
        })
    } else {
        None
    };

    let safe_mode = if ws.library_ui.read_only {
        SafeModeLevel::ReadOnly
    } else {
        ws.controller.active_safe_mode()
    };

    let timeout_secs = ws
        .library_ui
        .timeout_secs_input
        .read(cx)
        .value()
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|&v| v > 0);

    Some(SavedConnection {
        id: uuid,
        name: display_name,
        driver_id: driver_id_from_engine(draft.engine).to_owned(),
        host: draft.host.trim().to_owned(),
        port: draft.port,
        database: draft.database.trim().to_owned(),
        username: draft.username.trim().to_owned(),
        tls,
        safe_mode,
        external_access: prev.map_or(ExternalAccess::ReadOnly, |p| p.external_access),
        ai_policy: prev.map_or(AiPolicy::Ask, |p| p.ai_policy),
        query_timeout_secs: timeout_secs,
        auth_mode: match draft.auth_mode {
            AuthMode::SspiIntegrated => CoreAuthMode::Kerberos,
            AuthMode::Password | AuthMode::Certificate => CoreAuthMode::Password,
        },
        ssh,
        last_opened_at: prev.and_then(|p| p.last_opened_at),
        color: ws.library_ui.selected_color,
        group_id: prev.and_then(|p| p.group_id),
        order: prev.map_or(0, |p| p.order),
        tags: prev.map(|p| p.tags.clone()).unwrap_or_default(),
        icon: prev.and_then(|p| p.icon),
        options: build_network_options(&ws.library_ui, cx),
    })
}

fn apply_saved_secrets_outcome(
    ws: &mut DataForgeWorkspace,
    conn_id_str: &str,
    outcome: Result<(), String>,
    cx: &mut Context<DataForgeWorkspace>,
) {
    match outcome {
        Ok(()) => {
            if ws.library_ui.selected_connection_id.as_deref() == Some(conn_id_str) {
                if !ws.library_ui.password_input.read(cx).value().is_empty() {
                    ws.library_ui.has_stored_db_password = true;
                    set_input_text(&ws.library_ui.password_input, "", cx);
                }
                if !ws
                    .library_ui
                    .ssh_passphrase_input
                    .read(cx)
                    .value()
                    .is_empty()
                {
                    ws.library_ui.has_stored_ssh_passphrase = true;
                    set_input_text(&ws.library_ui.ssh_passphrase_input, "", cx);
                }
            }
            ws.notify_toast(
                ToastSeverity::Success,
                "Connection & credentials saved",
                None,
                cx,
            );
            cx.notify();
        }
        Err(err) => {
            ws.notify_toast(
                ToastSeverity::Error,
                "Saved profile, credential store failed",
                Some(err.into()),
                cx,
            );
        }
    }
}

fn persist_saved_connection_and_secrets(
    ws: &mut DataForgeWorkspace,
    saved: SavedConnection,
    is_favorite: bool,
    cx: &mut Context<DataForgeWorkspace>,
) {
    let Some(services) = ws.controller.services.clone() else {
        return;
    };
    if let Err(err) = services.connection_store.upsert_blocking(saved.clone()) {
        ws.notify_toast(
            ToastSeverity::Error,
            "Failed to persist connection",
            Some(err.to_string().into()),
            cx,
        );
        return;
    }
    if is_favorite {
        let _ = services
            .connection_store
            .add_favorites_blocking(&[saved.id], None);
    }

    let db_pwd = ws.library_ui.password_input.read(cx).value().to_owned();
    let ssh_pass = ws
        .library_ui
        .ssh_passphrase_input
        .read(cx)
        .value()
        .to_owned();

    if db_pwd.is_empty() && ssh_pass.is_empty() {
        ws.notify_toast(ToastSeverity::Success, "Connection saved", None, cx);
        return;
    }

    let conn_id_str = saved.id.to_string();
    let db_label = saved.secret_target(SecretKind::DatabasePassword);
    let ssh_label = saved.secret_target(SecretKind::SshPassphrase);
    cx.spawn(
        async move |this: WeakEntity<DataForgeWorkspace>, cx: &mut AsyncApp| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let creds = services.credentials.clone();
            drop(services.runtime.spawn(async move {
                let mut result: Result<(), String> = Ok(());
                if !db_pwd.is_empty()
                    && let Err(err) = creds
                        .store(
                            saved.id,
                            SecretKind::DatabasePassword,
                            &SecretString::from(db_pwd),
                            &db_label,
                        )
                        .await
                {
                    result = Err(err.to_string());
                }
                if result.is_ok()
                    && !ssh_pass.is_empty()
                    && let Err(err) = creds
                        .store(
                            saved.id,
                            SecretKind::SshPassphrase,
                            &SecretString::from(ssh_pass),
                            &ssh_label,
                        )
                        .await
                {
                    result = Err(err.to_string());
                }
                let _ = tx.send(result);
            }));
            if let Ok(outcome) = rx.await {
                let _ = this.update(cx, |ws, cx| {
                    apply_saved_secrets_outcome(ws, &conn_id_str, outcome, cx);
                });
            }
        },
    )
    .detach();
}

fn toggle_saved_favorite(
    ws: &mut DataForgeWorkspace,
    conn_id: &str,
    cx: &mut Context<DataForgeWorkspace>,
) {
    let now_fav = ws.controller.toggle_library_favorite(conn_id);
    if let Some(services) = ws.controller.services.as_ref()
        && let Ok(uuid) = Uuid::parse_str(conn_id)
    {
        if now_fav {
            let _ = services
                .connection_store
                .add_favorites_blocking(&[uuid], None);
        } else {
            let _ = services.connection_store.remove_favorites_blocking(&[uuid]);
        }
    }
    cx.notify();
}

fn handle_delete_press(
    ws: &mut DataForgeWorkspace,
    conn_id: &str,
    cx: &mut Context<DataForgeWorkspace>,
) {
    if ws.library_ui.delete_armed_for.as_deref() != Some(conn_id) {
        ws.library_ui.delete_armed_for = Some(conn_id.to_owned());
        cx.notify();
        return;
    }

    ws.library_ui.delete_armed_for = None;
    let mut fresh = ws.controller.library.storage().clone();
    fresh
        .connections
        .remove(&LibraryConnectionId(conn_id.to_owned()));
    ws.controller.library.request_reload(fresh);
    let _ = ws.controller.library.apply_pending_reload_if_needed();
    ws.library_ui.saved_records.remove(conn_id);

    if let Some(services) = ws.controller.services.clone()
        && let Ok(uuid) = Uuid::parse_str(conn_id)
    {
        if let Err(err) = services.connection_store.remove_blocking(uuid) {
            ws.notify_toast(
                ToastSeverity::Error,
                "Failed to delete connection",
                Some(err.to_string().into()),
                cx,
            );
            return;
        }
        let creds = services.credentials.clone();
        drop(services.runtime.spawn(async move {
            let _ = creds.delete_connection(uuid).await;
        }));
    }

    ws.library_ui
        .reset_to_new(&mut ws.controller.connection_form, cx);
    ws.notify_toast(ToastSeverity::Success, "Connection removed", None, cx);
    cx.notify();
}

fn trigger_test_connection(ws: &mut DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) {
    let Some(services) = ws.controller.services.clone() else {
        ws.library_ui.test_outcome = Some(Err("Service runtime is not initialized.".to_owned()));
        cx.notify();
        return;
    };

    let typed_password = ws.library_ui.password_input.read(cx).value().to_owned();
    let typed_ssh_passphrase = ws
        .library_ui
        .ssh_passphrase_input
        .read(cx)
        .value()
        .to_owned();
    let saved_uuid = ws
        .library_ui
        .selected_connection_id
        .as_deref()
        .and_then(|id| Uuid::parse_str(id).ok());
    let draft = ws.controller.connection_form.clone();
    let tls_override = build_tls_config(&ws.library_ui, &draft, cx);
    let network_options = build_network_options(&ws.library_ui, cx);
    let read_only = ws.library_ui.read_only;

    ws.library_ui.test_in_flight = true;
    ws.library_ui.test_outcome = None;
    ws.library_ui.test_generation = ws.library_ui.test_generation.saturating_add(1);
    let test_gen = ws.library_ui.test_generation;
    cx.notify();

    let params = TestConnectParams {
        draft,
        saved_uuid,
        typed_password,
        typed_ssh_passphrase,
        tls_override,
        network_options,
        read_only,
    };

    cx.spawn(
        async move |this: WeakEntity<DataForgeWorkspace>, cx: &mut AsyncApp| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let svc = services.clone();
            drop(services.runtime.spawn(async move {
                let outcome = run_test_connection_async(&svc, params).await;
                let _ = tx.send(outcome);
            }));

            if let Ok(outcome) = rx.await {
                let _ = this.update(cx, |ws, cx| {
                    if ws.library_ui.test_generation != test_gen {
                        return;
                    }
                    ws.library_ui.test_in_flight = false;
                    ws.library_ui.test_outcome = Some(outcome);
                    cx.notify();
                });
            }
        },
    )
    .detach();
}

struct TestConnectParams {
    draft: ConnectionFormDraft,
    saved_uuid: Option<Uuid>,
    typed_password: String,
    typed_ssh_passphrase: String,
    tls_override: Result<TlsConfig, String>,
    network_options: NetworkOptions,
    read_only: bool,
}

async fn run_test_connection_async(
    services: &crate::services::ServiceHub,
    params: TestConnectParams,
) -> Result<ConnectionTestResult, String> {
    let TestConnectParams {
        draft,
        saved_uuid,
        typed_password,
        typed_ssh_passphrase,
        tls_override,
        network_options,
        read_only,
    } = params;
    let tls = tls_override?;
    let password = resolve_secret_or_stored(
        services,
        saved_uuid,
        SecretKind::DatabasePassword,
        typed_password,
    )
    .await;
    let ssh_passphrase = resolve_secret_or_stored(
        services,
        saved_uuid,
        SecretKind::SshPassphrase,
        typed_ssh_passphrase,
    )
    .await;

    let mut request =
        build_connect_request(&services.runtime, &draft, password, SafeModeLevel::Off)
            .map_err(|e| e.to_string())?;
    if read_only {
        request.open_read_only();
    }
    if let RequestTarget::Network {
        tls: req_tls,
        options,
        ssh,
        ..
    } = &mut request.target
    {
        *req_tls = tls;
        *options = network_options;
        if let Some(ssh_cfg) = ssh.as_mut()
            && let SshAuth::PrivateKey { passphrase, .. } = &mut ssh_cfg.auth
            && ssh_passphrase.is_some()
        {
            *passphrase = ssh_passphrase;
        }
    }

    let started = Instant::now();
    let cancel = CancellationToken::new();
    let pending = services
        .runtime
        .connector()
        .establish(request, CredentialInteraction::Unattended, cancel)
        .await
        .map_err(|err| err.to_string())?;

    let latency_ms = started.elapsed().as_millis();
    let info = pending.established().connection.server_info().clone();
    let schema_count = pending.schemas().len();
    let _ = pending.discard().await;

    let server_label = match (info.product.trim(), info.version.trim()) {
        ("", "") => draft.engine.display_name().to_owned(),
        (p, "") => p.to_owned(),
        ("", v) => format!("{} {v}", draft.engine.display_name()),
        (p, v) => format!("{p} {v}"),
    };
    Ok(ConnectionTestResult {
        server_label,
        latency_ms,
        schema_count,
    })
}

fn trigger_connect_current(ws: &mut DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) {
    let typed_password = ws.library_ui.password_input.read(cx).value().to_owned();
    let saved_uuid = ws
        .library_ui
        .selected_connection_id
        .as_deref()
        .and_then(|id| Uuid::parse_str(id).ok());

    if !typed_password.is_empty() || saved_uuid.is_none() || !ws.library_ui.has_stored_db_password {
        let secret = (!typed_password.is_empty()).then(|| SecretString::from(typed_password));
        start_connect_and_switch(ws, secret, cx);
        return;
    }

    let Some(services) = ws.controller.services.clone() else {
        start_connect_and_switch(ws, None, cx);
        return;
    };
    let Some(uuid) = saved_uuid else {
        start_connect_and_switch(ws, None, cx);
        return;
    };

    cx.spawn(
        async move |this: WeakEntity<DataForgeWorkspace>, cx: &mut AsyncApp| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let creds = services.credentials.clone();
            drop(services.runtime.spawn(async move {
                let pwd = creds
                    .load(uuid, SecretKind::DatabasePassword)
                    .await
                    .ok()
                    .and_then(SecretLookup::found);
                let _ = tx.send(pwd);
            }));
            if let Ok(secret) = rx.await {
                let _ = this.update(cx, |ws, cx| {
                    start_connect_and_switch(ws, secret, cx);
                });
            }
        },
    )
    .detach();
}

fn start_connect_and_switch(
    ws: &mut DataForgeWorkspace,
    password: Option<SecretString>,
    cx: &mut Context<DataForgeWorkspace>,
) {
    ws.controller.connection_form.last_error = None;
    match ws.controller.connect_from_draft_async(password) {
        Ok(_token) => {
            ws.switch_screen(ScreenMode::Workspace, cx);
        }
        Err(err) => {
            ws.controller.connection_form.last_error = Some(err.to_string());
            ws.library_ui.last_synced_draft.last_error = Some(err.to_string());
            ws.notify_toast(
                ToastSeverity::Error,
                "Unable to start connection",
                Some(err.to_string().into()),
                cx,
            );
            cx.notify();
        }
    }
}

fn connect_saved_connection_blocking(
    ws: &mut DataForgeWorkspace,
    conn_id: &str,
    cx: &mut Context<DataForgeWorkspace>,
) {
    match ws.controller.connect_library_item_blocking(conn_id) {
        Ok(()) => {
            ws.switch_screen(ScreenMode::Workspace, cx);
        }
        Err(err) => {
            ws.controller.connection_form.last_error = Some(err.to_string());
            ws.notify_toast(
                ToastSeverity::Error,
                "Connection failed",
                Some(err.to_string().into()),
                cx,
            );
            cx.notify();
        }
    }
}

fn trigger_open_local_file(cx: &mut Context<DataForgeWorkspace>) {
    let rx = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Open SQLite or DuckDB Database".into()),
    });

    cx.spawn(
        async move |this: WeakEntity<DataForgeWorkspace>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |ws, cx| {
                match ws.controller.connect_local_file_blocking(&path) {
                    Ok(_token) => {
                        let draft = ws.controller.connection_form.clone();
                        ws.library_ui.sync_from_controller(&draft, cx);
                        ws.switch_screen(ScreenMode::Workspace, cx);
                    }
                    Err(err) => {
                        ws.notify_toast(
                            ToastSeverity::Error,
                            "Failed to open database file",
                            Some(err.to_string().into()),
                            cx,
                        );
                    }
                }
            });
        },
    )
    .detach();
}

#[derive(Debug, Clone, Copy)]
enum BrowseTarget {
    DatabaseFile,
    TlsCaFile,
    TlsClientCert,
    TlsClientKey,
    SshKeyFile,
}

fn trigger_browse_path(target: BrowseTarget, cx: &mut Context<DataForgeWorkspace>) {
    let prompt = match target {
        BrowseTarget::DatabaseFile => "Select Database File",
        BrowseTarget::TlsCaFile => "Select TLS CA Certificate",
        BrowseTarget::TlsClientCert => "Select TLS Client Certificate",
        BrowseTarget::TlsClientKey => "Select TLS Client Private Key",
        BrowseTarget::SshKeyFile => "Select SSH Private Key",
    };
    let rx = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some(prompt.into()),
    });

    cx.spawn(
        async move |this: WeakEntity<DataForgeWorkspace>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let value = path.to_string_lossy().into_owned();
            let _ = this.update(cx, |ws, cx| {
                match target {
                    BrowseTarget::DatabaseFile => {
                        set_input_text(&ws.library_ui.database_input, &value, cx);
                        ws.controller.connection_form.database = value.clone();
                        ws.library_ui.last_synced_draft.database = value;
                    }
                    BrowseTarget::TlsCaFile => {
                        set_input_text(&ws.library_ui.tls_ca_input, &value, cx)
                    }
                    BrowseTarget::TlsClientCert => {
                        set_input_text(&ws.library_ui.tls_cert_input, &value, cx);
                    }
                    BrowseTarget::TlsClientKey => {
                        set_input_text(&ws.library_ui.tls_key_input, &value, cx);
                    }
                    BrowseTarget::SshKeyFile => {
                        set_input_text(&ws.library_ui.ssh_key_input, &value, cx);
                        ws.controller.connection_form.ssh_key_path = value.clone();
                        ws.library_ui.last_synced_draft.ssh_key_path = value;
                    }
                }
                cx.notify();
            });
        },
    )
    .detach();
}

fn trigger_foreign_import(
    ws: &mut DataForgeWorkspace,
    app: ForeignApp,
    cx: &mut Context<DataForgeWorkspace>,
) {
    if app.picked_file_extensions().is_none() {
        run_foreign_import(ws, app, None, cx);
        return;
    }

    let rx = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some(format!("Select {} Export File", app.display_name()).into()),
    });
    cx.spawn(
        async move |this: WeakEntity<DataForgeWorkspace>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(picked) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |ws, cx| {
                run_foreign_import(ws, app, Some(picked), cx);
            });
        },
    )
    .detach();
}

fn run_foreign_import(
    ws: &mut DataForgeWorkspace,
    app: ForeignApp,
    picked_file: Option<PathBuf>,
    cx: &mut Context<DataForgeWorkspace>,
) {
    let existing_ids: HashSet<LibraryConnectionId> = ws
        .controller
        .library
        .storage()
        .connections
        .keys()
        .cloned()
        .collect();

    match ws.controller.import_foreign_app_default(app, picked_file) {
        Ok(report) => {
            persist_newly_imported_connections(ws, &existing_ids);
            ws.library_ui.last_import_report = Some(Ok(report.clone()));
            ws.notify_toast(
                ToastSeverity::Success,
                format!("Imported {}", app.display_name()),
                Some(SharedString::from(format!(
                    "{} added, {} skipped",
                    report.imported_count, report.skipped_duplicates
                ))),
                cx,
            );
            rescan_foreign_inventories(ws);
        }
        Err(err) => {
            ws.library_ui.last_import_report = Some(Err(err.to_string()));
            ws.notify_toast(
                ToastSeverity::Error,
                format!("{} import failed", app.display_name()),
                Some(err.to_string().into()),
                cx,
            );
        }
    }
    cx.notify();
}

fn persist_newly_imported_connections(
    ws: &mut DataForgeWorkspace,
    previous_ids: &HashSet<LibraryConnectionId>,
) {
    let Some(services) = ws.controller.services.as_ref() else {
        return;
    };
    let connections = &ws.controller.library.storage().connections;
    for (id, conn) in connections {
        if previous_ids.contains(id) {
            continue;
        }
        let Ok(uuid) = Uuid::parse_str(&id.0) else {
            continue;
        };
        let saved = SavedConnection {
            id: uuid,
            name: conn.name.clone(),
            driver_id: conn.driver_id.clone(),
            host: conn.host.clone(),
            port: conn.port.unwrap_or(0),
            database: conn.database.clone(),
            username: conn.username.clone(),
            tls: TlsConfig::disabled(),
            safe_mode: ws.controller.active_safe_mode(),
            external_access: ExternalAccess::ReadOnly,
            ai_policy: AiPolicy::Ask,
            query_timeout_secs: None,
            auth_mode: CoreAuthMode::Password,
            ssh: None,
            last_opened_at: None,
            color: None,
            group_id: None,
            order: conn.order as u32,
            tags: Vec::new(),
            icon: None,
            options: NetworkOptions::default(),
        };
        if services
            .connection_store
            .upsert_blocking(saved.clone())
            .is_ok()
        {
            ws.library_ui.saved_records.insert(id.0.clone(), saved);
        }
    }
}

async fn resolve_secret_or_stored(
    services: &crate::services::ServiceHub,
    saved_uuid: Option<Uuid>,
    kind: SecretKind,
    typed: String,
) -> Option<SecretString> {
    if !typed.is_empty() {
        return Some(SecretString::from(typed));
    }
    let uuid = saved_uuid?;
    services
        .credentials
        .load(uuid, kind)
        .await
        .ok()
        .and_then(SecretLookup::found)
}

fn build_tls_config(
    ui: &LibraryScreenState,
    draft: &ConnectionFormDraft,
    cx: &App,
) -> Result<TlsConfig, String> {
    let mut cfg = match draft.tls_mode {
        TlsMode::Disable => TlsConfig::disabled(),
        TlsMode::VerifyFull | TlsMode::VerifyCa => TlsConfig::verify_full(),
        TlsMode::Require => {
            let mut c = TlsConfig::disabled();
            c.mode = CoreTlsMode::Require;
            c
        }
        TlsMode::Prefer => {
            let mut c = TlsConfig::disabled();
            c.mode = CoreTlsMode::Prefer;
            c
        }
    };

    let ca = ui.tls_ca_input.read(cx).value().trim().to_owned();
    let cert = ui.tls_cert_input.read(cx).value().trim().to_owned();
    let key = ui.tls_key_input.read(cx).value().trim().to_owned();
    let sni = ui.tls_server_name_input.read(cx).value().trim().to_owned();

    cfg.ca_file = (!ca.is_empty()).then(|| PathBuf::from(ca));
    cfg.server_name = (!sni.is_empty()).then_some(sni);
    cfg.client_identity = match (cert.is_empty(), key.is_empty()) {
        (true, true) => None,
        (false, false) => Some(ClientIdentity {
            certificate: PathBuf::from(cert),
            key: PathBuf::from(key),
        }),
        _ => {
            return Err(
                "Both TLS client certificate and private key paths are required.".to_owned(),
            );
        }
    };
    Ok(cfg)
}

fn build_network_options(ui: &LibraryScreenState, cx: &App) -> NetworkOptions {
    let auth_db = ui.auth_database_input.read(cx).value().trim().to_owned();
    let replica = ui.replica_set_input.read(cx).value().trim().to_owned();
    NetworkOptions {
        dns_seed_list: ui.dns_seed_list,
        auth_database: (!auth_db.is_empty()).then_some(auth_db),
        replica_set: (!replica.is_empty()).then_some(replica),
        more_hosts: Vec::new(),
        channel_binding: ui.channel_binding,
    }
}

fn parse_tags_input(ui: &LibraryScreenState, cx: &App) -> BTreeSet<String> {
    ui.tags_input
        .read(cx)
        .value()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn set_input_text(entity: &Entity<InputState>, value: &str, cx: &mut App) {
    entity.update(cx, |state, _| {
        state.set_value(value.to_owned());
    });
}

fn target_summary(draft: &ConnectionFormDraft) -> String {
    if matches!(draft.engine, EngineKind::Sqlite | EngineKind::Duckdb) {
        return truncate_middle(&draft.database, 40);
    }
    if draft.database.trim().is_empty() {
        format!("{}:{}", draft.host, draft.port)
    } else {
        format!("{}:{}/{}", draft.host, draft.port, draft.database)
    }
}

const fn plural_word<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
}

const fn color_display_name(color: ConnectionColor) -> &'static str {
    match color {
        ConnectionColor::Blue => "Blue",
        ConnectionColor::Teal => "Teal",
        ConnectionColor::Green => "Green",
        ConnectionColor::Yellow => "Yellow",
        ConnectionColor::Orange => "Orange",
        ConnectionColor::Red => "Red",
        ConnectionColor::Pink => "Pink",
        ConnectionColor::Purple => "Purple",
        ConnectionColor::Slate => "Slate",
    }
}

fn color_dot(color: ConnectionColor) -> AnyElement {
    let hsla = connection_color_hsla(color);
    div()
        .w(px(8.0))
        .h(px(8.0))
        .rounded(px(999.0))
        .flex_shrink_0()
        .bg(hsla)
        .border_1()
        .border_color(with_alpha(hsla, 0.35))
        .into_any_element()
}

fn connection_color_hsla(color: ConnectionColor) -> Hsla {
    let (r, g, b): (u8, u8, u8) = match color {
        ConnectionColor::Blue => (0x3B, 0x82, 0xF6),
        ConnectionColor::Teal => (0x14, 0xB8, 0xA6),
        ConnectionColor::Green => (0x22, 0xC5, 0x5E),
        ConnectionColor::Yellow => (0xEA, 0xB3, 0x08),
        ConnectionColor::Orange => (0xF9, 0x73, 0x16),
        ConnectionColor::Red => (0xEF, 0x44, 0x44),
        ConnectionColor::Pink => (0xEC, 0x48, 0x99),
        ConnectionColor::Purple => (0xA8, 0x55, 0xF7),
        ConnectionColor::Slate => (0x64, 0x74, 0x8B),
    };
    Hsla::from(herogpui::gpui::Rgba {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        a: 1.0,
    })
}

fn truncate_middle(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars || max_chars < 5 {
        return text.to_owned();
    }
    let keep_head = (max_chars - 1) / 2;
    let keep_tail = max_chars - 1 - keep_head;
    let head: String = text.chars().take(keep_head).collect();
    let tail: String = text.chars().skip(count.saturating_sub(keep_tail)).collect();
    format!("{head}…{tail}")
}

fn tabular_font_features() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".to_owned(), 1)]))
}
