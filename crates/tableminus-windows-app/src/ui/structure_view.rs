//! Table structure & metadata view rendered as a crisp Data-Grid-style table.
//!
//! Displays real inspected columns, indexes, outbound & inbound foreign keys,
//! triggers, and syntax-highlighted `CREATE TABLE` DDL via `TableDefinition`
//! fetched non-blockingly from the active session, with live column search.

use std::collections::HashMap;
use std::sync::Arc;

use herogpui::components::{
    Alert, Button, Chip, ChipLabel, ChipVariant, Icon, IconName, IconSize, InputState, SearchField,
    Skeleton, TabItem, Tabs, TabsSize, TabsVariant,
};
use herogpui::core::{Color as HeroColor, Size as HeroSize, Variant, with_alpha};
use herogpui::gpui::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Div, Entity, Font, FontFeatures,
    FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use herogpui::prelude::FluentBuilder;
use herogpui::theme::ActiveTheme;
use tablepro_core::column::ColumnDefault;
use tablepro_core::ddl::{InboundForeignKey, SchemaObjectKind, TableDefinition};
use tablepro_core::foreign_key_info::ForeignKeyInfo;
use tablepro_core::meta::{IndexInfo, IndexKey, ObjectKind, TableRef};
use tablepro_session::{OperationError, OperationKind};

use super::results_pane::format_core_value;
use super::shell_state::{ToastSeverity, WorkspaceToolView};
use super::{TableMinusWorkspace, syntax_bridge, theme};

const GRID_ROW_HEIGHT_PX: f32 = 28.0;
const GRID_HEADER_HEIGHT_PX: f32 = 32.0;

/// Sub-tabs within the Structure View.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StructureSubTab {
    #[default]
    Columns,
    Indexes,
    ForeignKeys,
    DdlAndTriggers,
}

impl StructureSubTab {
    const fn key(self) -> &'static str {
        match self {
            Self::Columns => "columns",
            Self::Indexes => "indexes",
            Self::ForeignKeys => "foreign-keys",
            Self::DdlAndTriggers => "ddl-triggers",
        }
    }

    fn from_key(key: &str) -> Self {
        match key {
            "indexes" => Self::Indexes,
            "foreign-keys" => Self::ForeignKeys,
            "ddl-triggers" => Self::DdlAndTriggers,
            _ => Self::Columns,
        }
    }
}

/// Sortable columns in the Structure Columns grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureSortColumn {
    Column,
    Type,
    Nullable,
}

/// Persistent UI state for the Structure View pane.
#[derive(Debug)]
pub struct StructureViewState {
    pub selected_column: Option<usize>,
    pub selected_index: Option<usize>,
    pub selected_fk: Option<usize>,
    pub active_tab: StructureSubTab,
    pub loaded_table: Option<TableRef>,
    pub loading_table: Option<TableRef>,
    pub definition: Option<TableDefinition>,
    pub ddl_text: Option<String>,
    pub load_error: Option<String>,
    pub sort_column: Option<StructureSortColumn>,
    pub sort_ascending: bool,
    pub column_search_input: Entity<InputState>,
    pub column_search_query: String,
}

impl StructureViewState {
    /// Creates initial Structure View state.
    pub fn new(_window: &mut Window, cx: &mut Context<TableMinusWorkspace>) -> Self {
        Self {
            selected_column: None,
            selected_index: None,
            selected_fk: None,
            active_tab: StructureSubTab::default(),
            loaded_table: None,
            loading_table: None,
            definition: None,
            ddl_text: None,
            load_error: None,
            sort_column: None,
            sort_ascending: true,
            column_search_input: cx.new(|cx| InputState::new(cx)),
            column_search_query: String::new(),
        }
    }

    /// Returns cached outbound foreign keys for `table` if `TableDefinition` has loaded.
    #[must_use]
    pub fn cached_foreign_keys(&self, table: &TableRef) -> Option<&[ForeignKeyInfo]> {
        if self.loaded_table.as_ref() == Some(table) {
            self.definition
                .as_ref()
                .map(|def| def.foreign_keys.as_slice())
        } else {
            None
        }
    }

    /// Triggers an async `fetch_table_definition` if `table` has not yet been fetched.
    pub(crate) fn ensure_table_loaded(
        ws: &mut TableMinusWorkspace,
        table: &TableRef,
        cx: &mut Context<TableMinusWorkspace>,
    ) {
        if ws.structure_ui.loaded_table.as_ref() == Some(table)
            || ws.structure_ui.loading_table.as_ref() == Some(table)
        {
            return;
        }
        start_fetch_definition(ws, table.clone(), false, cx);
    }
}

/// Renders the Structure View pane for `ws.controller.active_table`.
pub fn render_structure_view(
    ws: &mut TableMinusWorkspace,
    _window: &mut Window,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let Some(table) = ws.controller.active_table.clone() else {
        return render_no_table_selected(&ws.controller.strings, cx);
    };

    StructureViewState::ensure_table_loaded(ws, &table, cx);

    let header = render_structure_header(ws, &table, cx);
    let body = render_structure_body(ws, &table, cx);

    div()
        .id("structure-view-root")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme::panel_surface_bg(cx))
        .child(header)
        .child(div().flex_1().min_h_0().w_full().child(body))
        .into_any_element()
}

fn start_fetch_definition(
    ws: &mut TableMinusWorkspace,
    table: TableRef,
    force_refresh: bool,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let Some(services) = ws.controller.services.clone() else {
        ws.structure_ui.loaded_table = Some(table);
        ws.structure_ui.loading_table = None;
        return;
    };
    let workspace_id = ws.controller.active_workspace;
    ws.structure_ui.loading_table = Some(table.clone());
    if force_refresh {
        ws.structure_ui.load_error = None;
    }

    let (tx, rx) = async_channel::bounded::<Result<(TableDefinition, Option<String>), String>>(1);
    let no_session_msg = ws.controller.strings.no_active_connection;
    spawn_definition_fetch_task(services, workspace_id, table.clone(), no_session_msg, tx);

    cx.spawn(async move |this, cx| {
        if let Ok(result) = rx.recv().await {
            let _ = this.update(cx, |ws, cx| {
                apply_fetched_definition(ws, table, result);
                cx.notify();
            });
        }
    })
    .detach();
}

fn spawn_definition_fetch_task(
    services: Arc<crate::services::ServiceHub>,
    workspace_id: tableminus_state::connection_window::WorkspaceId,
    table: TableRef,
    no_session_msg: &'static str,
    tx: async_channel::Sender<Result<(TableDefinition, Option<String>), String>>,
) {
    let runtime = services.runtime.clone();
    runtime.spawn(async move {
        let outcome = async {
            let guard = services.sessions.read().await;
            let session = guard
                .session(workspace_id)
                .ok_or_else(|| no_session_msg.to_owned())?;
            let def = session
                .run(OperationKind::Metadata, |lease, call| {
                    let target = table.clone();
                    async move {
                        lease
                            .fetch_table_definition(&target, call)
                            .await
                            .map_err(OperationError::from)
                    }
                })
                .await
                .map_err(|err| err.to_string())?;
            let ddl = resolve_definition_ddl(&def, session);
            Ok::<_, String>((def, ddl))
        }
        .await;
        let _ = tx.send(outcome).await;
    });
}

fn resolve_definition_ddl(
    def: &TableDefinition,
    session: Arc<tablepro_session::Session>,
) -> Option<String> {
    if let Some(source) = &def.source {
        let mut script = source.create_statement.clone();
        for dep in &source.dependents {
            if !dep.sql.trim().is_empty() {
                script.push_str("\n\n");
                script.push_str(&dep.sql);
            }
        }
        Some(script)
    } else if let Some(dialect) = session.current_dialect()
        && let Some(ddl_dialect) = dialect.ddl()
    {
        ddl_dialect
            .create_table_script(def)
            .ok()
            .map(|plan| plan.sql().join(";\n\n"))
    } else {
        None
    }
}

fn apply_fetched_definition(
    ws: &mut TableMinusWorkspace,
    table: TableRef,
    result: Result<(TableDefinition, Option<String>), String>,
) {
    if ws.controller.active_table.as_ref() != Some(&table)
        && ws.structure_ui.loading_table.as_ref() != Some(&table)
    {
        return;
    }
    let table_changed = ws.structure_ui.loaded_table.as_ref() != Some(&table);
    ws.structure_ui.loading_table = None;
    ws.structure_ui.loaded_table = Some(table);
    if table_changed {
        ws.structure_ui.selected_column = None;
        ws.structure_ui.selected_index = None;
        ws.structure_ui.selected_fk = None;
    }
    match result {
        Ok((def, ddl)) => {
            ws.structure_ui.load_error = None;
            ws.controller.structure_columns = columns_from_definition(&def);
            ws.structure_ui.definition = Some(def);
            ws.structure_ui.ddl_text = ddl;
        }
        Err(err) => {
            ws.structure_ui.load_error = Some(err);
        }
    }
}

fn columns_from_definition(
    def: &TableDefinition,
) -> Vec<crate::views::content::StructureColumnRow> {
    def.columns
        .iter()
        .map(|col| {
            let default_expr = match &col.info.default {
                ColumnDefault::None => None,
                ColumnDefault::Literal(val) => Some(format_core_value(val)),
                ColumnDefault::Expression(expr) => Some(expr.as_sql().to_owned()),
            };
            crate::views::content::StructureColumnRow {
                name: col.info.name.clone(),
                data_type: col.info.column_type.name.clone(),
                nullable: col.info.nullable,
                default_expr,
                is_primary_key: col.info.primary_key,
            }
        })
        .collect()
}

fn collect_primary_key_summary(
    ws: &TableMinusWorkspace,
    def: Option<&TableDefinition>,
) -> Option<String> {
    let pk_cols: Vec<&str> = if let Some(d) = def {
        d.columns
            .iter()
            .filter(|c| c.info.primary_key)
            .map(|c| c.info.name.as_str())
            .collect()
    } else {
        ws.controller
            .structure_columns
            .iter()
            .filter(|c| c.is_primary_key)
            .map(|c| c.name.as_str())
            .collect()
    };
    if pk_cols.is_empty() {
        None
    } else if pk_cols.len() == 1 {
        Some(format!("PK: {}", pk_cols[0]))
    } else {
        Some(format!("PK: ({})", pk_cols.join(", ")))
    }
}

fn render_structure_header(
    ws: &TableMinusWorkspace,
    table: &TableRef,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let qualified = match &table.schema {
        Some(schema) => format!("{schema}.{}", table.name),
        None => table.name.clone(),
    };
    let def = ws
        .structure_ui
        .loaded_table
        .as_ref()
        .filter(|loaded| *loaded == table)
        .and(ws.structure_ui.definition.as_ref());

    let pk_summary = collect_primary_key_summary(ws, def);
    let tabs = render_structure_subtabs(ws, def, cx);
    let is_loading = ws.structure_ui.loading_table.as_ref() == Some(table);
    let table_for_refresh = table.clone();
    let accent = cx.colors().accent.color;

    div()
        .w_full()
        .px(px(12.0))
        .pt(px(8.0))
        .pb(px(4.0))
        .border_b_1()
        .border_color(theme::hairline_border_color(cx))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            Icon::new(IconName::Database)
                                .size(IconSize::Sm)
                                .color(accent),
                        )
                        .child(
                            div()
                                .text_size(px(theme::TEXT_TITLE_PX))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(cx.colors().foreground)
                                .truncate()
                                .child(qualified),
                        )
                        .when_some(pk_summary, |el, pk| {
                            el.child(
                                div()
                                    .px(px(6.0))
                                    .py(px(1.0))
                                    .rounded(px(4.0))
                                    .bg(with_alpha(accent, 0.12))
                                    .border_1()
                                    .border_color(with_alpha(accent, 0.30))
                                    .text_size(px(theme::TEXT_BADGE_PX))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(accent)
                                    .whitespace_nowrap()
                                    .child(pk),
                            )
                        })
                        .when_some(def.and_then(|d| d.options.as_ref()), |el, opts| {
                            el.when_some(opts.collation.as_ref(), |el, coll| {
                                el.child(
                                    div()
                                        .text_size(px(theme::TEXT_META_PX))
                                        .text_color(cx.colors().muted)
                                        .truncate()
                                        .max_w(px(240.0))
                                        .child(coll.clone()),
                                )
                            })
                        }),
                )
                .child(render_structure_header_actions(
                    ws,
                    is_loading,
                    table_for_refresh,
                    cx,
                )),
        )
        .child(tabs)
        .into_any_element()
}

fn render_structure_header_actions(
    ws: &TableMinusWorkspace,
    is_loading: bool,
    table_for_refresh: TableRef,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let ws_for_clear = cx.entity().downgrade();
    let search = SearchField::new(ws.structure_ui.column_search_input.clone())
        .placeholder("Search column name...")
        .full_width()
        .height(px(28.0))
        .text_size(px(theme::TEXT_CONTROL_PX))
        .on_change(cx.listener(|ws, value: &str, _, cx| {
            ws.structure_ui.column_search_query = value.to_owned();
            cx.notify();
        }))
        .on_clear(move |_window, cx| {
            let _ = ws_for_clear.update(cx, |ws, cx| {
                ws.structure_ui.column_search_query.clear();
                cx.notify();
            });
        });

    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(
            div()
                .id("structure-column-search")
                .w(px(210.0))
                .child(search),
        )
        .child(
            Button::new("structure-alter-schema-btn")
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .child(Icon::new(IconName::SquarePen).size(IconSize::Xs))
                .label(format!("{} / {}", s.schema_alter_table, s.schema_designer))
                .on_press(cx.listener(|ws, _ev, _w, cx| {
                    ws.workspace_pane
                        .open_tool(WorkspaceToolView::SchemaDesigner);
                    cx.notify();
                })),
        )
        .child(
            Button::new("structure-refresh-btn")
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .is_disabled(is_loading)
                .child(Icon::new(IconName::RotateCcw).size(IconSize::Xs))
                .label(if is_loading {
                    s.refreshing_label
                } else {
                    s.refresh_label
                })
                .on_press(cx.listener(move |ws, _ev, _w, cx| {
                    start_fetch_definition(ws, table_for_refresh.clone(), true, cx);
                    cx.notify();
                })),
        )
        .into_any_element()
}

fn render_structure_subtabs(
    ws: &TableMinusWorkspace,
    def: Option<&TableDefinition>,
    cx: &mut Context<TableMinusWorkspace>,
) -> Tabs {
    let col_count = def.map_or(ws.controller.structure_columns.len(), |d| d.columns.len());
    let idx_count = def.map_or(0, |d| d.indexes.len());
    let fk_count = def.map_or(0, |d| d.foreign_keys.len() + d.inbound_foreign_keys.len());
    let s = &ws.controller.strings;
    let items = vec![
        TabItem::new("columns", format!("{} ({col_count})", s.columns_button)),
        TabItem::new("indexes", format!("{} ({idx_count})", s.indexes_label)),
        TabItem::new(
            "foreign-keys",
            format!("{} ({fk_count})", s.foreign_keys_label),
        ),
        TabItem::new("ddl-triggers", s.ddl_triggers_tab),
    ];
    let selected_key = SharedString::from(ws.structure_ui.active_tab.key());
    let entity = cx.entity().clone();

    Tabs::new("structure-subtabs", items, selected_key.clone())
        .selected_key(selected_key)
        .variant(TabsVariant::Secondary)
        .size(TabsSize::Sm)
        .on_selection_change(move |key, _w, cx| {
            let next = StructureSubTab::from_key(key.as_ref());
            entity.update(cx, |ws, cx| {
                ws.structure_ui.active_tab = next;
                cx.notify();
            });
        })
}

fn render_structure_body(
    ws: &mut TableMinusWorkspace,
    table: &TableRef,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let is_loading = ws.structure_ui.loading_table.as_ref() == Some(table);
    let def_matches = ws.structure_ui.loaded_table.as_ref() == Some(table);

    if is_loading && !def_matches && ws.controller.structure_columns.is_empty() {
        return render_structure_skeleton(&ws.controller.strings, cx);
    }

    let s = &ws.controller.strings;
    let err_banner = ws.structure_ui.load_error.as_ref().map(|err| {
        div()
            .px(px(12.0))
            .pt(px(8.0))
            .child(
                Alert::new(s.structure_metadata_incomplete)
                    .status(HeroColor::Warning)
                    .description(err.clone()),
            )
            .into_any_element()
    });

    let def = if def_matches {
        ws.structure_ui.definition.as_ref()
    } else {
        None
    };

    let tab_content = match ws.structure_ui.active_tab {
        StructureSubTab::Columns => render_columns_grid(ws, def, cx),
        StructureSubTab::Indexes => render_indexes_grid(ws, def, cx),
        StructureSubTab::ForeignKeys => render_foreign_keys_grid(ws, def, cx),
        StructureSubTab::DdlAndTriggers => render_ddl_and_triggers(ws, table, def, cx),
    };

    div()
        .size_full()
        .flex()
        .flex_col()
        .children(err_banner)
        .child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .p(px(10.0))
                .child(tab_content),
        )
        .into_any_element()
}

struct StructureColumnDisplay {
    orig_idx: usize,
    name: String,
    dtype: String,
    nullable: bool,
    default_expr: String,
    attrs: Vec<String>,
    foreign_key: String,
    comment: String,
}

fn build_column_fk_map(def: Option<&TableDefinition>) -> HashMap<String, String> {
    let mut fk_by_col = HashMap::new();
    let Some(d) = def else {
        return fk_by_col;
    };
    for fk in &d.foreign_keys {
        let target_table = match &fk.ref_schema {
            Some(schema) => format!("{schema}.{}", fk.ref_table),
            None => fk.ref_table.clone(),
        };
        for (i, col_name) in fk.columns.iter().enumerate() {
            let ref_col = fk.ref_columns.get(i).or_else(|| fk.ref_columns.first());
            let target = match ref_col {
                Some(rc) => format!("-> {target_table}({rc})"),
                None => format!("-> {target_table}"),
            };
            fk_by_col.entry(col_name.clone()).or_insert(target);
        }
    }
    fk_by_col
}

fn collect_structure_column_entries(
    ws: &TableMinusWorkspace,
    def: Option<&TableDefinition>,
    fk_by_col: &HashMap<String, String>,
) -> Vec<StructureColumnDisplay> {
    let query = ws
        .structure_ui
        .column_search_query
        .trim()
        .to_ascii_lowercase();

    let mut entries: Vec<StructureColumnDisplay> = if let Some(d) = def {
        d.columns
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let mut attrs = Vec::new();
                if c.info.primary_key {
                    attrs.push("PK".to_owned());
                }
                if c.info.is_auto_increment {
                    attrs.push("AUTO_INC".to_owned());
                }
                if c.info.is_generated {
                    attrs.push("GENERATED".to_owned());
                }
                StructureColumnDisplay {
                    orig_idx: i,
                    name: c.info.name.clone(),
                    dtype: c.info.column_type.name.clone(),
                    nullable: c.info.nullable,
                    default_expr: format_column_default(&c.info.default),
                    attrs,
                    foreign_key: fk_by_col.get(&c.info.name).cloned().unwrap_or_default(),
                    comment: c.info.comment.clone().unwrap_or_default(),
                }
            })
            .collect()
    } else {
        ws.controller
            .structure_columns
            .iter()
            .enumerate()
            .map(|(i, c)| StructureColumnDisplay {
                orig_idx: i,
                name: c.name.clone(),
                dtype: c.data_type.clone(),
                nullable: c.nullable,
                default_expr: c.default_expr.clone().unwrap_or_default(),
                attrs: if c.is_primary_key {
                    vec!["PK".to_owned()]
                } else {
                    Vec::new()
                },
                foreign_key: fk_by_col.get(&c.name).cloned().unwrap_or_default(),
                comment: String::new(),
            })
            .collect()
    };

    if !query.is_empty() {
        entries.retain(|e| {
            e.name.to_ascii_lowercase().contains(&query)
                || e.dtype.to_ascii_lowercase().contains(&query)
        });
    }

    if let Some(sort_col) = ws.structure_ui.sort_column {
        let asc = ws.structure_ui.sort_ascending;
        entries.sort_by(|a, b| {
            let ord = match sort_col {
                StructureSortColumn::Column => a.name.cmp(&b.name),
                StructureSortColumn::Type => a.dtype.cmp(&b.dtype),
                StructureSortColumn::Nullable => a.nullable.cmp(&b.nullable),
            };
            if asc { ord } else { ord.reverse() }
        });
    }
    entries
}

fn grid_cell_box(
    fixed_width: Option<f32>,
    min_width: Option<f32>,
    border_right: Option<Hsla>,
) -> Div {
    let mut cell = div()
        .h_full()
        .px(px(8.0))
        .min_w_0()
        .overflow_hidden()
        .flex()
        .items_center();
    if let Some(w) = fixed_width {
        cell = cell.w(px(w)).flex_shrink_0();
    } else {
        cell = cell.flex_1();
        if let Some(mw) = min_width {
            cell = cell.min_w(px(mw));
        }
    }
    if let Some(b) = border_right {
        cell = cell.border_r_1().border_color(b);
    }
    cell
}

fn grid_ellipsis_text(text: impl Into<SharedString>) -> Div {
    div()
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(text.into())
}

fn render_grid_shell(
    scroll_id: &'static str,
    header_row: AnyElement,
    body_rows: AnyElement,
    border: Hsla,
) -> AnyElement {
    div()
        .size_full()
        .border_1()
        .border_color(border)
        .rounded(px(theme::CONTROL_RADIUS_PX))
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(header_row)
        .child(
            div()
                .id(scroll_id)
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_y_scroll()
                .child(body_rows),
        )
        .into_any_element()
}

#[derive(Clone, Copy)]
struct GridHeaderStyle {
    bg: Hsla,
    fg: Hsla,
    border: Hsla,
}

fn render_sortable_header_cell(
    id: &'static str,
    label: &'static str,
    sort_key: StructureSortColumn,
    active_sort: Option<(StructureSortColumn, bool)>,
    fixed_width: f32,
    style: GridHeaderStyle,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let is_active = active_sort.map(|(c, _)| c) == Some(sort_key);
    let indicator = match active_sort {
        Some((c, true)) if c == sort_key => Some("↑"),
        Some((c, false)) if c == sort_key => Some("↓"),
        _ => None,
    };
    let accent = cx.colors().accent.color;

    grid_cell_box(Some(fixed_width), None, Some(style.border))
        .id(id)
        .cursor_pointer()
        .justify_between()
        .gap(px(4.0))
        .hover(|s| s.bg(cx.colors().surface_secondary.opacity(0.6)))
        .on_click(cx.listener(move |ws, _ev, _w, cx| {
            if ws.structure_ui.sort_column == Some(sort_key) {
                if ws.structure_ui.sort_ascending {
                    ws.structure_ui.sort_ascending = false;
                } else {
                    ws.structure_ui.sort_column = None;
                    ws.structure_ui.sort_ascending = true;
                }
            } else {
                ws.structure_ui.sort_column = Some(sort_key);
                ws.structure_ui.sort_ascending = true;
            }
            cx.notify();
        }))
        .child(
            grid_ellipsis_text(label)
                .text_size(px(theme::TEXT_META_PX))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if is_active { accent } else { style.fg }),
        )
        .when_some(indicator, |el, arrow| {
            el.child(
                div()
                    .flex_shrink_0()
                    .text_size(px(theme::TEXT_BADGE_PX))
                    .font_weight(FontWeight::BOLD)
                    .text_color(accent)
                    .child(arrow),
            )
        })
        .into_any_element()
}

fn render_static_header_cell(
    label: &'static str,
    fixed_width: Option<f32>,
    min_width: Option<f32>,
    border_right: Option<Hsla>,
    hdr_fg: Hsla,
) -> AnyElement {
    grid_cell_box(fixed_width, min_width, border_right)
        .child(
            grid_ellipsis_text(label)
                .text_size(px(theme::TEXT_META_PX))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(hdr_fg),
        )
        .into_any_element()
}

fn render_columns_grid_header(
    ws: &TableMinusWorkspace,
    show_fk_col: bool,
    style: GridHeaderStyle,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let active_sort = ws
        .structure_ui
        .sort_column
        .map(|c| (c, ws.structure_ui.sort_ascending));

    div()
        .w_full()
        .h(px(GRID_HEADER_HEIGHT_PX))
        .flex_shrink_0()
        .bg(style.bg)
        .border_b_1()
        .border_color(style.border)
        .flex()
        .items_center()
        .overflow_hidden()
        .child(
            grid_cell_box(Some(44.0), None, Some(style.border))
                .justify_end()
                .child(
                    div()
                        .text_size(px(theme::TEXT_BADGE_PX))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(cx.colors().muted)
                        .child("#"),
                ),
        )
        .child(render_sortable_header_cell(
            "struct-hdr-col",
            s.struct_col_column,
            StructureSortColumn::Column,
            active_sort,
            190.0,
            style,
            cx,
        ))
        .child(render_sortable_header_cell(
            "struct-hdr-type",
            s.struct_col_type,
            StructureSortColumn::Type,
            active_sort,
            150.0,
            style,
            cx,
        ))
        .child(render_sortable_header_cell(
            "struct-hdr-nullable",
            s.struct_col_nullable,
            StructureSortColumn::Nullable,
            active_sort,
            86.0,
            style,
            cx,
        ))
        .child(render_static_header_cell(
            s.struct_col_default,
            Some(150.0),
            None,
            Some(style.border),
            style.fg,
        ))
        .child(render_static_header_cell(
            s.struct_col_attributes,
            Some(140.0),
            None,
            Some(style.border),
            style.fg,
        ))
        .when(show_fk_col, |el| {
            el.child(render_static_header_cell(
                s.foreign_keys_label,
                Some(180.0),
                None,
                Some(style.border),
                style.fg,
            ))
        })
        .child(render_static_header_cell(
            s.struct_col_comment,
            None,
            Some(120.0),
            None,
            style.fg,
        ))
        .into_any_element()
}

fn render_columns_grid(
    ws: &TableMinusWorkspace,
    def: Option<&TableDefinition>,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let border = theme::hairline_border_color(cx);
    let style = GridHeaderStyle {
        bg: ws.content_palette.data_grid_header_background,
        fg: ws.content_palette.data_grid_header_text,
        border,
    };
    let fk_by_col = build_column_fk_map(def);
    let show_fk_col = !fk_by_col.is_empty();
    let entries = collect_structure_column_entries(ws, def, &fk_by_col);
    let header_row = render_columns_grid_header(ws, show_fk_col, style, cx);

    let body_rows = if entries.is_empty() {
        let empty_msg = if ws.structure_ui.column_search_query.trim().is_empty() {
            s.structure_no_columns_empty.to_owned()
        } else {
            format!(
                "No columns matching \"{}\"",
                ws.structure_ui.column_search_query.trim()
            )
        };
        div()
            .w_full()
            .p(px(24.0))
            .text_size(px(theme::TEXT_META_PX))
            .text_color(cx.colors().muted)
            .child(empty_msg)
            .into_any_element()
    } else {
        let font = theme::value_gpui_font(&ws.controller.settings);
        let selected_column = ws.structure_ui.selected_column;
        let mut rows_col = div().w_full().flex().flex_col();
        for (row_idx, entry) in entries.into_iter().enumerate() {
            let is_selected = selected_column == Some(entry.orig_idx);
            rows_col = rows_col.child(render_column_grid_row(
                row_idx,
                entry,
                is_selected,
                show_fk_col,
                &font,
                border,
                s,
                cx,
            ));
        }
        rows_col.into_any_element()
    };

    render_grid_shell("structure-columns-scroll", header_row, body_rows, border)
}

fn render_column_nullable_cell(
    nullable: bool,
    border: Hsla,
    s: &crate::i18n::Strings,
    cx: &App,
) -> AnyElement {
    grid_cell_box(Some(86.0), None, Some(border))
        .child(
            grid_ellipsis_text(if nullable {
                s.nullable_yes
            } else {
                s.not_null_badge
            })
            .text_size(px(theme::TEXT_BADGE_PX))
            .font_weight(if nullable {
                FontWeight::NORMAL
            } else {
                FontWeight::SEMIBOLD
            })
            .text_color(if nullable {
                cx.colors().muted
            } else {
                cx.colors().foreground
            }),
        )
        .into_any_element()
}

fn render_grid_index_cell(
    display_num: usize,
    is_selected: bool,
    accent: Hsla,
    border: Hsla,
    cx: &App,
) -> AnyElement {
    grid_cell_box(Some(44.0), None, Some(border))
        .justify_end()
        .font_features(tnum_features())
        .text_size(px(theme::TEXT_META_PX))
        .text_color(if is_selected {
            accent
        } else {
            cx.colors().muted
        })
        .child(format!("{display_num}"))
        .into_any_element()
}

#[expect(
    clippy::too_many_arguments,
    reason = "renders a single structured column row with selection, FK flag, font, and i18n strings"
)]
fn render_column_grid_row(
    row_idx: usize,
    entry: StructureColumnDisplay,
    is_selected: bool,
    show_fk_col: bool,
    font: &Font,
    border: Hsla,
    s: &crate::i18n::Strings,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let orig_idx = entry.orig_idx;
    let accent = cx.colors().accent.color;
    let row_bg = if is_selected {
        Some(with_alpha(accent, 0.10))
    } else {
        theme::row_zebra_bg(row_idx, cx)
    };
    let name_weight = if is_selected {
        FontWeight::SEMIBOLD
    } else {
        FontWeight::MEDIUM
    };
    let name_color = if is_selected {
        accent
    } else {
        cx.colors().foreground
    };

    div()
        .id(("struct-col-row", orig_idx))
        .w_full()
        .h(px(GRID_ROW_HEIGHT_PX))
        .flex_shrink_0()
        .border_b_1()
        .border_color(border)
        .when_some(row_bg, |el, bg| el.bg(bg))
        .hover(move |style| {
            if is_selected {
                style
            } else {
                style.bg(with_alpha(accent, 0.05))
            }
        })
        .cursor_pointer()
        .flex()
        .items_center()
        .overflow_hidden()
        .on_click(cx.listener(move |ws, _ev, _w, cx| {
            ws.structure_ui.selected_column = if ws.structure_ui.selected_column == Some(orig_idx) {
                None
            } else {
                Some(orig_idx)
            };
            cx.notify();
        }))
        .child(render_grid_index_cell(
            orig_idx + 1,
            is_selected,
            accent,
            border,
            cx,
        ))
        .child(
            grid_cell_box(Some(190.0), None, Some(border)).child(
                grid_ellipsis_text(entry.name)
                    .font(font.clone())
                    .text_size(px(theme::TEXT_CONTROL_PX))
                    .font_weight(name_weight)
                    .text_color(name_color),
            ),
        )
        .child(
            grid_cell_box(Some(150.0), None, Some(border)).child(
                grid_ellipsis_text(entry.dtype)
                    .font(font.clone())
                    .text_size(px(theme::TEXT_META_PX))
                    .text_color(accent),
            ),
        )
        .child(render_column_nullable_cell(entry.nullable, border, s, cx))
        .child(
            grid_cell_box(Some(150.0), None, Some(border)).child(render_grid_mono_or_dash(
                &entry.default_expr,
                font,
                cx.colors().foreground,
                cx.colors().muted,
            )),
        )
        .child(
            grid_cell_box(Some(140.0), None, Some(border))
                .gap(px(4.0))
                .child(render_column_attrs_inline(entry.attrs, cx)),
        )
        .when(show_fk_col, |el| {
            el.child(grid_cell_box(Some(180.0), None, Some(border)).child(
                render_grid_mono_or_dash(&entry.foreign_key, font, accent, cx.colors().muted),
            ))
        })
        .child(
            grid_cell_box(None, Some(120.0), None).child(render_grid_mono_or_dash(
                &entry.comment,
                font,
                cx.colors().muted,
                cx.colors().muted,
            )),
        )
        .into_any_element()
}

fn render_grid_mono_or_dash(text: &str, font: &Font, fg: Hsla, muted: Hsla) -> AnyElement {
    if text.is_empty() {
        grid_ellipsis_text("—")
            .text_size(px(theme::TEXT_META_PX))
            .text_color(muted)
            .into_any_element()
    } else {
        grid_ellipsis_text(text.to_owned())
            .font(font.clone())
            .text_size(px(theme::TEXT_META_PX))
            .text_color(fg)
            .into_any_element()
    }
}

fn render_column_attrs_inline(attrs: Vec<String>, cx: &App) -> AnyElement {
    if attrs.is_empty() {
        return div()
            .text_size(px(theme::TEXT_META_PX))
            .text_color(cx.colors().muted)
            .child("—")
            .into_any_element();
    }
    let accent = cx.colors().accent.color;
    let mut row = div()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(px(4.0));
    for attr in attrs {
        let is_pk = attr == "PK";
        row = row.child(
            div()
                .flex_shrink_0()
                .px(px(5.0))
                .py(px(1.0))
                .rounded(px(3.0))
                .bg(if is_pk {
                    with_alpha(accent, 0.14)
                } else {
                    cx.colors().surface_secondary
                })
                .text_size(px(theme::TEXT_BADGE_PX))
                .font_weight(if is_pk {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(if is_pk { accent } else { cx.colors().muted })
                .child(attr),
        );
    }
    row.into_any_element()
}

fn render_indexes_grid(
    ws: &TableMinusWorkspace,
    def: Option<&TableDefinition>,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let border = theme::hairline_border_color(cx);
    let hdr_bg = ws.content_palette.data_grid_header_background;
    let hdr_fg = ws.content_palette.data_grid_header_text;

    let header_row = div()
        .w_full()
        .h(px(GRID_HEADER_HEIGHT_PX))
        .flex_shrink_0()
        .bg(hdr_bg)
        .border_b_1()
        .border_color(border)
        .flex()
        .items_center()
        .overflow_hidden()
        .child(
            grid_cell_box(Some(44.0), None, Some(border))
                .justify_end()
                .child(
                    div()
                        .text_size(px(theme::TEXT_BADGE_PX))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(cx.colors().muted)
                        .child("#"),
                ),
        )
        .child(render_static_header_cell(
            s.struct_idx_name,
            Some(210.0),
            None,
            Some(border),
            hdr_fg,
        ))
        .child(render_static_header_cell(
            s.struct_idx_keys,
            Some(260.0),
            None,
            Some(border),
            hdr_fg,
        ))
        .child(render_static_header_cell(
            s.struct_idx_flags,
            Some(150.0),
            None,
            Some(border),
            hdr_fg,
        ))
        .child(render_static_header_cell(
            s.struct_idx_predicate,
            None,
            Some(160.0),
            None,
            hdr_fg,
        ))
        .into_any_element();

    let query = ws
        .structure_ui
        .column_search_query
        .trim()
        .to_ascii_lowercase();
    let indexes: Vec<&IndexInfo> = def
        .map(|d| {
            d.indexes
                .iter()
                .filter(|idx| index_matches_query(idx, &query))
                .collect()
        })
        .unwrap_or_default();

    let body_rows = if indexes.is_empty() {
        div()
            .w_full()
            .p(px(24.0))
            .text_size(px(theme::TEXT_META_PX))
            .text_color(cx.colors().muted)
            .child(s.structure_no_indexes_empty)
            .into_any_element()
    } else {
        let font = theme::value_gpui_font(&ws.controller.settings);
        let selected_index = ws.structure_ui.selected_index;
        let mut rows_col = div().w_full().flex().flex_col();
        for (row_idx, idx) in indexes.into_iter().enumerate() {
            let is_selected = selected_index == Some(row_idx);
            rows_col = rows_col.child(render_index_grid_row(
                row_idx,
                idx,
                is_selected,
                &font,
                border,
                cx,
            ));
        }
        rows_col.into_any_element()
    };

    render_grid_shell("structure-indexes-scroll", header_row, body_rows, border)
}

fn index_matches_query(idx: &IndexInfo, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    if idx.name.to_ascii_lowercase().contains(query) {
        return true;
    }
    idx.keys.iter().any(|k| {
        let key_name = match k {
            IndexKey::Column { name, .. } | IndexKey::Expression { sql: name, .. } => name,
        };
        key_name.to_ascii_lowercase().contains(query)
    })
}

fn format_index_keys(idx: &IndexInfo) -> String {
    idx.keys
        .iter()
        .map(|k| match k {
            IndexKey::Column { name, descending }
            | IndexKey::Expression {
                sql: name,
                descending,
            } => {
                if *descending {
                    format!("{name} DESC")
                } else {
                    name.clone()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_index_flags(idx: &IndexInfo, accent: Hsla, cx: &App) -> AnyElement {
    let mut flag_labels = Vec::new();
    if idx.primary {
        flag_labels.push(("PRIMARY", true));
    }
    if idx.unique {
        flag_labels.push(("UNIQUE", false));
    }
    if flag_labels.is_empty() {
        flag_labels.push(("INDEX", false));
    }

    let mut flags_row = div()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(px(4.0));
    for (label, is_primary) in flag_labels {
        flags_row = flags_row.child(
            div()
                .flex_shrink_0()
                .px(px(5.0))
                .py(px(1.0))
                .rounded(px(3.0))
                .bg(if is_primary {
                    with_alpha(accent, 0.14)
                } else {
                    cx.colors().surface_secondary
                })
                .text_size(px(theme::TEXT_BADGE_PX))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if is_primary {
                    accent
                } else {
                    cx.colors().muted
                })
                .child(label),
        );
    }
    flags_row.into_any_element()
}

fn render_index_grid_row(
    row_idx: usize,
    idx: &IndexInfo,
    is_selected: bool,
    font: &Font,
    border: Hsla,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let accent = cx.colors().accent.color;
    let row_bg = if is_selected {
        Some(with_alpha(accent, 0.10))
    } else {
        theme::row_zebra_bg(row_idx, cx)
    };
    let keys_str = format_index_keys(idx);
    let flags_row = render_index_flags(idx, accent, cx);

    div()
        .id(("struct-idx-row", row_idx))
        .w_full()
        .h(px(GRID_ROW_HEIGHT_PX))
        .flex_shrink_0()
        .border_b_1()
        .border_color(border)
        .when_some(row_bg, |el, bg| el.bg(bg))
        .hover(move |style| {
            if is_selected {
                style
            } else {
                style.bg(with_alpha(accent, 0.05))
            }
        })
        .cursor_pointer()
        .flex()
        .items_center()
        .overflow_hidden()
        .on_click(cx.listener(move |ws, _ev, _w, cx| {
            ws.structure_ui.selected_index = if ws.structure_ui.selected_index == Some(row_idx) {
                None
            } else {
                Some(row_idx)
            };
            cx.notify();
        }))
        .child(render_grid_index_cell(
            row_idx + 1,
            is_selected,
            accent,
            border,
            cx,
        ))
        .child(
            grid_cell_box(Some(210.0), None, Some(border)).child(
                grid_ellipsis_text(idx.name.clone())
                    .font(font.clone())
                    .text_size(px(theme::TEXT_CONTROL_PX))
                    .font_weight(if is_selected {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(if is_selected {
                        accent
                    } else {
                        cx.colors().foreground
                    }),
            ),
        )
        .child(
            grid_cell_box(Some(260.0), None, Some(border)).child(
                grid_ellipsis_text(keys_str)
                    .font(font.clone())
                    .text_size(px(theme::TEXT_META_PX))
                    .text_color(cx.colors().foreground),
            ),
        )
        .child(grid_cell_box(Some(150.0), None, Some(border)).child(flags_row))
        .child(
            grid_cell_box(None, Some(160.0), None).child(render_grid_mono_or_dash(
                idx.predicate.as_deref().unwrap_or(""),
                font,
                cx.colors().muted,
                cx.colors().muted,
            )),
        )
        .into_any_element()
}

struct FkDisplayRow {
    constraint: String,
    is_outbound: bool,
    direction_label: &'static str,
    relationship: String,
    actions: String,
}

fn render_foreign_keys_grid(
    ws: &TableMinusWorkspace,
    def: Option<&TableDefinition>,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let border = theme::hairline_border_color(cx);
    let hdr_bg = ws.content_palette.data_grid_header_background;
    let hdr_fg = ws.content_palette.data_grid_header_text;

    let header_row = div()
        .w_full()
        .h(px(GRID_HEADER_HEIGHT_PX))
        .flex_shrink_0()
        .bg(hdr_bg)
        .border_b_1()
        .border_color(border)
        .flex()
        .items_center()
        .overflow_hidden()
        .child(
            grid_cell_box(Some(44.0), None, Some(border))
                .justify_end()
                .child(
                    div()
                        .text_size(px(theme::TEXT_BADGE_PX))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(cx.colors().muted)
                        .child("#"),
                ),
        )
        .child(render_static_header_cell(
            s.struct_fk_constraint,
            Some(200.0),
            None,
            Some(border),
            hdr_fg,
        ))
        .child(render_static_header_cell(
            s.struct_fk_direction,
            Some(110.0),
            None,
            Some(border),
            hdr_fg,
        ))
        .child(render_static_header_cell(
            s.struct_fk_relationship,
            None,
            Some(260.0),
            Some(border),
            hdr_fg,
        ))
        .child(render_static_header_cell(
            s.struct_fk_actions,
            Some(180.0),
            None,
            None,
            hdr_fg,
        ))
        .into_any_element();

    let query = ws
        .structure_ui
        .column_search_query
        .trim()
        .to_ascii_lowercase();
    let fk_rows = collect_fk_display_rows(def, s, &query);

    let body_rows = if fk_rows.is_empty() {
        div()
            .w_full()
            .p(px(24.0))
            .text_size(px(theme::TEXT_META_PX))
            .text_color(cx.colors().muted)
            .child(s.structure_no_fks_empty)
            .into_any_element()
    } else {
        let font = theme::value_gpui_font(&ws.controller.settings);
        let selected_fk = ws.structure_ui.selected_fk;
        let mut rows_col = div().w_full().flex().flex_col();
        for (row_idx, row) in fk_rows.into_iter().enumerate() {
            let is_selected = selected_fk == Some(row_idx);
            rows_col = rows_col.child(render_fk_grid_row(
                row_idx,
                row,
                is_selected,
                &font,
                border,
                cx,
            ));
        }
        rows_col.into_any_element()
    };

    render_grid_shell("structure-fks-scroll", header_row, body_rows, border)
}

fn collect_fk_display_rows(
    def: Option<&TableDefinition>,
    strings: &crate::i18n::Strings,
    query: &str,
) -> Vec<FkDisplayRow> {
    let Some(d) = def else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for fk in &d.foreign_keys {
        if !outbound_fk_matches_query(fk, query) {
            continue;
        }
        let target_table = match &fk.ref_schema {
            Some(schema) => format!("{schema}.{}", fk.ref_table),
            None => fk.ref_table.clone(),
        };
        let relationship = format!(
            "({}) -> {}({})",
            fk.columns.join(", "),
            target_table,
            fk.ref_columns.join(", ")
        );
        let mut actions = Vec::new();
        if let Some(del) = fk.on_delete {
            actions.push(format!("DEL: {del:?}"));
        }
        if let Some(upd) = fk.on_update {
            actions.push(format!("UPD: {upd:?}"));
        }
        let actions_text = if actions.is_empty() {
            "NO ACTION".to_owned()
        } else {
            actions.join(" · ")
        };
        rows.push(FkDisplayRow {
            constraint: fk.name.clone(),
            is_outbound: true,
            direction_label: strings.fk_outbound,
            relationship,
            actions: actions_text,
        });
    }
    for in_fk in &d.inbound_foreign_keys {
        if !inbound_fk_matches_query(in_fk, query) {
            continue;
        }
        let src_table = match &in_fk.table.schema {
            Some(schema) => format!("{schema}.{}", in_fk.table.name),
            None => in_fk.table.name.clone(),
        };
        let relationship = format!(
            "{src_table}({}) -> ({})",
            in_fk.columns.join(", "),
            in_fk.referenced_columns.join(", ")
        );
        rows.push(FkDisplayRow {
            constraint: in_fk.name.clone(),
            is_outbound: false,
            direction_label: strings.fk_inbound,
            relationship,
            actions: strings.fk_references_this_table.to_owned(),
        });
    }
    rows
}

fn outbound_fk_matches_query(fk: &ForeignKeyInfo, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    fk.name.to_ascii_lowercase().contains(query)
        || fk.ref_table.to_ascii_lowercase().contains(query)
        || fk
            .columns
            .iter()
            .any(|c| c.to_ascii_lowercase().contains(query))
        || fk
            .ref_columns
            .iter()
            .any(|c| c.to_ascii_lowercase().contains(query))
}

fn inbound_fk_matches_query(in_fk: &InboundForeignKey, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    in_fk.name.to_ascii_lowercase().contains(query)
        || in_fk.table.name.to_ascii_lowercase().contains(query)
        || in_fk
            .columns
            .iter()
            .any(|c| c.to_ascii_lowercase().contains(query))
        || in_fk
            .referenced_columns
            .iter()
            .any(|c| c.to_ascii_lowercase().contains(query))
}

fn render_fk_grid_row(
    row_idx: usize,
    row: FkDisplayRow,
    is_selected: bool,
    font: &Font,
    border: Hsla,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let accent = cx.colors().accent.color;
    let row_bg = if is_selected {
        Some(with_alpha(accent, 0.10))
    } else {
        theme::row_zebra_bg(row_idx, cx)
    };

    div()
        .id(("struct-fk-row", row_idx))
        .w_full()
        .h(px(GRID_ROW_HEIGHT_PX))
        .flex_shrink_0()
        .border_b_1()
        .border_color(border)
        .when_some(row_bg, |el, bg| el.bg(bg))
        .hover(move |style| {
            if is_selected {
                style
            } else {
                style.bg(with_alpha(accent, 0.05))
            }
        })
        .cursor_pointer()
        .flex()
        .items_center()
        .overflow_hidden()
        .on_click(cx.listener(move |ws, _ev, _w, cx| {
            ws.structure_ui.selected_fk = if ws.structure_ui.selected_fk == Some(row_idx) {
                None
            } else {
                Some(row_idx)
            };
            cx.notify();
        }))
        .child(
            grid_cell_box(Some(44.0), None, Some(border))
                .justify_end()
                .font_features(tnum_features())
                .text_size(px(theme::TEXT_META_PX))
                .text_color(if is_selected {
                    accent
                } else {
                    cx.colors().muted
                })
                .child(format!("{}", row_idx + 1)),
        )
        .child(
            grid_cell_box(Some(200.0), None, Some(border)).child(
                grid_ellipsis_text(row.constraint)
                    .font(font.clone())
                    .text_size(px(theme::TEXT_CONTROL_PX))
                    .font_weight(if is_selected {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(if is_selected {
                        accent
                    } else {
                        cx.colors().foreground
                    }),
            ),
        )
        .child(
            grid_cell_box(Some(110.0), None, Some(border)).child(
                div()
                    .flex_shrink_0()
                    .px(px(5.0))
                    .py(px(1.0))
                    .rounded(px(3.0))
                    .bg(if row.is_outbound {
                        with_alpha(accent, 0.14)
                    } else {
                        cx.colors().surface_secondary
                    })
                    .text_size(px(theme::TEXT_BADGE_PX))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if row.is_outbound {
                        accent
                    } else {
                        cx.colors().muted
                    })
                    .child(row.direction_label),
            ),
        )
        .child(
            grid_cell_box(None, Some(260.0), Some(border)).child(
                grid_ellipsis_text(row.relationship)
                    .font(font.clone())
                    .text_size(px(theme::TEXT_META_PX))
                    .text_color(cx.colors().foreground),
            ),
        )
        .child(
            grid_cell_box(Some(180.0), None, None).child(
                grid_ellipsis_text(row.actions)
                    .text_size(px(theme::TEXT_BADGE_PX))
                    .text_color(cx.colors().muted),
            ),
        )
        .into_any_element()
}

fn render_ddl_and_triggers(
    ws: &TableMinusWorkspace,
    table: &TableRef,
    def: Option<&TableDefinition>,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let grammar = ws.active_grammar();
    let editor_font = theme::editor_gpui_font(&ws.controller.settings);
    let border = theme::hairline_border_color(cx);
    let trigger_deps = collect_table_triggers(ws, table, def);
    let ddl_text = ws.structure_ui.ddl_text.clone().unwrap_or_default();

    div()
        .id("structure-ddl-scroll")
        .size_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(render_ddl_card(
            ws,
            ddl_text,
            grammar,
            &editor_font,
            border,
            cx,
        ))
        .child(render_triggers_card(
            ws,
            trigger_deps,
            grammar,
            &editor_font,
            border,
            cx,
        ))
        .into_any_element()
}

fn collect_table_triggers(
    ws: &TableMinusWorkspace,
    table: &TableRef,
    def: Option<&TableDefinition>,
) -> Vec<(String, Option<String>)> {
    let mut items = Vec::new();
    if let Some(d) = def
        && let Some(src) = &d.source
    {
        for dep in &src.dependents {
            if dep.kind == SchemaObjectKind::Trigger {
                items.push((dep.name.clone(), Some(dep.sql.clone())));
            }
        }
    }
    for obj in &ws.controller.cached_objects {
        if obj.kind == ObjectKind::Trigger
            && obj.parent_table.as_deref() == Some(table.name.as_str())
            && !items.iter().any(|(n, _)| n == &obj.name)
        {
            items.push((obj.name.clone(), None));
        }
    }
    items
}

fn render_ddl_card(
    ws: &TableMinusWorkspace,
    ddl_text: String,
    grammar: tablepro_core::sql_syntax::SqlGrammar,
    editor_font: &Font,
    border: Hsla,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    if ddl_text.is_empty() {
        return div()
            .p(px(16.0))
            .rounded(px(theme::CONTROL_RADIUS_PX))
            .border_1()
            .border_color(border)
            .text_size(px(theme::TEXT_META_PX))
            .text_color(cx.colors().muted)
            .child(s.structure_no_ddl_empty)
            .into_any_element();
    }

    let ddl_for_copy = ddl_text.clone();
    div()
        .rounded(px(theme::CONTROL_RADIUS_PX))
        .border_1()
        .border_color(border)
        .bg(ws.content_palette.editor_background)
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(GRID_HEADER_HEIGHT_PX))
                .px(px(12.0))
                .bg(ws.content_palette.data_grid_header_background)
                .border_b_1()
                .border_color(border)
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(theme::TEXT_META_PX))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(ws.content_palette.data_grid_header_text)
                        .child(s.structure_table_ddl),
                )
                .child(
                    Button::new("structure-copy-ddl")
                        .variant(Variant::Tertiary)
                        .size(HeroSize::Sm)
                        .child(Icon::new(IconName::Copy).size(IconSize::Xs))
                        .label(s.copy_ddl)
                        .on_press(cx.listener(move |ws, _ev, _w, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(ddl_for_copy.clone()));
                            let msg = ws.controller.strings.ddl_copied_toast;
                            ws.notify_toast(ToastSeverity::Success, msg, None, cx);
                        })),
                ),
        )
        .child(
            div()
                .p(px(12.0))
                .font(editor_font.clone())
                .text_size(px(ws.controller.settings.editor_font.size.max(11.0)))
                .line_height(px(19.0))
                .text_color(ws.content_palette.editor_text)
                .child(syntax_bridge::styled_sql_block(
                    ddl_text,
                    grammar,
                    &ws.content_palette,
                )),
        )
        .into_any_element()
}

fn render_triggers_card(
    ws: &TableMinusWorkspace,
    trigger_deps: Vec<(String, Option<String>)>,
    grammar: tablepro_core::sql_syntax::SqlGrammar,
    editor_font: &Font,
    border: Hsla,
    cx: &App,
) -> AnyElement {
    let mut triggers_card = div()
        .rounded(px(theme::CONTROL_RADIUS_PX))
        .border_1()
        .border_color(border)
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(GRID_HEADER_HEIGHT_PX))
                .px(px(12.0))
                .bg(ws.content_palette.data_grid_header_background)
                .border_b_1()
                .border_color(border)
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(theme::TEXT_META_PX))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(ws.content_palette.data_grid_header_text)
                        .child(format!(
                            "{} ({})",
                            ws.controller.strings.triggers_label,
                            trigger_deps.len()
                        )),
                ),
        );

    if trigger_deps.is_empty() {
        triggers_card = triggers_card.child(
            div()
                .p(px(12.0))
                .text_size(px(theme::TEXT_META_PX))
                .text_color(cx.colors().muted)
                .child(ws.controller.strings.structure_no_triggers_empty),
        );
        return triggers_card.into_any_element();
    }

    for (name, sql_opt) in trigger_deps {
        let mut row = div()
            .p(px(12.0))
            .border_b_1()
            .border_color(border)
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        Chip::new()
                            .variant(ChipVariant::Soft)
                            .size(HeroSize::Sm)
                            .color(HeroColor::Accent)
                            .child(ChipLabel::new().child("TRIGGER")),
                    )
                    .child(
                        div()
                            .text_size(px(theme::TEXT_CONTROL_PX))
                            .font_weight(FontWeight::MEDIUM)
                            .child(name),
                    ),
            );
        if let Some(sql) = sql_opt {
            row = row.child(
                div()
                    .p(px(8.0))
                    .rounded(px(theme::CONTROL_RADIUS_PX))
                    .bg(ws.content_palette.editor_background)
                    .font(editor_font.clone())
                    .text_size(px(theme::TEXT_META_PX))
                    .text_color(ws.content_palette.editor_text)
                    .child(syntax_bridge::styled_sql_block(
                        sql,
                        grammar,
                        &ws.content_palette,
                    )),
            );
        }
        triggers_card = triggers_card.child(row);
    }
    triggers_card.into_any_element()
}

fn render_structure_skeleton(s: &crate::i18n::Strings, cx: &App) -> AnyElement {
    let mut stack = div()
        .size_full()
        .p(px(16.0))
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .text_size(px(theme::TEXT_META_PX))
                .text_color(cx.colors().muted)
                .child(s.structure_loading_metadata),
        );
    for idx in 0usize..6 {
        stack = stack.child(
            Skeleton::new()
                .id(("structure-skel", idx))
                .w_full()
                .max_w(px(560.0))
                .h(px(28.0)),
        );
    }
    stack.into_any_element()
}

fn render_no_table_selected(s: &crate::i18n::Strings, cx: &App) -> AnyElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .p(px(24.0))
        .child(
            Icon::new(IconName::Database)
                .size(IconSize::Lg)
                .color(cx.colors().muted),
        )
        .child(
            div()
                .text_size(px(theme::TEXT_TITLE_PX))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(cx.colors().foreground)
                .child(s.structure_no_table_title),
        )
        .child(
            div()
                .text_size(px(theme::TEXT_META_PX))
                .text_color(cx.colors().muted)
                .child(s.structure_no_table_desc),
        )
        .into_any_element()
}

fn format_column_default(default: &ColumnDefault) -> String {
    match default {
        ColumnDefault::None => String::new(),
        ColumnDefault::Literal(val) => format_core_value(val),
        ColumnDefault::Expression(expr) => expr.as_sql().to_owned(),
    }
}

fn tnum_features() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".to_owned(), 1)]))
}
