//! Table structure & metadata view powered by HeroGPUI `Table`, `Tabs`, and `Skeleton`.
//!
//! Displays real inspected columns, indexes, outbound & inbound foreign keys,
//! triggers, and syntax-highlighted `CREATE TABLE` DDL via `TableDefinition`
//! fetched non-blockingly from the active session.

use std::sync::Arc;

use herogpui::components::{
    Alert, Button, Chip, ChipLabel, ChipVariant, Icon, IconName, IconSize, Skeleton,
    SortDescriptor, SortDirection, TabItem, Table, TableColumn, TableRow, TableVariant, Tabs,
    TabsSize, TabsVariant,
};
use herogpui::core::{Color as HeroColor, SelectionMode, Size as HeroSize, Variant, with_alpha};
use herogpui::gpui::{
    AnyElement, App, ClipboardItem, Context, Entity, FontFeatures, FontWeight, InteractiveElement,
    IntoElement, MouseButton, ParentElement, SharedString, StatefulInteractiveElement, Styled,
    Window, div, px,
};
use herogpui::prelude::FluentBuilder;
use herogpui::theme::ActiveTheme;
use tablepro_core::column::ColumnDefault;
use tablepro_core::ddl::{SchemaObjectKind, TableDefinition};
use tablepro_core::foreign_key_info::ForeignKeyInfo;
use tablepro_core::meta::{IndexKey, ObjectKind, TableRef};
use tablepro_session::{OperationError, OperationKind};

use crate::views::content::StructureColumnRow;

use super::results_pane::format_core_value;
use super::shell_state::ToastSeverity;
use super::{DataForgeWorkspace, syntax_bridge, theme};

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

/// Persistent UI state for the Structure View pane.
#[derive(Debug, Default)]
pub struct StructureViewState {
    pub selected_column: Option<usize>,
    pub active_tab: StructureSubTab,
    pub loaded_table: Option<TableRef>,
    pub loading_table: Option<TableRef>,
    pub definition: Option<TableDefinition>,
    pub ddl_text: Option<String>,
    pub load_error: Option<String>,
    pub sort_descriptor: Option<SortDescriptor>,
}

impl StructureViewState {
    /// Creates initial Structure View state.
    pub fn new(_window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) -> Self {
        Self::default()
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
        ws: &mut DataForgeWorkspace,
        table: &TableRef,
        cx: &mut Context<DataForgeWorkspace>,
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
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
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
    ws: &mut DataForgeWorkspace,
    table: TableRef,
    force_refresh: bool,
    cx: &mut Context<DataForgeWorkspace>,
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
    workspace_id: dataforge_state::connection_window::WorkspaceId,
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
    ws: &mut DataForgeWorkspace,
    table: TableRef,
    result: Result<(TableDefinition, Option<String>), String>,
) {
    if ws.controller.active_table.as_ref() != Some(&table)
        && ws.structure_ui.loading_table.as_ref() != Some(&table)
    {
        return;
    }
    ws.structure_ui.loading_table = None;
    ws.structure_ui.loaded_table = Some(table);
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

fn columns_from_definition(def: &TableDefinition) -> Vec<StructureColumnRow> {
    def.columns
        .iter()
        .map(|col| {
            let default_expr = match &col.info.default {
                ColumnDefault::None => None,
                ColumnDefault::Literal(val) => Some(format_core_value(val)),
                ColumnDefault::Expression(expr) => Some(expr.as_sql().to_owned()),
            };
            StructureColumnRow {
                name: col.info.name.clone(),
                data_type: col.info.column_type.name.clone(),
                nullable: col.info.nullable,
                default_expr,
                is_primary_key: col.info.primary_key,
            }
        })
        .collect()
}

fn render_structure_header(
    ws: &DataForgeWorkspace,
    table: &TableRef,
    cx: &mut Context<DataForgeWorkspace>,
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

    let tabs = render_structure_subtabs(ws, def, cx);
    let is_loading = ws.structure_ui.loading_table.as_ref() == Some(table);
    let table_for_refresh = table.clone();

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
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            Icon::new(IconName::Database)
                                .size(IconSize::Sm)
                                .color(cx.colors().accent.color),
                        )
                        .child(
                            div()
                                .text_size(px(14.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(cx.colors().foreground)
                                .child(qualified),
                        )
                        .when_some(def.and_then(|d| d.options.as_ref()), |el, opts| {
                            el.when_some(opts.collation.as_ref(), |el, coll| {
                                el.child(
                                    div()
                                        .text_size(px(12.0))
                                        .text_color(cx.colors().muted)
                                        .truncate()
                                        .max_w(px(280.0))
                                        .child(coll.clone()),
                                )
                            })
                        }),
                )
                .child(
                    Button::new("structure-refresh-btn")
                        .variant(Variant::Secondary)
                        .size(HeroSize::Sm)
                        .is_disabled(is_loading)
                        .child(Icon::new(IconName::RotateCcw).size(IconSize::Xs))
                        .label(if is_loading {
                            ws.controller.strings.refreshing_label
                        } else {
                            ws.controller.strings.refresh_label
                        })
                        .on_press(cx.listener(move |ws, _ev, _w, cx| {
                            start_fetch_definition(ws, table_for_refresh.clone(), true, cx);
                            cx.notify();
                        })),
                ),
        )
        .child(tabs)
        .into_any_element()
}

fn render_structure_subtabs(
    ws: &DataForgeWorkspace,
    def: Option<&TableDefinition>,
    cx: &mut Context<DataForgeWorkspace>,
) -> Tabs {
    let col_count = def.map_or(ws.controller.structure_columns.len(), |d| d.columns.len());
    let idx_count = def.map_or(0, |d| d.indexes.len());
    let fk_count = def.map_or(0, |d| d.foreign_keys.len() + d.inbound_foreign_keys.len());
    let s = &ws.controller.strings;
    let items = vec![
        TabItem::new("columns", format!("{} ({col_count})", s.columns_button)),
        TabItem::new("indexes", format!("{} ({idx_count})", s.indexes_label)),
        TabItem::new("foreign-keys", format!("{} ({fk_count})", s.foreign_keys_label)),
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
    ws: &mut DataForgeWorkspace,
    table: &TableRef,
    cx: &mut Context<DataForgeWorkspace>,
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
        StructureSubTab::Columns => render_columns_table(ws, def, cx),
        StructureSubTab::Indexes => render_indexes_table(ws, def, cx),
        StructureSubTab::ForeignKeys => render_foreign_keys_table(ws, def, cx),
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
                .p(px(12.0))
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
    comment: String,
}

fn collect_structure_column_entries(
    ws: &DataForgeWorkspace,
    def: Option<&TableDefinition>,
) -> Vec<StructureColumnDisplay> {
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
                comment: String::new(),
            })
            .collect()
    };

    if let Some(desc) = &ws.structure_ui.sort_descriptor {
        let asc = desc.direction == SortDirection::Ascending;
        let s = &ws.controller.strings;
        entries.sort_by(|a, b| {
            let col: &str = desc.column.as_ref();
            let ord = if col == s.struct_col_column || col == "Column" {
                a.name.cmp(&b.name)
            } else if col == s.struct_col_type || col == "Type" {
                a.dtype.cmp(&b.dtype)
            } else if col == s.struct_col_nullable || col == "Nullable" {
                a.nullable.cmp(&b.nullable)
            } else {
                a.orig_idx.cmp(&b.orig_idx)
            };
            if asc { ord } else { ord.reverse() }
        });
    }
    entries
}

fn render_structure_checkbox_cell(
    orig_idx: usize,
    is_selected: bool,
    entity_sel: Entity<DataForgeWorkspace>,
    cx: &App,
) -> AnyElement {
    let colors = cx.colors();
    let accent = colors.accent.color;
    let accent_fg = colors.accent.foreground;
    let box_bg = if is_selected {
        accent
    } else {
        colors.field.background
    };
    let box_border = if is_selected {
        accent
    } else {
        theme::hairline_border_color(cx)
    };
    let box_hover_border = if is_selected {
        accent
    } else {
        with_alpha(accent, 0.55)
    };
    let box_hover_bg = if is_selected {
        accent
    } else {
        with_alpha(accent, 0.08)
    };

    div()
        .id(("struct-col-check", orig_idx))
        .size(px(15.0))
        .rounded(px(4.0))
        .border_1()
        .border_color(box_border)
        .bg(box_bg)
        .hover(move |s| s.border_color(box_hover_border).bg(box_hover_bg))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .on_click(move |_, _, cx| {
            entity_sel.update(cx, |ws, cx| {
                ws.structure_ui.selected_column =
                    if ws.structure_ui.selected_column == Some(orig_idx) {
                        None
                    } else {
                        Some(orig_idx)
                    };
                cx.notify();
            });
        })
        .when(is_selected, |el| {
            el.child(Icon::new(IconName::Check).size(px(11.0)).color(accent_fg))
        })
        .into_any_element()
}

fn build_structure_column_row(
    entry: StructureColumnDisplay,
    is_selected: bool,
    font: &herogpui::gpui::Font,
    entity_sel: Entity<DataForgeWorkspace>,
    s: &crate::i18n::Strings,
    cx: &App,
) -> TableRow {
    let orig_idx = entry.orig_idx;
    let select_cell = render_structure_checkbox_cell(orig_idx, is_selected, entity_sel, cx);
    let colors = cx.colors();
    let accent = colors.accent.color;

    let idx_cell = div()
        .font_features(tnum_features())
        .text_color(if is_selected { accent } else { colors.muted })
        .text_size(px(12.0))
        .child(format!("{}", entry.orig_idx + 1))
        .into_any_element();
    let name_cell = div()
        .font(font.clone())
        .text_size(px(12.0))
        .font_weight(if is_selected {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::MEDIUM
        })
        .when(is_selected, |el| el.text_color(accent))
        .truncate()
        .child(entry.name.clone())
        .into_any_element();
    let type_cell = div()
        .font(font.clone())
        .text_size(px(12.0))
        .text_color(cx.colors().accent.color)
        .truncate()
        .child(entry.dtype)
        .into_any_element();
    let null_cell = Chip::new()
        .variant(ChipVariant::Secondary)
        .size(HeroSize::Sm)
        .color(if entry.nullable {
            HeroColor::Default
        } else {
            HeroColor::Warning
        })
        .child(ChipLabel::new().child(if entry.nullable {
            s.nullable_yes
        } else {
            s.not_null_badge
        }))
        .into_any_element();
    let def_cell = render_column_default_cell(&entry.default_expr, font, cx);
    let attrs_row = render_column_attrs_row(entry.attrs, cx);
    let comment_cell = div()
        .text_size(px(12.0))
        .text_color(cx.colors().muted)
        .truncate()
        .child(if entry.comment.is_empty() {
            "—".to_owned()
        } else {
            entry.comment
        })
        .into_any_element();

    TableRow::new(vec![
        select_cell,
        idx_cell,
        name_cell,
        type_cell,
        null_cell,
        def_cell,
        attrs_row,
        comment_cell,
    ])
    .key(format!("{}", entry.orig_idx))
    .text_value(entry.name)
}

fn render_column_default_cell(
    default_expr: &str,
    font: &herogpui::gpui::Font,
    cx: &App,
) -> AnyElement {
    if default_expr.is_empty() {
        div()
            .text_size(px(12.0))
            .text_color(cx.colors().muted)
            .child("—")
            .into_any_element()
    } else {
        div()
            .font(font.clone())
            .text_size(px(12.0))
            .truncate()
            .child(default_expr.to_owned())
            .into_any_element()
    }
}

fn render_column_attrs_row(attrs: Vec<String>, cx: &App) -> AnyElement {
    let mut row = div().flex().items_center().gap(px(4.0));
    if attrs.is_empty() {
        row = row.child(
            div()
                .text_size(px(12.0))
                .text_color(cx.colors().muted)
                .child("—"),
        );
    } else {
        for attr in attrs {
            let color = if attr == "PK" {
                HeroColor::Accent
            } else {
                HeroColor::Default
            };
            row = row.child(
                Chip::new()
                    .variant(ChipVariant::Soft)
                    .size(HeroSize::Sm)
                    .color(color)
                    .child(ChipLabel::new().child(attr)),
            );
        }
    }
    row.into_any_element()
}

fn render_columns_table(
    ws: &DataForgeWorkspace,
    def: Option<&TableDefinition>,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let cols = vec![
        TableColumn::new("").width(px(34.0)).allows_resizing(false),
        TableColumn::new("#").width(px(44.0)).allows_resizing(false),
        TableColumn::new(s.struct_col_column)
            .is_row_header(true)
            .allows_sorting(true)
            .allows_resizing(true)
            .default_width(px(180.0))
            .min_width(px(100.0)),
        TableColumn::new(s.struct_col_type)
            .allows_sorting(true)
            .allows_resizing(true)
            .default_width(px(150.0)),
        TableColumn::new(s.struct_col_nullable)
            .allows_sorting(true)
            .default_width(px(90.0)),
        TableColumn::new(s.struct_col_default)
            .allows_resizing(true)
            .default_width(px(160.0)),
        TableColumn::new(s.struct_col_attributes).default_width(px(160.0)),
        TableColumn::new(s.struct_col_comment)
            .allows_resizing(true)
            .default_width(px(200.0)),
    ];

    let entries = collect_structure_column_entries(ws, def);
    let entry_orig_indices: Vec<usize> = entries.iter().map(|e| e.orig_idx).collect();
    let selected_column = ws.structure_ui.selected_column;
    let font = theme::value_gpui_font(&ws.controller.settings);
    let entity_sel = cx.entity().clone();
    let entity_sort = cx.entity().clone();
    let rows: Vec<TableRow> = entries
        .into_iter()
        .map(|entry| {
            let is_selected = selected_column == Some(entry.orig_idx);
            build_structure_column_row(entry, is_selected, &font, entity_sel.clone(), s, cx)
        })
        .collect();

    let mut table = Table::new(Vec::new())
        .id("structure-columns-table")
        .variant(TableVariant::Primary)
        .columns(cols)
        .row_height(px(34.0))
        .selection_mode(SelectionMode::None)
        .on_row_click(move |row_idx, _ev, _w, cx| {
            let Some(orig_idx) = entry_orig_indices.get(*row_idx).copied() else {
                return;
            };
            entity_sel.update(cx, |ws, cx| {
                ws.structure_ui.selected_column =
                    if ws.structure_ui.selected_column == Some(orig_idx) {
                        None
                    } else {
                        Some(orig_idx)
                    };
                cx.notify();
            });
        })
        .on_sort_change(move |desc, _w, cx| {
            let next = desc.clone();
            entity_sort.update(cx, |ws, cx| {
                ws.structure_ui.sort_descriptor = Some(next);
                cx.notify();
            });
        })
        .empty_state(
            div()
                .p(px(24.0))
                .text_size(px(12.0))
                .text_color(cx.colors().muted)
                .child(s.structure_no_columns_empty),
        );

    if let Some(sort) = &ws.structure_ui.sort_descriptor {
        table = table.sort_descriptor(sort.clone());
    }
    for row in rows {
        table = table.tree_row(row);
    }
    table.into_any_element()
}

fn render_indexes_table(
    ws: &DataForgeWorkspace,
    def: Option<&TableDefinition>,
    cx: &App,
) -> AnyElement {
    let s = &ws.controller.strings;
    let cols = vec![
        TableColumn::new(s.struct_idx_name)
            .is_row_header(true)
            .allows_resizing(true)
            .default_width(px(200.0)),
        TableColumn::new(s.struct_idx_keys)
            .allows_resizing(true)
            .default_width(px(260.0)),
        TableColumn::new(s.struct_idx_flags).default_width(px(140.0)),
        TableColumn::new(s.struct_idx_predicate).default_width(px(220.0)),
    ];
    let font = theme::value_gpui_font(&ws.controller.settings);

    let mut table = Table::new(Vec::new())
        .id("structure-indexes-table")
        .variant(TableVariant::Primary)
        .columns(cols)
        .row_height(px(32.0))
        .empty_state(
            div()
                .p(px(24.0))
                .text_size(px(12.0))
                .text_color(cx.colors().muted)
                .child(s.structure_no_indexes_empty),
        );

    if let Some(d) = def {
        for idx in &d.indexes {
            table = table.tree_row(build_index_table_row(idx, &font, cx));
        }
    }

    table.into_any_element()
}

fn build_index_table_row(
    idx: &tablepro_core::meta::IndexInfo,
    font: &herogpui::gpui::Font,
    cx: &App,
) -> TableRow {
    let keys_str = idx
        .keys
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
        .join(", ");

    let mut flags = div().flex().items_center().gap(px(4.0));
    if idx.primary {
        flags = flags.child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .size(HeroSize::Sm)
                .color(HeroColor::Accent)
                .child(ChipLabel::new().child("PRIMARY")),
        );
    }
    if idx.unique {
        flags = flags.child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .size(HeroSize::Sm)
                .color(HeroColor::Success)
                .child(ChipLabel::new().child("UNIQUE")),
        );
    }
    if !idx.primary && !idx.unique {
        flags = flags.child(
            Chip::new()
                .variant(ChipVariant::Secondary)
                .size(HeroSize::Sm)
                .child(ChipLabel::new().child("INDEX")),
        );
    }

    TableRow::new(vec![
        div()
            .font(font.clone())
            .text_size(px(12.0))
            .font_weight(FontWeight::MEDIUM)
            .child(idx.name.clone())
            .into_any_element(),
        div()
            .font(font.clone())
            .text_size(px(12.0))
            .truncate()
            .child(keys_str)
            .into_any_element(),
        flags.into_any_element(),
        div()
            .font(font.clone())
            .text_size(px(12.0))
            .text_color(cx.colors().muted)
            .truncate()
            .child(idx.predicate.clone().unwrap_or_else(|| "—".to_owned()))
            .into_any_element(),
    ])
    .key(idx.name.clone())
}

fn render_foreign_keys_table(
    ws: &DataForgeWorkspace,
    def: Option<&TableDefinition>,
    cx: &App,
) -> AnyElement {
    let s = &ws.controller.strings;
    let cols = vec![
        TableColumn::new(s.struct_fk_constraint)
            .is_row_header(true)
            .allows_resizing(true)
            .default_width(px(190.0)),
        TableColumn::new(s.struct_fk_direction).default_width(px(110.0)),
        TableColumn::new(s.struct_fk_relationship).default_width(px(280.0)),
        TableColumn::new(s.struct_fk_actions).default_width(px(170.0)),
    ];
    let font = theme::value_gpui_font(&ws.controller.settings);

    let mut table = Table::new(Vec::new())
        .id("structure-fks-table")
        .variant(TableVariant::Primary)
        .columns(cols)
        .row_height(px(32.0))
        .empty_state(
            div()
                .p(px(24.0))
                .text_size(px(12.0))
                .text_color(cx.colors().muted)
                .child(s.structure_no_fks_empty),
        );

    if let Some(d) = def {
        for fk in &d.foreign_keys {
            table = table.tree_row(build_outbound_fk_row(fk, &font, s, cx));
        }
        for in_fk in &d.inbound_foreign_keys {
            table = table.tree_row(build_inbound_fk_row(in_fk, &font, s, cx));
        }
    }

    table.into_any_element()
}

fn build_outbound_fk_row(
    fk: &ForeignKeyInfo,
    font: &herogpui::gpui::Font,
    strings: &crate::i18n::Strings,
    cx: &App,
) -> TableRow {
    let target_table = match &fk.ref_schema {
        Some(s) => format!("{s}.{}", fk.ref_table),
        None => fk.ref_table.clone(),
    };
    let rel = format!(
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
    let act_text = if actions.is_empty() {
        "NO ACTION".to_owned()
    } else {
        actions.join(" · ")
    };

    TableRow::new(vec![
        div()
            .font(font.clone())
            .text_size(px(12.0))
            .font_weight(FontWeight::MEDIUM)
            .child(fk.name.clone())
            .into_any_element(),
        Chip::new()
            .variant(ChipVariant::Soft)
            .size(HeroSize::Sm)
            .color(HeroColor::Accent)
            .child(ChipLabel::new().child(strings.fk_outbound))
            .into_any_element(),
        div()
            .font(font.clone())
            .text_size(px(12.0))
            .truncate()
            .child(rel)
            .into_any_element(),
        div()
            .text_size(px(11.0))
            .text_color(cx.colors().muted)
            .child(act_text)
            .into_any_element(),
    ])
    .key(format!("out-{}", fk.name))
}

fn build_inbound_fk_row(
    in_fk: &tablepro_core::ddl::InboundForeignKey,
    font: &herogpui::gpui::Font,
    strings: &crate::i18n::Strings,
    cx: &App,
) -> TableRow {
    let src_table = match &in_fk.table.schema {
        Some(s) => format!("{s}.{}", in_fk.table.name),
        None => in_fk.table.name.clone(),
    };
    let rel = format!(
        "{src_table}({}) -> ({})",
        in_fk.columns.join(", "),
        in_fk.referenced_columns.join(", ")
    );
    TableRow::new(vec![
        div()
            .font(font.clone())
            .text_size(px(12.0))
            .font_weight(FontWeight::MEDIUM)
            .child(in_fk.name.clone())
            .into_any_element(),
        Chip::new()
            .variant(ChipVariant::Secondary)
            .size(HeroSize::Sm)
            .child(ChipLabel::new().child(strings.fk_inbound))
            .into_any_element(),
        div()
            .font(font.clone())
            .text_size(px(12.0))
            .truncate()
            .child(rel)
            .into_any_element(),
        div()
            .text_size(px(11.0))
            .text_color(cx.colors().muted)
            .child(strings.fk_references_this_table)
            .into_any_element(),
    ])
    .key(format!("in-{}", in_fk.name))
}

fn render_ddl_and_triggers(
    ws: &DataForgeWorkspace,
    table: &TableRef,
    def: Option<&TableDefinition>,
    cx: &mut Context<DataForgeWorkspace>,
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
    ws: &DataForgeWorkspace,
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
    ws: &DataForgeWorkspace,
    ddl_text: String,
    grammar: tablepro_core::sql_syntax::SqlGrammar,
    editor_font: &herogpui::gpui::Font,
    border: herogpui::gpui::Hsla,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    if ddl_text.is_empty() {
        return div()
            .p(px(16.0))
            .rounded(px(theme::PANEL_RADIUS_PX))
            .border_1()
            .border_color(border)
            .text_size(px(12.0))
            .text_color(cx.colors().muted)
            .child(s.structure_no_ddl_empty)
            .into_any_element();
    }

    let ddl_for_copy = ddl_text.clone();
    div()
        .rounded(px(theme::PANEL_RADIUS_PX))
        .border_1()
        .border_color(border)
        .bg(ws.content_palette.editor_background)
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(34.0))
                .px(px(12.0))
                .bg(theme::muted_surface_bg(cx))
                .border_b_1()
                .border_color(border)
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
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
    ws: &DataForgeWorkspace,
    trigger_deps: Vec<(String, Option<String>)>,
    grammar: tablepro_core::sql_syntax::SqlGrammar,
    editor_font: &herogpui::gpui::Font,
    border: herogpui::gpui::Hsla,
    cx: &App,
) -> AnyElement {
    let mut triggers_card = div()
        .rounded(px(theme::PANEL_RADIUS_PX))
        .border_1()
        .border_color(border)
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(32.0))
                .px(px(12.0))
                .bg(theme::muted_surface_bg(cx))
                .border_b_1()
                .border_color(border)
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
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
                .text_size(px(12.0))
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
                            .text_size(px(12.0))
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
                    .text_size(px(12.0))
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
                .text_size(px(12.0))
                .text_color(cx.colors().muted)
                .child(s.structure_loading_metadata),
        );
    for idx in 0usize..6 {
        stack = stack.child(
            Skeleton::new()
                .id(("structure-skel", idx))
                .w(px(560.0))
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
                .text_size(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(cx.colors().foreground)
                .child(s.structure_no_table_title),
        )
        .child(
            div()
                .text_size(px(12.0))
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
