//! Cross-engine Data Transfer & Schema Migration workbench view.

use std::sync::Arc;

use herogpui::components::{
    Alert, Button, Checkbox, Chip, ChipLabel, ChipVariant, FieldVariant, Icon, IconName, IconSize,
    Input, InputState, InputType, PickerItem, Select, Spinner, SpinnerSize,
};
use herogpui::core::{Color as HeroColor, Size as HeroSize, Variant};
use herogpui::gpui::prelude::FluentBuilder as _;
use herogpui::gpui::{
    AnyElement, App, AppContext as _, AsyncApp, ClipboardItem, Context, Entity, FontWeight,
    InteractiveElement, IntoElement, ParentElement, SharedString, StatefulInteractiveElement,
    Styled, Task, WeakEntity, Window, div, px,
};
use herogpui::theme::ActiveTheme;
use secrecy::SecretString;
use tableminus_gfx::glyphs::EngineKind;
use tableminus_services::data_transfer::{
    ColumnTypeMapping, TransferConfig, TransferMode, TransferReport, format_sql_literal,
    map_sql_type, transfer_table_cancellable,
};
use tableminus_services::{
    AlterTableOp, ColumnSpec, CreateTableSpec, DatabaseEngine, build_alter_table_sql,
    build_create_table_sql,
};
use tableminus_state::connection_window::ConnectionWindowPhase;
use tableminus_state::library::LibraryConnection;
use tablepro_core::column::ColumnDefault;
use tablepro_core::credentials::{CredentialInteraction, SecretKind, SecretLookup, SecretVault};
use tablepro_core::ddl::{ColumnAttributes, ColumnDefinition, TableDefinition};
use tablepro_core::meta::{ObjectInfo, ObjectKind, ObjectScope, TableRef};
use tablepro_core::sql_syntax::SqlGrammar;
use tablepro_core::{AuthMode as CoreAuthMode, SafeModeLevel, TlsMode as CoreTlsMode};
use tablepro_session::connect::{
    ConnectRequest, Connector as _, CredentialSource, NoStoredCredentials, RequestTarget,
};
use tablepro_session::operation::{OperationError, OperationKind};
use tablepro_session::{Session, SessionDeps};
use tablepro_storage::{KeyringCredentials, SavedSshAuth};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::TableMinusWorkspace;
use super::engine_icons::engine_icon;
use super::shell_state::{push_error_toast, push_success_toast};
use super::syntax_bridge;
use super::theme::{
    CONTROL_RADIUS_PX, PANEL_RADIUS_PX, editor_gpui_font, hairline_border_color, is_reduced_motion,
    muted_surface_bg, panel_surface_bg, row_zebra_bg, value_gpui_font,
};
use crate::services::{ServiceHub, build_connect_request, default_port_for_engine};
use crate::views::library::{AuthMode, ConnectionFormDraft, TlsMode, engine_from_driver_id};

const ACTIVE_CONNECTION_KEY: &str = "__active__";
const DEFAULT_BATCH_SIZE: &str = "500";
const DEFAULT_BATCH_SIZE_NUM: usize = 500;

const TRANSFER_TARGET_ENGINES: [DatabaseEngine; 6] = [
    DatabaseEngine::Sqlite,
    DatabaseEngine::Postgres,
    DatabaseEngine::Mysql,
    DatabaseEngine::Mssql,
    DatabaseEngine::Duckdb,
    DatabaseEngine::Clickhouse,
];

#[derive(Debug)]
struct ColumnMappingDraft {
    source_column: String,
    source_type: String,
    nullable: bool,
    is_primary_key: bool,
    auto_increment: bool,
    default_value: ColumnDefault,
    include: bool,
    target_col_input: Entity<InputState>,
    target_type_input: Entity<InputState>,
}

/// Persistent UI state for the Cross-Engine Data Transfer workbench view.
#[derive(Debug)]
pub struct DataTransferViewState {
    source_connection_id: Option<String>,
    source_tables: Vec<TableRef>,
    source_tables_loaded_for: Option<String>,
    source_table: Option<TableRef>,
    source_engine: DatabaseEngine,
    source_def: Option<TableDefinition>,
    loaded_source_key: Option<String>,
    loading_source_metadata: bool,
    target_connection_id: Option<String>,
    target_engine: DatabaseEngine,
    target_schema_input: Entity<InputState>,
    target_table_input: Entity<InputState>,
    mode: TransferMode,
    batch_size_input: Entity<InputState>,
    column_mappings: Vec<ColumnMappingDraft>,
    is_running: bool,
    cancel_token: Option<CancellationToken>,
    last_report: Option<TransferReport>,
    last_error: Option<String>,
    metadata_task: Option<Task<()>>,
    transfer_task: Option<Task<()>>,
}

impl DataTransferViewState {
    /// Creates initial Cross-Engine Data Transfer view state owned by [`TableMinusWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, cx: &mut Context<TableMinusWorkspace>) -> Self {
        let target_schema_input = cx.new(|cx| InputState::new(cx));
        let target_table_input = cx.new(|cx| InputState::new(cx));
        let batch_size_input = cx.new(|cx| InputState::with_value(cx, DEFAULT_BATCH_SIZE));
        Self {
            source_connection_id: None,
            source_tables: Vec::new(),
            source_tables_loaded_for: None,
            source_table: None,
            source_engine: DatabaseEngine::Sqlite,
            source_def: None,
            loaded_source_key: None,
            loading_source_metadata: false,
            target_connection_id: None,
            target_engine: DatabaseEngine::Postgres,
            target_schema_input,
            target_table_input,
            mode: TransferMode::CreateAndLoad,
            batch_size_input,
            column_mappings: Vec::new(),
            is_running: false,
            cancel_token: None,
            last_report: None,
            last_error: None,
            metadata_task: None,
            transfer_task: None,
        }
    }
}

/// Renders the Cross-Engine Data Transfer & Schema Migration workbench view.
#[must_use]
pub fn render_data_transfer_view(
    ws: &mut TableMinusWorkspace,
    window: &mut Window,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let is_connected = is_active_workspace_connected(ws);
    sync_source_selection_and_metadata(ws, is_connected, cx);

    let rem_scale = (f32::from(window.rem_size()) / 16.0).clamp(0.75, 2.0);
    let viewport_width = f32::from(window.viewport_size().width) / rem_scale;
    let is_narrow_endpoints = viewport_width < 1040.0;
    let is_narrow_body = viewport_width < 880.0;

    div()
        .id("data-transfer-view")
        .size_full()
        .min_w_0()
        .min_h_0()
        .overflow_hidden()
        .flex()
        .flex_col()
        .bg(panel_surface_bg(cx))
        .child(render_header(ws, cx))
        .child(render_endpoints_bar(
            ws,
            is_connected,
            is_narrow_endpoints,
            cx,
        ))
        .child(render_options_bar(ws, is_connected, cx))
        .child(render_body(ws, is_connected, is_narrow_body, cx))
        .into_any_element()
}

fn is_active_workspace_connected(ws: &TableMinusWorkspace) -> bool {
    matches!(
        ws.controller
            .connections
            .phase(ws.controller.active_workspace),
        Some(ConnectionWindowPhase::Connected)
    )
}

fn sync_source_selection_and_metadata(
    ws: &mut TableMinusWorkspace,
    is_connected: bool,
    cx: &mut Context<TableMinusWorkspace>,
) {
    if ws.data_transfer_ui.source_connection_id.is_none() {
        if !is_connected {
            return;
        }
        let active_engine = ws.controller.active_database_engine();
        if ws.data_transfer_ui.loaded_source_key.is_none() {
            ws.data_transfer_ui.source_engine = active_engine;
            if ws.data_transfer_ui.target_connection_id.is_none()
                && ws.data_transfer_ui.source_def.is_none()
            {
                ws.data_transfer_ui.target_engine = active_engine;
            }
        }
        let tables = collect_transferable_tables(&ws.controller.cached_objects);
        ensure_default_source_table(
            &mut ws.data_transfer_ui,
            &tables,
            ws.controller.active_table.as_ref(),
        );
        if let Some(table) = ws.data_transfer_ui.source_table.clone() {
            let key = format!("{ACTIVE_CONNECTION_KEY}::{}", format_table_ref(&table));
            if ws.data_transfer_ui.loaded_source_key.as_deref() != Some(key.as_str())
                && !ws.data_transfer_ui.loading_source_metadata
            {
                start_load_source_definition(ws, None, table, cx);
            }
        }
        return;
    }

    let Some(conn_id) = ws.data_transfer_ui.source_connection_id.clone() else {
        return;
    };
    if ws.data_transfer_ui.source_tables_loaded_for.as_deref() != Some(conn_id.as_str())
        && !ws.data_transfer_ui.loading_source_metadata
    {
        start_load_saved_source_tables(ws, conn_id, cx);
        return;
    }
    if let Some(table) = ws.data_transfer_ui.source_table.clone() {
        let key = format!("{conn_id}::{}", format_table_ref(&table));
        if ws.data_transfer_ui.loaded_source_key.as_deref() != Some(key.as_str())
            && !ws.data_transfer_ui.loading_source_metadata
        {
            start_load_source_definition(ws, Some(conn_id), table, cx);
        }
    }
}

fn ensure_default_source_table(
    state: &mut DataTransferViewState,
    tables: &[TableRef],
    active_table: Option<&TableRef>,
) {
    if tables.is_empty() {
        return;
    }
    if state
        .source_table
        .as_ref()
        .is_none_or(|t| !tables.contains(t))
    {
        state.source_table = active_table
            .filter(|t| tables.contains(t))
            .cloned()
            .or_else(|| tables.first().cloned());
    }
}

fn render_header(ws: &TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();

    div()
        .min_h(px(46.0))
        .px(px(14.0))
        .py(px(7.0))
        .border_b_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(
                    Icon::new(IconName::ArrowLeftRight)
                        .size(IconSize::Sm)
                        .color(colors.accent.color),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .child(
                            div()
                                .truncate()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(colors.foreground)
                                .child(s.data_transfer_title),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(11.5))
                                .text_color(colors.muted)
                                .child(s.data_transfer_desc),
                        ),
                ),
        )
        .child(
            div().flex_shrink_0().child(
                Button::new("transfer-view-close")
                    .variant(Variant::Ghost)
                    .size(HeroSize::Sm)
                    .is_icon_only(true)
                    .aria_label(s.close_label)
                    .on_press(cx.listener(|ws, _, _, cx| {
                        ws.workspace_pane.close_tool();
                        cx.notify();
                    }))
                    .child(Icon::new(IconName::X).size(IconSize::Xs)),
            ),
        )
        .into_any_element()
}

fn render_endpoints_bar(
    ws: &TableMinusWorkspace,
    is_connected: bool,
    is_narrow: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let border = hairline_border_color(cx);
    let colors = cx.colors().clone();

    let bar = div()
        .px(px(14.0))
        .py(px(10.0))
        .bg(muted_surface_bg(cx))
        .border_b_1()
        .border_color(border)
        .min_w_0()
        .overflow_hidden()
        .flex();

    if is_narrow {
        bar.flex_col()
            .items_stretch()
            .gap(px(8.0))
            .child(render_source_section(ws, is_connected, is_narrow, cx))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_shrink_0()
                    .child(
                        Icon::new(IconName::ArrowDown)
                            .size(IconSize::Sm)
                            .color(colors.muted),
                    ),
            )
            .child(render_target_section(ws, is_connected, is_narrow, cx))
            .into_any_element()
    } else {
        bar.flex_row()
            .items_stretch()
            .gap(px(10.0))
            .child(render_source_section(ws, is_connected, is_narrow, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_shrink_0()
                    .child(
                        Icon::new(IconName::ArrowRight)
                            .size(IconSize::Sm)
                            .color(colors.muted),
                    ),
            )
            .child(render_target_section(ws, is_connected, is_narrow, cx))
            .into_any_element()
    }
}

fn endpoint_card(border: herogpui::gpui::Hsla, is_narrow: bool, cx: &App) -> herogpui::gpui::Div {
    let card = div()
        .min_w_0()
        .overflow_hidden()
        .rounded(px(PANEL_RADIUS_PX))
        .border_1()
        .border_color(border)
        .bg(panel_surface_bg(cx))
        .p(px(10.0))
        .flex()
        .flex_col()
        .gap(px(8.0));
    if is_narrow {
        card.w_full()
    } else {
        card.flex_1()
    }
}

fn apply_single_line_select_value(select: Select, fg_color: herogpui::gpui::Hsla) -> Select {
    select.value_content(move |val| {
        if val.is_placeholder {
            return val.default_children;
        }
        div()
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .text_color(fg_color)
            .child(val.selected_text.to_string())
            .into_any_element()
    })
}

fn render_source_section(
    ws: &TableMinusWorkspace,
    is_connected: bool,
    is_narrow: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let border = hairline_border_color(cx);
    let conn_items = build_connection_picker_items(ws, is_connected);
    let selected_conn = ws
        .data_transfer_ui
        .source_connection_id
        .as_deref()
        .unwrap_or(ACTIVE_CONNECTION_KEY)
        .to_owned();
    let is_busy = ws.data_transfer_ui.is_running || ws.data_transfer_ui.loading_source_metadata;

    endpoint_card(border, is_narrow, cx)
        .child(render_section_heading(
            s.transfer_source_section,
            ws.data_transfer_ui.source_engine,
            colors.muted,
        ))
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.0))
                .child(render_source_conn_select(
                    conn_items,
                    selected_conn,
                    ws.data_transfer_ui.is_running,
                    colors.foreground,
                    cx,
                ))
                .child(render_source_table_select(ws, colors.foreground, cx))
                .child(
                    div().flex_shrink_0().child(
                        Button::new("transfer-refresh-source")
                            .variant(Variant::Secondary)
                            .size(HeroSize::Sm)
                            .is_icon_only(true)
                            .is_disabled(is_busy)
                            .aria_label(s.refresh_label)
                            .on_press(cx.listener(|ws, _, _, cx| {
                                force_reload_source_metadata(ws, cx);
                            }))
                            .child(Icon::new(IconName::RefreshCw).size(IconSize::Xs)),
                    ),
                ),
        )
        .into_any_element()
}

fn render_source_conn_select(
    conn_items: Vec<PickerItem>,
    selected_conn: String,
    disabled: bool,
    fg_color: herogpui::gpui::Hsla,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let select = Select::new("transfer-source-conn", conn_items)
        .variant(FieldVariant::Secondary)
        .height(px(28.0))
        .trigger_text_size(px(12.0))
        .row_text_size(px(12.0))
        .full_width(true)
        .is_disabled(disabled)
        .value(Some(SharedString::from(selected_conn)))
        .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _, cx| {
            apply_source_connection_change(ws, key.as_deref(), cx);
        }));

    div()
        .flex_1()
        .min_w(px(140.0))
        .max_w_full()
        .overflow_hidden()
        .child(apply_single_line_select_value(select, fg_color))
        .into_any_element()
}

fn render_source_table_select(
    ws: &TableMinusWorkspace,
    fg_color: herogpui::gpui::Hsla,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let tables = active_source_tables(ws);
    let table_items: Vec<PickerItem> = tables
        .iter()
        .enumerate()
        .map(|(i, t)| PickerItem::new(i.to_string(), format_table_ref(t)))
        .collect();
    let selected_table_val = ws
        .data_transfer_ui
        .source_table
        .as_ref()
        .and_then(|t| tables.iter().position(|x| x == t))
        .map(|i| SharedString::from(i.to_string()));
    let disabled = tables.is_empty() || ws.data_transfer_ui.is_running;
    let tables_for_cb = tables;

    let select = Select::new("transfer-source-table", table_items)
        .variant(FieldVariant::Secondary)
        .placeholder(ws.controller.strings.tables)
        .height(px(28.0))
        .trigger_text_size(px(12.0))
        .row_text_size(px(12.0))
        .full_width(true)
        .is_disabled(disabled)
        .value(selected_table_val)
        .on_selection_change(cx.listener(move |ws, key: &Option<SharedString>, _, cx| {
            if let Some(idx) = key.as_deref().and_then(|k| k.parse::<usize>().ok())
                && let Some(t) = tables_for_cb.get(idx).cloned()
            {
                ws.data_transfer_ui.source_table = Some(t);
                ws.data_transfer_ui.loaded_source_key = None;
                ws.data_transfer_ui.last_error = None;
                cx.notify();
            }
        }));

    div()
        .flex_1()
        .min_w(px(140.0))
        .max_w_full()
        .overflow_hidden()
        .child(apply_single_line_select_value(select, fg_color))
        .into_any_element()
}

fn render_target_section(
    ws: &TableMinusWorkspace,
    is_connected: bool,
    is_narrow: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let border = hairline_border_color(cx);
    let conn_items = build_connection_picker_items(ws, is_connected);
    let selected_conn = ws
        .data_transfer_ui
        .target_connection_id
        .as_deref()
        .unwrap_or(ACTIVE_CONNECTION_KEY)
        .to_owned();
    let disabled = ws.data_transfer_ui.is_running;

    endpoint_card(border, is_narrow, cx)
        .child(render_section_heading(
            s.transfer_target_section,
            ws.data_transfer_ui.target_engine,
            colors.muted,
        ))
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.0))
                .child(render_target_conn_select(
                    conn_items,
                    selected_conn,
                    disabled,
                    colors.foreground,
                    cx,
                ))
                .child(render_target_engine_select(
                    ws.data_transfer_ui.target_engine,
                    disabled,
                    colors.foreground,
                    cx,
                ))
                .child(
                    div().w(px(110.0)).flex_shrink_0().overflow_hidden().child(
                        Input::new(&ws.data_transfer_ui.target_schema_input)
                            .variant(FieldVariant::Secondary)
                            .placeholder(s.default_schema)
                            .is_disabled(disabled)
                            .full_width()
                            .on_change(cx.listener(|_, _: &str, _, cx| cx.notify())),
                    ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(120.0))
                        .max_w_full()
                        .overflow_hidden()
                        .child(
                            Input::new(&ws.data_transfer_ui.target_table_input)
                                .variant(FieldVariant::Secondary)
                                .placeholder(s.import_target_table)
                                .is_disabled(disabled)
                                .full_width()
                                .on_change(cx.listener(|_, _: &str, _, cx| cx.notify())),
                        ),
                ),
        )
        .into_any_element()
}

fn render_target_conn_select(
    conn_items: Vec<PickerItem>,
    selected_conn: String,
    disabled: bool,
    fg_color: herogpui::gpui::Hsla,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let select = Select::new("transfer-target-conn", conn_items)
        .variant(FieldVariant::Secondary)
        .height(px(28.0))
        .trigger_text_size(px(12.0))
        .row_text_size(px(12.0))
        .full_width(true)
        .is_disabled(disabled)
        .value(Some(SharedString::from(selected_conn)))
        .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _, cx| {
            apply_target_connection_change(ws, key.as_deref(), cx);
        }));

    div()
        .flex_1()
        .min_w(px(140.0))
        .max_w_full()
        .overflow_hidden()
        .child(apply_single_line_select_value(select, fg_color))
        .into_any_element()
}

fn render_target_engine_select(
    target_engine: DatabaseEngine,
    disabled: bool,
    icon_color: herogpui::gpui::Hsla,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let engine_items: Vec<PickerItem> = TRANSFER_TARGET_ENGINES
        .iter()
        .map(|&eng| PickerItem::new(eng.as_str(), database_engine_to_kind(eng).display_name()))
        .collect();
    let target_engine_key = SharedString::from(target_engine.as_str());

    let select = Select::new("transfer-target-engine", engine_items)
        .variant(FieldVariant::Secondary)
        .height(px(28.0))
        .trigger_text_size(px(12.0))
        .row_text_size(px(12.0))
        .full_width(true)
        .is_disabled(disabled)
        .value(Some(target_engine_key))
        .item_leading(move |key: &SharedString, _selected: bool| {
            let kind = database_engine_to_kind(DatabaseEngine::from_driver_id(key.as_ref()));
            Some(engine_icon(kind, px(13.0), icon_color))
        })
        .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _, cx| {
            if let Some(eng_str) = key.as_deref() {
                let eng = DatabaseEngine::from_driver_id(eng_str);
                apply_target_engine_change(ws, eng, cx);
            }
        }));

    div()
        .w(px(125.0))
        .flex_shrink_0()
        .overflow_hidden()
        .child(apply_single_line_select_value(select, icon_color))
        .into_any_element()
}

fn render_section_heading(
    label: &'static str,
    engine: DatabaseEngine,
    muted: herogpui::gpui::Hsla,
) -> AnyElement {
    div()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(div().flex_shrink_0().child(engine_icon(
            database_engine_to_kind(engine),
            px(12.0),
            muted,
        )))
        .child(
            div()
                .min_w_0()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(muted)
                .child(label),
        )
        .into_any_element()
}

fn render_options_bar(
    ws: &TableMinusWorkspace,
    is_connected: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let border = hairline_border_color(cx);
    let can_start = can_start_transfer(ws, is_connected, cx);

    div()
        .px(px(14.0))
        .py(px(8.0))
        .border_b_1()
        .border_color(border)
        .min_w_0()
        .overflow_hidden()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .child(render_mode_controls(ws, cx))
        .child(render_batch_and_action_controls(ws, border, can_start, cx))
        .into_any_element()
}

fn can_start_transfer(ws: &TableMinusWorkspace, is_connected: bool, cx: &App) -> bool {
    let has_included = ws
        .data_transfer_ui
        .column_mappings
        .iter()
        .any(|m| m.include);
    let target_table_ok = !ws
        .data_transfer_ui
        .target_table_input
        .read(cx)
        .value()
        .trim()
        .is_empty();
    let src_ready = ws.data_transfer_ui.source_table.is_some()
        && (ws.data_transfer_ui.source_connection_id.is_some() || is_connected);
    let dst_ready = ws.data_transfer_ui.target_connection_id.is_some() || is_connected;
    !ws.data_transfer_ui.is_running && src_ready && dst_ready && has_included && target_table_ok
}

fn render_mode_controls(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let mode = ws.data_transfer_ui.mode;
    let disabled = ws.data_transfer_ui.is_running;

    div()
        .min_w_0()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(11.5))
                .text_color(colors.muted)
                .child(s.transfer_mode_label),
        )
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(mode_button(
                    "transfer-mode-create",
                    s.transfer_mode_create,
                    mode == TransferMode::CreateAndLoad,
                    TransferMode::CreateAndLoad,
                    disabled,
                    cx,
                ))
                .child(mode_button(
                    "transfer-mode-append",
                    s.transfer_mode_append,
                    mode == TransferMode::Append,
                    TransferMode::Append,
                    disabled,
                    cx,
                ))
                .child(mode_button(
                    "transfer-mode-truncate",
                    s.transfer_mode_truncate,
                    mode == TransferMode::TruncateAndLoad,
                    TransferMode::TruncateAndLoad,
                    disabled,
                    cx,
                )),
        )
        .into_any_element()
}

fn render_batch_and_action_controls(
    ws: &TableMinusWorkspace,
    border: herogpui::gpui::Hsla,
    can_start: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let disabled = ws.data_transfer_ui.is_running;

    div()
        .min_w_0()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(11.5))
                        .text_color(colors.muted)
                        .child(s.transfer_batch_size),
                )
                .child(
                    div().w(px(76.0)).flex_shrink_0().child(
                        Input::new(&ws.data_transfer_ui.batch_size_input)
                            .input_type(InputType::Number)
                            .variant(FieldVariant::Secondary)
                            .placeholder(DEFAULT_BATCH_SIZE)
                            .is_disabled(disabled)
                            .full_width()
                            .on_change(cx.listener(|_, _: &str, _, cx| cx.notify())),
                    ),
                ),
        )
        .child(
            div()
                .w(px(1.0))
                .h(px(18.0))
                .mx(px(2.0))
                .bg(border)
                .flex_shrink_0(),
        )
        .child(render_transfer_action_area(ws, can_start, cx))
        .into_any_element()
}

fn render_transfer_action_area(
    ws: &TableMinusWorkspace,
    can_start: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();

    if ws.data_transfer_ui.is_running {
        let indicator = if is_reduced_motion(cx) {
            Icon::new(IconName::LoaderCircle)
                .size(IconSize::Sm)
                .color(colors.accent.color)
                .into_any_element()
        } else {
            Spinner::new("transfer-running-spinner")
                .size(SpinnerSize::Sm)
                .color(HeroColor::Accent)
                .into_any_element()
        };
        div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(indicator)
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(colors.muted)
                    .child(s.running),
            )
            .child(
                div().flex_shrink_0().child(
                    Button::new("transfer-cancel-btn")
                        .variant(Variant::DangerSoft)
                        .size(HeroSize::Sm)
                        .on_press(cx.listener(|ws, _, _, cx| {
                            if let Some(token) = ws.data_transfer_ui.cancel_token.as_ref() {
                                token.cancel();
                            }
                            cx.notify();
                        }))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(5.0))
                                .child(Icon::new(IconName::Square).size(IconSize::Xs))
                                .child(s.cancel_label),
                        ),
                ),
            )
            .into_any_element()
    } else {
        div()
            .flex_shrink_0()
            .child(
                Button::new("transfer-start-btn")
                    .variant(Variant::Primary)
                    .size(HeroSize::Sm)
                    .is_disabled(!can_start)
                    .on_press(cx.listener(|ws, _, _, cx| {
                        start_data_transfer(ws, cx);
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.0))
                            .child(Icon::new(IconName::Play).size(IconSize::Xs))
                            .child(s.transfer_start_btn),
                    ),
            )
            .into_any_element()
    }
}

fn mode_button(
    id: &'static str,
    label: &'static str,
    selected: bool,
    mode: TransferMode,
    disabled: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    div()
        .flex_shrink_0()
        .child(
            Button::new(id)
                .variant(if selected {
                    Variant::Primary
                } else {
                    Variant::Secondary
                })
                .size(HeroSize::Sm)
                .is_disabled(disabled)
                .on_press(cx.listener(move |ws, _, _, cx| {
                    ws.data_transfer_ui.mode = mode;
                    cx.notify();
                }))
                .child(label),
        )
        .into_any_element()
}

fn render_body(
    ws: &TableMinusWorkspace,
    is_connected: bool,
    is_narrow_body: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let has_saved = !ws.controller.library.storage().connections.is_empty();
    if !is_connected && ws.data_transfer_ui.source_connection_id.is_none() && !has_saved {
        return render_centered_empty(s.no_active_connection, s.no_active_connection_desc, cx);
    }
    if ws.data_transfer_ui.loading_source_metadata && ws.data_transfer_ui.column_mappings.is_empty()
    {
        return render_loading_state(s.running, cx);
    }
    if ws.data_transfer_ui.column_mappings.is_empty() {
        return div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .when_some(ws.data_transfer_ui.last_error.as_ref(), |el, err| {
                el.child(
                    div().p(px(14.0)).child(
                        Alert::new(s.query_error_title)
                            .status(HeroColor::Danger)
                            .description(err.clone()),
                    ),
                )
            })
            .child(render_centered_empty(
                s.structure_no_table_title,
                s.structure_no_table_desc,
                cx,
            ))
            .into_any_element();
    }

    let body = div().flex_1().min_w_0().min_h_0().overflow_hidden().flex();

    if is_narrow_body {
        body.flex_col()
            .child(render_column_mapping_pane(ws, is_narrow_body, cx))
            .child(render_preview_and_report_pane(ws, is_narrow_body, cx))
            .into_any_element()
    } else {
        body.flex_row()
            .child(render_column_mapping_pane(ws, is_narrow_body, cx))
            .child(render_preview_and_report_pane(ws, is_narrow_body, cx))
            .into_any_element()
    }
}

fn render_column_mapping_pane(
    ws: &TableMinusWorkspace,
    is_narrow_body: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let border = hairline_border_color(cx);
    let value_font = value_gpui_font(&ws.controller.settings);

    let mut rows_scroll = div()
        .id("transfer-mapping-scroll")
        .flex_1()
        .min_w_0()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col();

    for (idx, col) in ws.data_transfer_ui.column_mappings.iter().enumerate() {
        rows_scroll = rows_scroll.child(render_mapping_row(
            idx,
            col,
            ws.data_transfer_ui.is_running,
            &value_font,
            border,
            s,
            cx,
        ));
    }

    let mut pane = div()
        .id("transfer-mapping-pane")
        .flex_1()
        .min_w_0()
        .min_h_0()
        .overflow_x_scroll()
        .border_color(border)
        .flex()
        .flex_col();

    pane = if is_narrow_body {
        pane.min_h(px(160.0)).border_b_1()
    } else {
        pane.border_r_1()
    };

    pane.child(render_column_mapping_header(ws, border, cx))
        .child(render_mapping_table_header(border, s, cx))
        .child(rows_scroll)
        .into_any_element()
}

fn render_column_mapping_header(
    ws: &TableMinusWorkspace,
    border: herogpui::gpui::Hsla,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let total = ws.data_transfer_ui.column_mappings.len();
    let included = ws
        .data_transfer_ui
        .column_mappings
        .iter()
        .filter(|m| m.include)
        .count();
    let all_included = total > 0 && included == total;

    div()
        .h(px(34.0))
        .px(px(12.0))
        .border_b_1()
        .border_color(border)
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .min_w_0()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(s.transfer_column_mapping),
                )
                .child(
                    div().flex_shrink_0().child(
                        Chip::new()
                            .variant(ChipVariant::Soft)
                            .color(HeroColor::Default)
                            .size(HeroSize::Sm)
                            .child(ChipLabel::new().child(format!("{included}/{total}"))),
                    ),
                ),
        )
        .child(
            div().flex_shrink_0().child(
                Button::new("transfer-toggle-all-cols")
                    .variant(Variant::Ghost)
                    .size(HeroSize::Sm)
                    .is_disabled(ws.data_transfer_ui.is_running)
                    .on_press(cx.listener(move |ws, _, _, cx| {
                        let target_state = !all_included;
                        for entry in &mut ws.data_transfer_ui.column_mappings {
                            entry.include = target_state;
                        }
                        cx.notify();
                    }))
                    .child(s.show_all),
            ),
        )
        .into_any_element()
}

fn render_mapping_table_header(
    border: herogpui::gpui::Hsla,
    s: &crate::i18n::Strings,
    cx: &App,
) -> AnyElement {
    let colors = cx.colors();
    div()
        .h(px(28.0))
        .px(px(12.0))
        .bg(muted_surface_bg(cx))
        .border_b_1()
        .border_color(border)
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(div().w(px(24.0)).flex_shrink_0())
        .child(
            div()
                .flex_1()
                .min_w(px(110.0))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.muted)
                .child(s.struct_col_column),
        )
        .child(
            div()
                .w(px(110.0))
                .flex_shrink_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.muted)
                .child(s.schema_column_type),
        )
        .child(div().w(px(18.0)).flex_shrink_0())
        .child(
            div()
                .flex_1()
                .min_w(px(120.0))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.muted)
                .child(s.compare_target_prefix),
        )
        .child(
            div()
                .w(px(135.0))
                .flex_shrink_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.muted)
                .child(s.schema_column_type),
        )
        .into_any_element()
}

fn render_mapping_row(
    idx: usize,
    col: &ColumnMappingDraft,
    disabled: bool,
    value_font: &herogpui::Font,
    border: herogpui::gpui::Hsla,
    s: &crate::i18n::Strings,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let mut row = div()
        .w_full()
        .min_h(px(34.0))
        .px(px(12.0))
        .py(px(4.0))
        .border_b_1()
        .border_color(border)
        .flex()
        .items_center()
        .gap(px(8.0));

    if let Some(zebra) = row_zebra_bg(idx, cx) {
        row = row.bg(zebra);
    }
    if !col.include {
        row = row.opacity(0.5);
    }

    row.child(render_mapping_checkbox(idx, col.include, disabled, cx))
        .child(render_mapping_source_name(
            col,
            colors.foreground,
            colors.warning.color,
        ))
        .child(
            div()
                .w(px(110.0))
                .flex_shrink_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .font(value_font.clone())
                .text_size(px(11.0))
                .text_color(colors.muted)
                .child(col.source_type.clone()),
        )
        .child(
            div()
                .w(px(18.0))
                .flex_shrink_0()
                .flex()
                .justify_center()
                .child(
                    Icon::new(IconName::ArrowRight)
                        .size(IconSize::Xs)
                        .color(colors.muted),
                ),
        )
        .child(
            div().flex_1().min_w(px(120.0)).overflow_hidden().child(
                Input::new(&col.target_col_input)
                    .variant(FieldVariant::Secondary)
                    .placeholder(s.schema_column_name)
                    .is_disabled(disabled)
                    .full_width()
                    .on_change(cx.listener(|_, _: &str, _, cx| cx.notify())),
            ),
        )
        .child(
            div().w(px(135.0)).flex_shrink_0().overflow_hidden().child(
                Input::new(&col.target_type_input)
                    .variant(FieldVariant::Secondary)
                    .placeholder(s.schema_column_type)
                    .is_disabled(disabled)
                    .full_width()
                    .on_change(cx.listener(|_, _: &str, _, cx| cx.notify())),
            ),
        )
        .into_any_element()
}

fn render_mapping_checkbox(
    idx: usize,
    include: bool,
    disabled: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    div()
        .w(px(24.0))
        .flex_shrink_0()
        .child(
            Checkbox::new(SharedString::from(format!("transfer-col-inc-{idx}")))
                .is_selected(include)
                .is_disabled(disabled)
                .on_change(cx.listener(move |ws, &on: &bool, _, cx| {
                    if let Some(entry) = ws.data_transfer_ui.column_mappings.get_mut(idx) {
                        entry.include = on;
                    }
                    cx.notify();
                })),
        )
        .into_any_element()
}

fn render_mapping_source_name(
    col: &ColumnMappingDraft,
    fg: herogpui::gpui::Hsla,
    pk_color: herogpui::gpui::Hsla,
) -> AnyElement {
    div()
        .flex_1()
        .min_w(px(110.0))
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(px(5.0))
        .when(col.is_primary_key, |el| {
            el.child(
                div()
                    .flex_shrink_0()
                    .child(Icon::new(IconName::Key).size(IconSize::Xs).color(pk_color)),
            )
        })
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(fg)
                .child(col.source_column.clone()),
        )
        .into_any_element()
}

fn render_preview_and_report_pane(
    ws: &TableMinusWorkspace,
    is_narrow_body: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let border = hairline_border_color(cx);
    let preview_sql = build_transfer_preview_sql(ws, cx);
    let copy_sql = preview_sql.clone();
    let editor_font = editor_gpui_font(&ws.controller.settings);
    let grammar = grammar_for_db_engine(ws.data_transfer_ui.target_engine);

    let pane = div()
        .min_w_0()
        .min_h_0()
        .overflow_hidden()
        .flex()
        .flex_col();
    let pane = if is_narrow_body {
        pane.w_full().flex_1().min_h(px(160.0))
    } else {
        pane.w(px(320.0))
            .min_w(px(220.0))
            .max_w(px(440.0))
            .flex_shrink(1.0)
    };

    pane.child(
        div()
            .h(px(34.0))
            .px(px(12.0))
            .border_b_1()
            .border_color(border)
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.foreground)
                    .child(s.schema_preview_sql),
            )
            .child(
                div().flex_shrink_0().child(
                    Button::new("transfer-copy-ddl")
                        .variant(Variant::Ghost)
                        .size(HeroSize::Sm)
                        .is_disabled(preview_sql.trim().is_empty())
                        .on_press(cx.listener(move |ws, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_sql.clone()));
                            let msg = ws.controller.strings.ddl_copied_toast;
                            push_success_toast(msg, None, cx);
                        }))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.0))
                                .child(Icon::new(IconName::Copy).size(IconSize::Xs))
                                .child(s.copy_label),
                        ),
                ),
            ),
    )
    .child(
        div()
            .id("transfer-ddl-preview-scroll")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .overflow_x_hidden()
            .p(px(12.0))
            .bg(ws.content_palette.editor_background)
            .font(editor_font)
            .text_size(px(ws.controller.settings.editor_font.size.max(11.0)))
            .line_height(px(18.0))
            .text_color(ws.content_palette.editor_text)
            .child(syntax_bridge::styled_sql_block(
                preview_sql,
                grammar,
                &ws.content_palette,
            )),
    )
    .children(render_report_footer(ws, border, cx))
    .into_any_element()
}

fn render_report_footer(
    ws: &TableMinusWorkspace,
    border: herogpui::gpui::Hsla,
    cx: &App,
) -> Option<AnyElement> {
    let s = &ws.controller.strings;
    let has_err = ws.data_transfer_ui.last_error.is_some();
    let has_rep = ws.data_transfer_ui.last_report.is_some();
    if !ws.data_transfer_ui.is_running && !has_err && !has_rep {
        return None;
    }

    let mut panel = div()
        .border_t_1()
        .border_color(border)
        .p(px(12.0))
        .flex()
        .flex_col()
        .gap(px(8.0));

    if ws.data_transfer_ui.is_running {
        panel = panel.child(
            div()
                .px(px(10.0))
                .py(px(8.0))
                .rounded(px(CONTROL_RADIUS_PX))
                .border_1()
                .border_color(border)
                .bg(muted_surface_bg(cx))
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    Icon::new(IconName::LoaderCircle)
                        .size(IconSize::Sm)
                        .color(cx.colors().accent.color),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(cx.colors().foreground)
                        .child(s.running),
                ),
        );
    }

    if let Some(err) = ws.data_transfer_ui.last_error.as_ref() {
        panel = panel.child(
            Alert::new(s.query_error_title)
                .status(HeroColor::Danger)
                .description(err.clone()),
        );
    }

    if let Some(report) = ws.data_transfer_ui.last_report.as_ref() {
        panel = panel.child(render_report_metrics(report, border, s, cx));
        if !report.warnings.is_empty() {
            panel = panel.child(
                Alert::new(s.messages)
                    .status(HeroColor::Warning)
                    .description(report.warnings.join("\n")),
            );
        }
    }

    Some(panel.into_any_element())
}

fn render_report_metrics(
    report: &TransferReport,
    border: herogpui::gpui::Hsla,
    s: &crate::i18n::Strings,
    cx: &App,
) -> AnyElement {
    let rows_val = if report.rows_read == report.rows_written {
        format!("{} {}", report.rows_written, s.rows_suffix)
    } else {
        format!("{} / {}", report.rows_written, report.rows_read)
    };

    div()
        .w_full()
        .rounded(px(PANEL_RADIUS_PX))
        .border_1()
        .border_color(border)
        .bg(muted_surface_bg(cx))
        .px(px(10.0))
        .py(px(8.0))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .child(metric_cell(s.transfer_rows_written, rows_val, cx))
        .child(metric_cell(
            s.statements_unit,
            report.batches.to_string(),
            cx,
        ))
        .child(metric_cell(
            s.summary_elapsed,
            format!("{}ms", report.elapsed_ms),
            cx,
        ))
        .into_any_element()
}

fn metric_cell(label: &'static str, value: String, cx: &App) -> AnyElement {
    let colors = cx.colors();
    div()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .text_size(px(10.5))
                .text_color(colors.muted)
                .truncate()
                .child(label),
        )
        .child(
            div()
                .text_size(px(12.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.foreground)
                .truncate()
                .child(value),
        )
        .into_any_element()
}

fn render_centered_empty(title: &str, subtitle: &str, cx: &App) -> AnyElement {
    let colors = cx.colors();
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .p(px(24.0))
        .child(
            Icon::new(IconName::Database)
                .size(IconSize::Lg)
                .color(colors.muted),
        )
        .child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.foreground)
                .child(title.to_owned()),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(colors.muted)
                .child(subtitle.to_owned()),
        )
        .into_any_element()
}

fn render_loading_state(label: &'static str, cx: &App) -> AnyElement {
    let colors = cx.colors();
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .child(
            Icon::new(IconName::LoaderCircle)
                .size(IconSize::Sm)
                .color(colors.accent.color),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(colors.muted)
                .child(label),
        )
        .into_any_element()
}

fn build_connection_picker_items(ws: &TableMinusWorkspace, is_connected: bool) -> Vec<PickerItem> {
    let s = &ws.controller.strings;
    let mut items = Vec::new();
    if is_connected {
        let host = ws.controller.toolbar_status.host.trim();
        let db = ws.controller.toolbar_status.database.trim();
        let active_label = match (host.is_empty() || host == "—", db.is_empty() || db == "—") {
            (false, false) => format!("{} ({host} / {db})", s.active_connection_fallback),
            (false, true) => format!("{} ({host})", s.active_connection_fallback),
            (true, false) => format!("{} ({db})", s.active_connection_fallback),
            (true, true) => s.active_connection_fallback.to_owned(),
        };
        items.push(PickerItem::new(ACTIVE_CONNECTION_KEY, active_label));
    }
    for conn in ws.controller.library.storage().connections.values() {
        let eng = engine_from_driver_id(&conn.driver_id).display_name();
        let detail = if conn.database.trim().is_empty() {
            eng.to_owned()
        } else {
            format!("{eng} · {}", conn.database)
        };
        items.push(PickerItem::new(
            conn.id.0.clone(),
            format!("{} ({detail})", conn.name),
        ));
    }
    items
}

fn active_source_tables(ws: &TableMinusWorkspace) -> Vec<TableRef> {
    if ws.data_transfer_ui.source_connection_id.is_none() {
        collect_transferable_tables(&ws.controller.cached_objects)
    } else {
        ws.data_transfer_ui.source_tables.clone()
    }
}

fn collect_transferable_tables(objects: &[ObjectInfo]) -> Vec<TableRef> {
    objects
        .iter()
        .filter(|obj| {
            matches!(
                obj.kind,
                ObjectKind::Table | ObjectKind::PartitionedTable | ObjectKind::ForeignTable
            )
        })
        .map(|obj| TableRef::new(obj.schema.clone(), obj.name.clone()))
        .collect()
}

fn apply_source_connection_change(
    ws: &mut TableMinusWorkspace,
    key: Option<&str>,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let next = key
        .filter(|k| !k.is_empty() && *k != ACTIVE_CONNECTION_KEY)
        .map(str::to_owned);
    if ws.data_transfer_ui.source_connection_id == next {
        return;
    }
    ws.data_transfer_ui.source_connection_id = next.clone();
    ws.data_transfer_ui.source_tables.clear();
    ws.data_transfer_ui.source_tables_loaded_for = None;
    ws.data_transfer_ui.source_table = None;
    ws.data_transfer_ui.source_def = None;
    ws.data_transfer_ui.loaded_source_key = None;
    ws.data_transfer_ui.column_mappings.clear();
    ws.data_transfer_ui.last_error = None;
    ws.data_transfer_ui.last_report = None;
    if let Some(conn_id) = next
        && let Some(conn) = find_library_connection(ws, &conn_id)
    {
        ws.data_transfer_ui.source_engine = DatabaseEngine::from_driver_id(&conn.driver_id);
    } else {
        ws.data_transfer_ui.source_engine = ws.controller.active_database_engine();
    }
    cx.notify();
}

fn apply_target_connection_change(
    ws: &mut TableMinusWorkspace,
    key: Option<&str>,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let next = key
        .filter(|k| !k.is_empty() && *k != ACTIVE_CONNECTION_KEY)
        .map(str::to_owned);
    ws.data_transfer_ui.target_connection_id = next.clone();
    let engine = if let Some(conn_id) = next {
        find_library_connection(ws, &conn_id).map_or_else(
            || ws.controller.active_database_engine(),
            |conn| DatabaseEngine::from_driver_id(&conn.driver_id),
        )
    } else {
        ws.controller.active_database_engine()
    };
    apply_target_engine_change(ws, engine, cx);
}

fn apply_target_engine_change(
    ws: &mut TableMinusWorkspace,
    target_engine: DatabaseEngine,
    cx: &mut Context<TableMinusWorkspace>,
) {
    ws.data_transfer_ui.target_engine = target_engine;
    let src_engine = ws.data_transfer_ui.source_engine;
    for col in &mut ws.data_transfer_ui.column_mappings {
        let mapped = map_sql_type(src_engine, target_engine, &col.source_type);
        col.target_type_input.update(cx, |inp, cx| {
            inp.set_value(mapped);
            cx.notify();
        });
    }
    cx.notify();
}

fn force_reload_source_metadata(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) {
    ws.data_transfer_ui.loaded_source_key = None;
    ws.data_transfer_ui.source_tables_loaded_for = None;
    ws.data_transfer_ui.last_error = None;
    if ws.data_transfer_ui.source_connection_id.is_none() {
        let _ = ws.controller.refresh_schema_sidebar_blocking();
    }
    cx.notify();
}

fn start_load_saved_source_tables(
    ws: &mut TableMinusWorkspace,
    conn_id: String,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let Some(services) = ws.controller.services.clone() else {
        return;
    };
    let Some(lib_conn) = find_library_connection(ws, &conn_id).cloned() else {
        return;
    };
    let safe_mode = ws.controller.active_safe_mode();
    ws.data_transfer_ui.loading_source_metadata = true;
    ws.data_transfer_ui.last_error = None;

    let (tx, rx) = async_channel::bounded::<Result<(DatabaseEngine, Vec<TableRef>), String>>(1);
    let runtime = services.runtime.clone();
    drop(runtime.spawn(async move {
        let outcome = load_saved_connection_tables(&services, &lib_conn, safe_mode).await;
        let _ = tx.send(outcome).await;
    }));

    let expected_conn = conn_id.clone();
    ws.data_transfer_ui.metadata_task = Some(cx.spawn(
        async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
            if let Ok(result) = rx.recv().await {
                let _ = this.update(cx, |ws, cx| {
                    if ws.data_transfer_ui.source_connection_id.as_deref()
                        != Some(expected_conn.as_str())
                    {
                        return;
                    }
                    ws.data_transfer_ui.loading_source_metadata = false;
                    ws.data_transfer_ui.source_tables_loaded_for = Some(expected_conn);
                    match result {
                        Ok((engine, tables)) => {
                            ws.data_transfer_ui.source_engine = engine;
                            ws.data_transfer_ui.source_tables = tables;
                            ws.data_transfer_ui.source_table =
                                ws.data_transfer_ui.source_tables.first().cloned();
                        }
                        Err(err) => {
                            ws.data_transfer_ui.last_error = Some(err);
                        }
                    }
                    cx.notify();
                });
            }
        },
    ));
}

fn start_load_source_definition(
    ws: &mut TableMinusWorkspace,
    conn_id: Option<String>,
    table: TableRef,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let Some(services) = ws.controller.services.clone() else {
        return;
    };
    let active_ws = ws.controller.active_workspace;
    let safe_mode = ws.controller.active_safe_mode();
    let lib_conn = conn_id
        .as_deref()
        .and_then(|id| find_library_connection(ws, id).cloned());
    if conn_id.is_some() && lib_conn.is_none() {
        return;
    }

    let key = format!(
        "{}::{}",
        conn_id.as_deref().unwrap_or(ACTIVE_CONNECTION_KEY),
        format_table_ref(&table)
    );
    ws.data_transfer_ui.loading_source_metadata = true;
    ws.data_transfer_ui.last_error = None;

    let (tx, rx) = async_channel::bounded::<Result<(DatabaseEngine, TableDefinition), String>>(1);
    let table_owned = table.clone();
    let runtime = services.runtime.clone();
    drop(runtime.spawn(async move {
        let outcome = match lib_conn {
            Some(conn) => {
                fetch_saved_table_definition(&services, &conn, &table_owned, safe_mode).await
            }
            None => fetch_active_table_definition(&services, active_ws, &table_owned).await,
        };
        let _ = tx.send(outcome).await;
    }));

    ws.data_transfer_ui.metadata_task = Some(cx.spawn(
        async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
            if let Ok(result) = rx.recv().await {
                let _ = this.update(cx, |ws, cx| {
                    ws.data_transfer_ui.loading_source_metadata = false;
                    ws.data_transfer_ui.loaded_source_key = Some(key);
                    match result {
                        Ok((src_engine, def)) => {
                            apply_loaded_source_definition(ws, src_engine, def, cx);
                        }
                        Err(err) => {
                            ws.data_transfer_ui.last_error = Some(err);
                            ws.data_transfer_ui.column_mappings.clear();
                        }
                    }
                    cx.notify();
                });
            }
        },
    ));
}

fn apply_loaded_source_definition(
    ws: &mut TableMinusWorkspace,
    src_engine: DatabaseEngine,
    def: TableDefinition,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let target_engine = ws.data_transfer_ui.target_engine;
    let pk_names: std::collections::HashSet<&str> = def
        .primary_key
        .as_ref()
        .map(|pk| pk.columns.iter().map(String::as_str).collect())
        .unwrap_or_default();

    let drafts = def
        .columns
        .iter()
        .map(|col| {
            let raw_type = column_type_text(col);
            let mapped = map_sql_type(src_engine, target_engine, &raw_type);
            let name = col.info.name.clone();
            let is_pk = col.info.primary_key || pk_names.contains(name.as_str());
            ColumnMappingDraft {
                source_column: name.clone(),
                source_type: raw_type,
                nullable: col.info.nullable,
                is_primary_key: is_pk,
                auto_increment: col.info.is_auto_increment,
                default_value: col.info.default.clone(),
                include: true,
                target_col_input: cx.new(|cx| InputState::with_value(cx, name)),
                target_type_input: cx.new(|cx| InputState::with_value(cx, mapped)),
            }
        })
        .collect();

    let same_connection =
        ws.data_transfer_ui.source_connection_id == ws.data_transfer_ui.target_connection_id;
    let default_target_table = if same_connection {
        format!("{}_copy", def.table.name)
    } else {
        def.table.name.clone()
    };
    ws.data_transfer_ui
        .target_table_input
        .update(cx, |inp, cx| {
            inp.set_value(default_target_table);
            cx.notify();
        });

    ws.data_transfer_ui.source_engine = src_engine;
    ws.data_transfer_ui.source_def = Some(def);
    ws.data_transfer_ui.column_mappings = drafts;
}

fn start_data_transfer(ws: &mut TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) {
    let Some(services) = ws.controller.services.clone() else {
        return;
    };
    let Some(config) = build_transfer_config(ws, cx) else {
        return;
    };

    let active_ws = ws.controller.active_workspace;
    let safe_mode = ws.controller.active_safe_mode();
    let src_conn = ws
        .data_transfer_ui
        .source_connection_id
        .as_deref()
        .and_then(|id| find_library_connection(ws, id).cloned());
    let dst_conn = ws
        .data_transfer_ui
        .target_connection_id
        .as_deref()
        .and_then(|id| find_library_connection(ws, id).cloned());
    if (ws.data_transfer_ui.source_connection_id.is_some() && src_conn.is_none())
        || (ws.data_transfer_ui.target_connection_id.is_some() && dst_conn.is_none())
    {
        return;
    }

    let cancel = CancellationToken::new();
    let cancel_bg = cancel.clone();
    ws.data_transfer_ui.is_running = true;
    ws.data_transfer_ui.cancel_token = Some(cancel);
    ws.data_transfer_ui.last_error = None;
    ws.data_transfer_ui.last_report = None;
    cx.notify();

    let (tx, rx) = async_channel::bounded::<Result<TransferReport, String>>(1);
    let runtime = services.runtime.clone();
    drop(runtime.spawn(async move {
        let outcome = execute_transfer_async(
            &services, active_ws, src_conn, dst_conn, &config, safe_mode, &cancel_bg,
        )
        .await;
        let _ = tx.send(outcome).await;
    }));

    ws.data_transfer_ui.transfer_task = Some(cx.spawn(
        async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
            if let Ok(result) = rx.recv().await {
                let _ = this.update(cx, |ws, cx| finish_data_transfer(ws, result, cx));
            }
        },
    ));
}

fn build_transfer_config(ws: &TableMinusWorkspace, cx: &App) -> Option<TransferConfig> {
    let src_table = ws.data_transfer_ui.source_table.clone()?;
    let target_table = ws
        .data_transfer_ui
        .target_table_input
        .read(cx)
        .value()
        .trim()
        .to_owned();
    if target_table.is_empty() {
        return None;
    }

    let target_schema_raw = ws
        .data_transfer_ui
        .target_schema_input
        .read(cx)
        .value()
        .trim()
        .to_owned();
    let batch_size = ws
        .data_transfer_ui
        .batch_size_input
        .read(cx)
        .value()
        .trim()
        .parse::<usize>()
        .ok()
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_BATCH_SIZE_NUM);

    let column_mappings = ws
        .data_transfer_ui
        .column_mappings
        .iter()
        .map(|col| ColumnTypeMapping {
            source_column: col.source_column.clone(),
            target_column: col.target_col_input.read(cx).value().trim().to_owned(),
            target_type: col.target_type_input.read(cx).value().trim().to_owned(),
            include: col.include,
        })
        .collect();

    Some(TransferConfig {
        source_schema: src_table.schema,
        source_table: src_table.name,
        target_schema: (!target_schema_raw.is_empty()).then_some(target_schema_raw),
        target_table,
        target_engine: ws.data_transfer_ui.target_engine,
        mode: ws.data_transfer_ui.mode,
        batch_size,
        column_mappings,
    })
}

fn finish_data_transfer(
    ws: &mut TableMinusWorkspace,
    result: Result<TransferReport, String>,
    cx: &mut Context<TableMinusWorkspace>,
) {
    ws.data_transfer_ui.is_running = false;
    ws.data_transfer_ui.cancel_token = None;
    match result {
        Ok(report) => {
            let title = ws.controller.strings.transfer_rows_written;
            let detail = format!(
                "{} {} ({}ms)",
                report.rows_written, ws.controller.strings.rows_suffix, report.elapsed_ms
            );
            ws.data_transfer_ui.last_report = Some(report);
            push_success_toast(title, Some(detail.into()), cx);
        }
        Err(err) => {
            let title = ws.controller.strings.failed;
            ws.data_transfer_ui.last_error = Some(err.clone());
            push_error_toast(title, Some(err.into()), cx);
        }
    }
    cx.notify();
}

async fn execute_transfer_async(
    services: &Arc<ServiceHub>,
    active_ws: tableminus_state::connection_window::WorkspaceId,
    src_conn: Option<LibraryConnection>,
    dst_conn: Option<LibraryConnection>,
    config: &TransferConfig,
    safe_mode: SafeModeLevel,
    cancel: &CancellationToken,
) -> Result<TransferReport, String> {
    let (src_session, src_ephemeral) = match src_conn {
        Some(conn) => (
            open_library_session(services, &conn, safe_mode).await?,
            true,
        ),
        None => (active_session_or_err(services, active_ws).await?, false),
    };
    let (dst_session, dst_ephemeral) = match dst_conn {
        Some(conn) => match open_library_session(services, &conn, safe_mode).await {
            Ok(session) => (session, true),
            Err(err) => {
                if src_ephemeral {
                    drop(src_session.begin_shutdown());
                }
                return Err(err);
            }
        },
        None => match active_session_or_err(services, active_ws).await {
            Ok(session) => (session, false),
            Err(err) => {
                if src_ephemeral {
                    drop(src_session.begin_shutdown());
                }
                return Err(err);
            }
        },
    };

    let src_engine = DatabaseEngine::from_grammar(src_session.grammar());
    let dst_engine = DatabaseEngine::from_grammar(dst_session.grammar());
    let result = transfer_table_cancellable(
        src_session.as_ref(),
        dst_session.as_ref(),
        src_engine,
        dst_engine,
        config,
        cancel,
    )
    .await
    .map_err(|err| err.to_string());

    if src_ephemeral {
        drop(src_session.begin_shutdown());
    }
    if dst_ephemeral {
        drop(dst_session.begin_shutdown());
    }
    result
}

async fn active_session_or_err(
    services: &ServiceHub,
    active_ws: tableminus_state::connection_window::WorkspaceId,
) -> Result<Arc<Session>, String> {
    let guard = services.sessions.read().await;
    guard
        .session(active_ws)
        .ok_or_else(|| "no active session".to_owned())
}

async fn load_saved_connection_tables(
    services: &ServiceHub,
    conn: &LibraryConnection,
    safe_mode: SafeModeLevel,
) -> Result<(DatabaseEngine, Vec<TableRef>), String> {
    let session = open_library_session(services, conn, safe_mode).await?;
    let engine = DatabaseEngine::from_grammar(session.grammar());
    let objects_res = session.refresh_objects(ObjectScope::AllSchemas).await;
    drop(session.begin_shutdown());
    let objects = objects_res.map_err(|err| err.to_string())?;
    Ok((engine, collect_transferable_tables(&objects)))
}

async fn fetch_active_table_definition(
    services: &ServiceHub,
    active_ws: tableminus_state::connection_window::WorkspaceId,
    table: &TableRef,
) -> Result<(DatabaseEngine, TableDefinition), String> {
    let session = active_session_or_err(services, active_ws).await?;
    let engine = DatabaseEngine::from_grammar(session.grammar());
    let def = fetch_table_definition_from_session(&session, table).await?;
    Ok((engine, def))
}

async fn fetch_saved_table_definition(
    services: &ServiceHub,
    conn: &LibraryConnection,
    table: &TableRef,
    safe_mode: SafeModeLevel,
) -> Result<(DatabaseEngine, TableDefinition), String> {
    let session = open_library_session(services, conn, safe_mode).await?;
    let engine = DatabaseEngine::from_grammar(session.grammar());
    let def_res = fetch_table_definition_from_session(&session, table).await;
    drop(session.begin_shutdown());
    Ok((engine, def_res?))
}

async fn fetch_table_definition_from_session(
    session: &Session,
    table: &TableRef,
) -> Result<TableDefinition, String> {
    let target = table.clone();
    if let Ok(def) = session
        .run(OperationKind::Metadata, |lease, call| {
            let t = target.clone();
            async move {
                lease
                    .fetch_table_definition(&t, call)
                    .await
                    .map_err(OperationError::from)
            }
        })
        .await
    {
        return Ok(def);
    }

    let cols = session
        .columns(table.clone())
        .await
        .map_err(|err| err.to_string())?;
    let columns = cols
        .iter()
        .cloned()
        .map(|info| ColumnDefinition::new(info, ColumnAttributes::default()))
        .collect();
    Ok(TableDefinition {
        table: table.clone(),
        columns,
        primary_key: None,
        indexes: Vec::new(),
        foreign_keys: Vec::new(),
        inbound_foreign_keys: Vec::new(),
        source: None,
        options: None,
    })
}

async fn open_library_session(
    services: &ServiceHub,
    conn: &LibraryConnection,
    safe_mode: SafeModeLevel,
) -> Result<Arc<Session>, String> {
    let saved = Uuid::parse_str(&conn.id.0).ok().and_then(|uuid| {
        services
            .connection_store
            .load_blocking()
            .ok()
            .and_then(|list| list.iter().find(|c| c.id == uuid).cloned())
    });

    let engine = engine_from_driver_id(&conn.driver_id);
    let port = conn.port.unwrap_or_else(|| default_port_for_engine(engine));
    let mut draft = ConnectionFormDraft {
        name: conn.name.clone(),
        engine,
        host: conn.host.clone(),
        port,
        database: conn.database.clone(),
        username: conn.username.clone(),
        ..ConnectionFormDraft::default()
    };

    let mut db_password: Option<SecretString> = None;
    if let Some(ref sc) = saved {
        draft.auth_mode = match sc.auth_mode {
            CoreAuthMode::Kerberos => AuthMode::SspiIntegrated,
            _ => AuthMode::Password,
        };
        draft.tls_mode = match sc.tls.mode {
            CoreTlsMode::Disable => TlsMode::Disable,
            CoreTlsMode::Prefer => TlsMode::Prefer,
            CoreTlsMode::Require => TlsMode::Require,
            CoreTlsMode::VerifyFull | CoreTlsMode::Strict => TlsMode::VerifyFull,
        };
        if let Some(ref ssh) = sc.ssh {
            draft.ssh_enabled = true;
            draft.ssh_host.clone_from(&ssh.host);
            draft.ssh_port = ssh.port.unwrap_or(22);
            draft.ssh_user = ssh.username.clone().unwrap_or_default();
            draft.ssh_key_path = match &ssh.auth {
                SavedSshAuth::PrivateKey { path: Some(p), .. } => p.to_string_lossy().into_owned(),
                _ => String::new(),
            };
        }
        if let Ok(SecretLookup::Found(secret)) = services
            .credentials
            .load(sc.id, SecretKind::DatabasePassword)
            .await
        {
            db_password = Some(secret);
        }
    }

    let mut request: ConnectRequest =
        build_connect_request(&services.runtime, &draft, db_password, safe_mode)
            .map_err(|err| err.to_string())?;
    if let Some(ref sc) = saved
        && let RequestTarget::Network { tls, options, .. } = &mut request.target
    {
        *tls = sc.tls.clone();
        *options = sc.options.clone();
    }

    let creds: Arc<dyn CredentialSource> = match saved {
        Some(ref sc) => Arc::new(KeyringCredentials::for_saved(
            services.credentials.clone(),
            sc,
        )),
        None => Arc::new(NoStoredCredentials),
    };
    let connector = services.runtime.connector();
    let pending = connector
        .establish(
            request,
            CredentialInteraction::Unattended,
            CancellationToken::new(),
        )
        .await
        .map_err(|err| err.to_string())?;

    let deps = SessionDeps {
        connector,
        credentials: creds,
        prompter: None,
        deadlines: services.runtime.deadlines().clone(),
        tasks: services.runtime.tasks().clone(),
    };
    Ok(Session::start(pending, deps))
}

fn build_transfer_preview_sql(ws: &TableMinusWorkspace, cx: &App) -> String {
    let target_engine = ws.data_transfer_ui.target_engine;
    let target_schema_raw = ws
        .data_transfer_ui
        .target_schema_input
        .read(cx)
        .value()
        .trim()
        .to_owned();
    let target_schema = (!target_schema_raw.is_empty()).then_some(target_schema_raw);
    let target_table_raw = ws
        .data_transfer_ui
        .target_table_input
        .read(cx)
        .value()
        .trim()
        .to_owned();
    let target_table = if target_table_raw.is_empty() {
        let Some(src) = ws.data_transfer_ui.source_table.as_ref() else {
            return String::new();
        };
        src.name.clone()
    } else {
        target_table_raw
    };

    let included: Vec<&ColumnMappingDraft> = ws
        .data_transfer_ui
        .column_mappings
        .iter()
        .filter(|col| col.include)
        .collect();
    if included.is_empty() {
        return String::new();
    }

    let mut statements = Vec::new();
    match ws.data_transfer_ui.mode {
        TransferMode::CreateAndLoad => {
            let columns = build_preview_columns(
                &included,
                ws.data_transfer_ui.source_engine,
                target_engine,
                cx,
            );
            let spec = CreateTableSpec {
                table_name: target_table.clone(),
                columns,
                foreign_keys: Vec::new(),
                indexes: Vec::new(),
                if_not_exists: true,
            };
            statements.push(build_create_table_sql(
                target_engine,
                target_schema.as_deref(),
                &spec,
            ));
        }
        TransferMode::TruncateAndLoad => {
            statements.extend(build_alter_table_sql(
                target_engine,
                target_schema.as_deref(),
                &target_table,
                &[AlterTableOp::TruncateTable],
            ));
        }
        TransferMode::Append => {}
    }

    let qualified = target_engine.qualify_table(target_schema.as_deref(), &target_table);
    let col_list = included
        .iter()
        .map(|col| {
            let raw = col.target_col_input.read(cx).value().trim().to_owned();
            let name = if raw.is_empty() {
                col.source_column.as_str()
            } else {
                raw.as_str()
            };
            target_engine.quote_identifier(name)
        })
        .collect::<Vec<_>>()
        .join(", ");
    statements.push(format!(
        "INSERT INTO {qualified} ({col_list}) VALUES (...);"
    ));
    statements.join("\n\n")
}

fn build_preview_columns(
    included: &[&ColumnMappingDraft],
    source_engine: DatabaseEngine,
    target_engine: DatabaseEngine,
    cx: &App,
) -> Vec<ColumnSpec> {
    included
        .iter()
        .map(|col| {
            let name_raw = col.target_col_input.read(cx).value().trim().to_owned();
            let name = if name_raw.is_empty() {
                col.source_column.clone()
            } else {
                name_raw
            };
            let type_raw = col.target_type_input.read(cx).value().trim().to_owned();
            let data_type = if type_raw.is_empty() {
                map_sql_type(source_engine, target_engine, &col.source_type)
            } else {
                type_raw
            };
            let nullable = if target_engine == DatabaseEngine::Clickhouse && col.is_primary_key {
                false
            } else {
                col.nullable
            };
            ColumnSpec {
                name,
                data_type,
                nullable,
                default_expr: preview_default_expr(&col.default_value, target_engine),
                is_primary_key: col.is_primary_key,
                is_unique: false,
                auto_increment: col.auto_increment && target_engine != DatabaseEngine::Duckdb,
            }
        })
        .collect()
}

fn preview_default_expr(default: &ColumnDefault, engine: DatabaseEngine) -> Option<String> {
    match default {
        ColumnDefault::None => None,
        ColumnDefault::Literal(val) => Some(format_sql_literal(engine, val)),
        ColumnDefault::Expression(expr) => {
            let trimmed = expr.as_sql().trim();
            let upper = trimmed.to_ascii_uppercase();
            let portable = matches!(
                upper.as_str(),
                "CURRENT_TIMESTAMP" | "CURRENT_DATE" | "CURRENT_TIME" | "NULL" | "TRUE" | "FALSE"
            ) || (trimmed.starts_with('\'')
                && trimmed.ends_with('\'')
                && trimmed.len() >= 2)
                || trimmed.parse::<f64>().is_ok();
            portable.then(|| trimmed.to_owned())
        }
    }
}

fn find_library_connection<'a>(
    ws: &'a TableMinusWorkspace,
    id: &str,
) -> Option<&'a LibraryConnection> {
    ws.controller
        .library
        .storage()
        .connections
        .values()
        .find(|c| c.id.0 == id)
}

fn column_type_text(col: &ColumnDefinition) -> String {
    col.info.column_type.catalog.as_ref().map_or_else(
        || col.info.column_type.name.clone(),
        |c| c.declaration.as_sql().to_owned(),
    )
}

fn format_table_ref(t: &TableRef) -> String {
    match t.schema.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(schema) => format!("{schema}.{}", t.name),
        None => t.name.clone(),
    }
}

const fn database_engine_to_kind(engine: DatabaseEngine) -> EngineKind {
    match engine {
        DatabaseEngine::Sqlite => EngineKind::Sqlite,
        DatabaseEngine::Postgres | DatabaseEngine::Generic => EngineKind::Postgresql,
        DatabaseEngine::Mysql => EngineKind::Mysql,
        DatabaseEngine::Mssql => EngineKind::Mssql,
        DatabaseEngine::Duckdb => EngineKind::Duckdb,
        DatabaseEngine::Clickhouse => EngineKind::Clickhouse,
    }
}

const fn grammar_for_db_engine(engine: DatabaseEngine) -> SqlGrammar {
    match engine {
        DatabaseEngine::Sqlite => SqlGrammar::Sqlite,
        DatabaseEngine::Postgres | DatabaseEngine::Generic => SqlGrammar::PostgreSql,
        DatabaseEngine::Mysql => SqlGrammar::MySql,
        DatabaseEngine::Mssql => SqlGrammar::MsSql,
        DatabaseEngine::Duckdb => SqlGrammar::DuckDb,
        DatabaseEngine::Clickhouse => SqlGrammar::ClickHouse,
    }
}
