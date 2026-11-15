//! Global metadata search modal for schemas, tables, views, routines, and columns.

use dataforge_state::connection_window::{ConnectionWindowPhase, WorkspaceId};
use dataforge_state::tabs::TabConnectionId;
use herogpui::components::{
    Alert, Button, Chip, ChipLabel, ChipVariant, Icon, IconName, IconSize, InputState, Modal,
    ModalSize, SearchField, Separator,
};
use herogpui::core::{Backdrop, Color as HeroColor, Size as HeroSize, Variant, with_alpha};
use herogpui::gpui::prelude::FluentBuilder as _;
use herogpui::gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, FontWeight,
    InteractiveElement, IntoElement, ParentElement, SharedString, StatefulInteractiveElement,
    Styled, Window, div, px,
};
use herogpui::theme::ActiveTheme;
use tablepro_core::ddl::TableDefinition;
use tablepro_core::meta::{ObjectInfo, ObjectKind, TableRef};

use super::shell_state::{ToastSeverity, WorkspaceSubView};
use super::theme::{COMPACT_GAP_PX, CONTROL_RADIUS_PX, MICRO_GAP_PX, hairline_border_color};
use super::{DataForgeWorkspace, TableViewMode};
use crate::app::ScreenMode;

const MAX_DISPLAYED_RESULTS: usize = 120;

/// Filter category pills in the global Metadata Search modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MetadataFilterKind {
    #[default]
    All,
    Tables,
    Views,
    Columns,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SearchResultKind {
    Table,
    View,
    Routine,
    Other,
    Column,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchResultEntry {
    kind: SearchResultKind,
    title: String,
    subtitle: String,
    detail: String,
    badge: Option<String>,
    target_table: Option<TableRef>,
    qualified_name: String,
}

/// Persistent UI state for the global Metadata Search modal.
#[derive(Debug)]
pub struct MetadataSearchState {
    pub open: bool,
    pub query: String,
    filter: MetadataFilterKind,
    query_input: Entity<InputState>,
    cached_defs: Vec<TableDefinition>,
    loaded_for: Option<(WorkspaceId, usize)>,
    last_error: Option<String>,
}

impl MetadataSearchState {
    /// Creates initial Metadata Search modal state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, cx: &mut Context<DataForgeWorkspace>) -> Self {
        Self {
            open: false,
            query: String::new(),
            filter: MetadataFilterKind::All,
            query_input: cx.new(|cx| InputState::new(cx)),
            cached_defs: Vec::new(),
            loaded_for: None,
            last_error: None,
        }
    }

    /// Opens the Metadata Search modal and focuses the search input.
    pub fn open_modal(&mut self, window: &mut Window, cx: &mut Context<DataForgeWorkspace>) {
        self.open = true;
        self.last_error = None;
        let handle = self.query_input.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        cx.notify();
    }

    /// Closes the Metadata Search modal.
    pub fn close_modal(&mut self) {
        self.open = false;
    }
}

/// Renders the opaque Metadata Search modal overlay above the workspace.
#[must_use]
pub fn render_metadata_search_modal(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let is_connected = matches!(
        ws.controller
            .connections
            .phase(ws.controller.active_workspace),
        Some(ConnectionWindowPhase::Connected)
    );
    ensure_columns_cached_once(ws, is_connected);

    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let opaque_card_bg = with_alpha(colors.overlay.background, 1.0);

    let matches = collect_search_results(
        &ws.metadata_search_ui.query,
        ws.metadata_search_ui.filter,
        &ws.controller.cached_objects,
        &ws.metadata_search_ui.cached_defs,
    );

    let body = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(COMPACT_GAP_PX + 2.0))
        .bg(opaque_card_bg)
        .child(render_search_controls(ws, is_connected, cx))
        .child(Separator::new())
        .child(render_results_container(ws, is_connected, &matches, cx));

    Modal::new()
        .id("modal-metadata-search")
        .is_open(ws.metadata_search_ui.open)
        .size(ModalSize::Lg)
        .backdrop(Backdrop::Opaque)
        .is_dismissible(true)
        .title(s.metadata_search)
        .icon(IconName::Search)
        .icon_color(HeroColor::Accent)
        .on_open_change(cx.listener(|ws, &open: &bool, _w, cx| {
            ws.metadata_search_ui.open = open;
            cx.notify();
        }))
        .child(body)
        .footer_child(render_footer_summary(ws, matches.len(), cx))
        .footer_child(
            Button::new("metadata-search-close")
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    ws.metadata_search_ui.close_modal();
                    cx.notify();
                }))
                .child(s.close_label),
        )
        .into_any_element()
}

fn ensure_columns_cached_once(ws: &mut DataForgeWorkspace, is_connected: bool) {
    if !ws.metadata_search_ui.open || !is_connected {
        return;
    }
    let key = (
        ws.controller.active_workspace,
        ws.controller.cached_objects.len(),
    );
    if ws.metadata_search_ui.loaded_for == Some(key) {
        return;
    }
    ws.metadata_search_ui.loaded_for = Some(key);
    refresh_cached_columns(ws);
}

fn refresh_cached_columns(ws: &mut DataForgeWorkspace) {
    match ws.controller.fetch_all_table_definitions_blocking() {
        Ok(defs) => {
            ws.metadata_search_ui.cached_defs = defs;
            ws.metadata_search_ui.last_error = None;
        }
        Err(err) => {
            ws.metadata_search_ui.last_error = Some(err.to_string());
        }
    }
}

fn render_search_controls(
    ws: &DataForgeWorkspace,
    is_connected: bool,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let ws_clear = cx.entity().downgrade();
    let search_input = SearchField::new(ws.metadata_search_ui.query_input.clone())
        .placeholder(s.metadata_search_placeholder)
        .full_width()
        .height(px(34.0))
        .text_size(px(13.0))
        .on_change(cx.listener(|ws, val: &str, _w, cx| {
            ws.metadata_search_ui.query = val.to_owned();
            cx.notify();
        }))
        .on_clear(move |_w, cx| {
            let _ = ws_clear.update(cx, |ws, cx| {
                ws.metadata_search_ui.query.clear();
                cx.notify();
            });
        });

    let refresh_btn = Button::new("metadata-search-refresh")
        .variant(Variant::Secondary)
        .size(HeroSize::Sm)
        .is_disabled(!is_connected)
        .on_press(cx.listener(|ws, _, _w, cx| {
            refresh_cached_columns(ws);
            cx.notify();
        }))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(5.0))
                .child(Icon::new(IconName::RefreshCw).size(IconSize::Xs))
                .child(s.refresh_label),
        );

    let filter = ws.metadata_search_ui.filter;
    let pills = div()
        .flex()
        .items_center()
        .justify_between()
        .flex_wrap()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .items_center()
                .flex_wrap()
                .gap(px(6.0))
                .child(filter_pill(
                    "meta-filter-all",
                    s.metadata_type_all,
                    MetadataFilterKind::All,
                    filter == MetadataFilterKind::All,
                    cx,
                ))
                .child(filter_pill(
                    "meta-filter-tables",
                    s.tables,
                    MetadataFilterKind::Tables,
                    filter == MetadataFilterKind::Tables,
                    cx,
                ))
                .child(filter_pill(
                    "meta-filter-views",
                    s.views,
                    MetadataFilterKind::Views,
                    filter == MetadataFilterKind::Views,
                    cx,
                ))
                .child(filter_pill(
                    "meta-filter-columns",
                    s.columns_button,
                    MetadataFilterKind::Columns,
                    filter == MetadataFilterKind::Columns,
                    cx,
                )),
        )
        .child(refresh_btn);

    div()
        .flex()
        .flex_col()
        .gap(px(COMPACT_GAP_PX))
        .child(search_input)
        .child(pills)
        .into_any_element()
}

fn filter_pill(
    id: &'static str,
    label: &'static str,
    kind: MetadataFilterKind,
    active: bool,
    cx: &mut Context<DataForgeWorkspace>,
) -> Button {
    Button::new(id)
        .variant(if active {
            Variant::Primary
        } else {
            Variant::Secondary
        })
        .size(HeroSize::Sm)
        .on_press(cx.listener(move |ws, _, _w, cx| {
            ws.metadata_search_ui.filter = kind;
            cx.notify();
        }))
        .child(label)
}

fn render_results_container(
    ws: &DataForgeWorkspace,
    is_connected: bool,
    matches: &[SearchResultEntry],
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();

    if !is_connected {
        return div()
            .h(px(220.0))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.foreground)
                    .child(s.no_active_connection),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(colors.muted)
                    .child(s.no_active_connection_desc),
            )
            .into_any_element();
    }

    let mut container = div().h(px(340.0)).flex().flex_col().gap(px(6.0));
    if let Some(err) = ws.metadata_search_ui.last_error.as_ref() {
        container = container.child(
            Alert::new(s.structure_metadata_incomplete)
                .status(HeroColor::Warning)
                .description(err.clone()),
        );
    }

    if matches.is_empty() {
        return container
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(4.0))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.foreground)
                            .child(s.no_matching_objects_title),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(colors.muted)
                            .child(s.no_matching_objects_desc),
                    ),
            )
            .into_any_element();
    }

    let mut list = div()
        .id("metadata-search-results")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(4.0));

    for (idx, entry) in matches.iter().take(MAX_DISPLAYED_RESULTS).enumerate() {
        list = list.child(render_result_row(idx, entry, cx));
    }

    container.child(list).into_any_element()
}

fn render_result_row(
    idx: usize,
    entry: &SearchResultEntry,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let (icon, icon_color, type_color) = match entry.kind {
        SearchResultKind::Table => (IconName::Table, colors.accent.color, HeroColor::Accent),
        SearchResultKind::View => (IconName::Layers, colors.accent.color, HeroColor::Default),
        SearchResultKind::Column => (IconName::Columns2, colors.muted, HeroColor::Warning),
        SearchResultKind::Routine | SearchResultKind::Other => {
            (IconName::Code, colors.muted, HeroColor::Default)
        }
    };

    let target_table = entry.target_table.clone();
    let qualified = entry.qualified_name.clone();
    let hover_bg = colors.surface_secondary;
    let border = hairline_border_color(cx);

    div()
        .id(SharedString::from(format!("meta-res-{idx}")))
        .w_full()
        .px(px(10.0))
        .py(px(7.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .border_1()
        .border_color(border)
        .bg(with_alpha(colors.overlay.background, 1.0))
        .hover(move |s| s.bg(hover_bg))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .on_click(cx.listener(move |ws, _, _w, cx| {
            activate_search_result(ws, target_table.as_ref(), &qualified, cx);
        }))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(Icon::new(icon).size(IconSize::Sm).color(icon_color))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(colors.foreground)
                                .child(entry.title.clone()),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(11.0))
                                .text_color(colors.muted)
                                .child(entry.subtitle.clone()),
                        ),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(MICRO_GAP_PX + 2.0))
                .when_some(entry.badge.clone(), |el, badge| {
                    el.child(
                        Chip::new()
                            .variant(ChipVariant::Soft)
                            .color(HeroColor::Accent)
                            .size(HeroSize::Sm)
                            .child(ChipLabel::new().child(badge)),
                    )
                })
                .child(
                    Chip::new()
                        .variant(ChipVariant::Soft)
                        .color(type_color)
                        .size(HeroSize::Sm)
                        .child(ChipLabel::new().child(entry.detail.clone())),
                ),
        )
        .into_any_element()
}

fn render_footer_summary(ws: &DataForgeWorkspace, total: usize, cx: &App) -> AnyElement {
    let colors = cx.colors();
    let s = &ws.controller.strings;
    let shown = total.min(MAX_DISPLAYED_RESULTS);
    let text = if total > shown {
        format!("{shown} / {total} {}", s.object_plural)
    } else if total == 1 {
        format!("1 {}", s.object_singular)
    } else {
        format!("{total} {}", s.object_plural)
    };
    div()
        .flex_1()
        .text_size(px(11.5))
        .text_color(colors.muted)
        .child(text)
        .into_any_element()
}

fn activate_search_result(
    ws: &mut DataForgeWorkspace,
    target_table: Option<&TableRef>,
    qualified_name: &str,
    cx: &mut Context<DataForgeWorkspace>,
) {
    ws.controller.screen_mode = ScreenMode::Workspace;
    ws.workspace_pane.close_tool();

    if let Some(table_ref) = target_table {
        ws.workspace_pane.sub_view = WorkspaceSubView::DataGrid;
        ws.controller.status_bar.view_mode = TableViewMode::Data;
        if let Err(err) = ws.controller.open_table_tab_blocking(table_ref.clone()) {
            ws.notify_toast(
                ToastSeverity::Error,
                ws.controller.strings.failed_to_open_table,
                Some(err.to_string().into()),
                cx,
            );
        }
    } else {
        let conn_id = TabConnectionId(format!("ws-{}", ws.controller.active_workspace.0));
        let title = format!("{} - {qualified_name}", ws.controller.strings.new_query_tab);
        ws.controller.tabs.open_query_tab(conn_id, Some(&title));
        ws.controller
            .set_editor_sql(&format!("-- {qualified_name}\n"));
        ws.workspace_pane.sub_view = WorkspaceSubView::SqlEditor;
    }

    ws.metadata_search_ui.close_modal();
    cx.notify();
}

fn collect_search_results(
    raw_query: &str,
    filter: MetadataFilterKind,
    objects: &[ObjectInfo],
    table_defs: &[TableDefinition],
) -> Vec<SearchResultEntry> {
    let needle = raw_query.trim().to_ascii_lowercase();
    let mut results = Vec::new();

    if !matches!(filter, MetadataFilterKind::Columns) {
        for obj in objects {
            let kind = match obj.kind {
                ObjectKind::Table | ObjectKind::PartitionedTable | ObjectKind::ForeignTable => {
                    SearchResultKind::Table
                }
                ObjectKind::View | ObjectKind::MaterializedView => SearchResultKind::View,
                ObjectKind::Function | ObjectKind::Procedure => SearchResultKind::Routine,
                _ => SearchResultKind::Other,
            };
            let include_by_filter = match filter {
                MetadataFilterKind::All => true,
                MetadataFilterKind::Tables => matches!(kind, SearchResultKind::Table),
                MetadataFilterKind::Views => matches!(kind, SearchResultKind::View),
                MetadataFilterKind::Columns => false,
            };
            if !include_by_filter {
                continue;
            }
            let qualified = qualify_name(obj.schema.as_deref(), &obj.name);
            if !matches_query(&needle, &[&obj.name, &qualified]) {
                continue;
            }
            let target_table = matches!(kind, SearchResultKind::Table | SearchResultKind::View)
                .then(|| TableRef::new(obj.schema.clone(), obj.name.clone()));
            let kind_tag = match kind {
                SearchResultKind::Table => "TABLE",
                SearchResultKind::View => "VIEW",
                SearchResultKind::Routine => "ROUTINE",
                SearchResultKind::Other => "OBJECT",
                SearchResultKind::Column => "COLUMN",
            };
            results.push(SearchResultEntry {
                kind,
                title: obj.name.clone(),
                subtitle: obj.schema.clone().unwrap_or_else(|| "default".to_owned()),
                detail: kind_tag.to_owned(),
                badge: None,
                target_table,
                qualified_name: qualified,
            });
        }
    }

    if matches!(
        filter,
        MetadataFilterKind::All | MetadataFilterKind::Columns
    ) {
        for def in table_defs {
            let table_qualified = qualify_name(def.table.schema.as_deref(), &def.table.name);
            for col in &def.columns {
                let col_name = col.name();
                let col_type = col.info.column_type.name.as_str();
                if !matches_query(&needle, &[col_name, &table_qualified, col_type]) {
                    continue;
                }
                results.push(SearchResultEntry {
                    kind: SearchResultKind::Column,
                    title: col_name.to_owned(),
                    subtitle: table_qualified.clone(),
                    detail: col_type.to_owned(),
                    badge: col.info.primary_key.then(|| "PK".to_owned()),
                    target_table: Some(def.table.clone()),
                    qualified_name: format!("{table_qualified}.{col_name}"),
                });
            }
        }
    }

    results
}

fn qualify_name(schema: Option<&str>, name: &str) -> String {
    match schema.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => format!("{s}.{name}"),
        None => name.to_owned(),
    }
}

fn matches_query(needle: &str, fields: &[&str]) -> bool {
    if needle.is_empty() {
        return true;
    }
    fields
        .iter()
        .any(|f| f.to_ascii_lowercase().contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tablepro_core::column::{ColumnDefault, ColumnInfo, ColumnKind, ColumnType, ReadForm};
    use tablepro_core::ddl::{ColumnAttributes, ColumnDefinition};

    fn sample_col(name: &str, ty: &str, pk: bool) -> ColumnDefinition {
        ColumnDefinition::new(
            ColumnInfo {
                name: name.to_owned(),
                column_type: ColumnType {
                    name: ty.to_owned(),
                    kind: ColumnKind::Other,
                    catalog: None,
                    dynamic_storage: false,
                    read_form: ReadForm::Native,
                },
                nullable: !pk,
                primary_key: pk,
                is_auto_increment: false,
                is_generated: false,
                partition_scoped: false,
                default: ColumnDefault::None,
                comment: None,
            },
            ColumnAttributes::default(),
        )
    }

    #[test]
    fn metadata_search_filters_tables_views_and_columns() {
        let objects = vec![
            ObjectInfo::new(Some("public".to_owned()), "users", ObjectKind::Table),
            ObjectInfo::new(Some("public".to_owned()), "active_users", ObjectKind::View),
        ];
        let defs = vec![TableDefinition {
            table: TableRef::new(Some("public".to_owned()), "users"),
            columns: vec![
                sample_col("id", "INTEGER", true),
                sample_col("email", "TEXT", false),
            ],
            primary_key: None,
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            inbound_foreign_keys: Vec::new(),
            source: None,
            options: None,
        }];

        let all = collect_search_results("user", MetadataFilterKind::All, &objects, &defs);
        assert_eq!(all.len(), 4);

        let only_views = collect_search_results("user", MetadataFilterKind::Views, &objects, &defs);
        assert_eq!(only_views.len(), 1);
        assert_eq!(only_views[0].title, "active_users");

        let only_cols =
            collect_search_results("email", MetadataFilterKind::Columns, &objects, &defs);
        assert_eq!(only_cols.len(), 1);
        assert_eq!(only_cols[0].title, "email");
        assert_eq!(
            only_cols[0].target_table,
            Some(TableRef::new(Some("public".to_owned()), "users"))
        );
    }
}
