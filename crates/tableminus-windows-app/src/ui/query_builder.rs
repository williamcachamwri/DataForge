//! Visual Query Builder workbench view for composing SELECT/JOIN/WHERE SQL visually.

use std::collections::BTreeMap;

use tableminus_services::{
    JoinClause, JoinKind, VisualQuerySpec, WhereClause, build_visual_query_sql,
};
use herogpui::components::{
    Alert, Button, Checkbox, Chip, ChipLabel, ChipVariant, FieldVariant, Icon, IconName, IconSize,
    Input, InputState, InputType, PickerItem, Select, Separator, Variant,
};
use herogpui::core::{Color as HeroColor, Size as HeroSize};
use herogpui::gpui::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, FontWeight,
    InteractiveElement, IntoElement, ParentElement, SharedString, StatefulInteractiveElement,
    Styled, Window, div, px,
};
use herogpui::theme::ActiveTheme;
use tablepro_core::meta::ObjectInfo;

use super::shell_state::{WorkspaceSubView, push_error_toast, push_success_toast};
use super::{TableMinusWorkspace, syntax_bridge, theme};

const JOIN_KINDS: [(JoinKind, &str); 4] = [
    (JoinKind::Inner, "INNER"),
    (JoinKind::Left, "LEFT"),
    (JoinKind::Right, "RIGHT"),
    (JoinKind::Full, "FULL"),
];

const FILTER_OPS: [&str; 10] = [
    "=",
    "!=",
    "<",
    "<=",
    ">",
    ">=",
    "LIKE",
    "IN",
    "IS NULL",
    "IS NOT NULL",
];

#[derive(Debug, Clone)]
struct JoinDraft {
    id: u64,
    kind: JoinKind,
    table_key: String,
    left_col: String,
    right_col: String,
}

#[derive(Debug)]
struct WhereDraft {
    id: u64,
    logical_op: String,
    column: String,
    operator: String,
    value_input: Entity<InputState>,
}

#[derive(Debug, Clone)]
struct OrderDraft {
    id: u64,
    column: String,
    asc: bool,
}

/// Persistent UI state for the Visual Query Builder view.
#[derive(Debug)]
pub struct QueryBuilderState {
    pub limit: Option<u32>,
    base_table_key: Option<String>,
    selected_columns: Vec<String>,
    joins: Vec<JoinDraft>,
    filters: Vec<WhereDraft>,
    group_by: Vec<String>,
    order_by: Vec<OrderDraft>,
    limit_input: Entity<InputState>,
    table_columns: BTreeMap<String, Vec<String>>,
    load_error: Option<String>,
    next_id: u64,
}

impl QueryBuilderState {
    /// Creates initial Query Builder state owned by [`TableMinusWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, cx: &mut Context<TableMinusWorkspace>) -> Self {
        let limit_input = cx.new(|cx| InputState::with_value(cx, "100"));
        Self {
            limit: Some(100),
            base_table_key: None,
            selected_columns: Vec::new(),
            joins: Vec::new(),
            filters: Vec::new(),
            group_by: Vec::new(),
            order_by: Vec::new(),
            limit_input,
            table_columns: BTreeMap::new(),
            load_error: None,
            next_id: 1,
        }
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }
}

/// Renders the Visual Query Builder workbench tool view.
#[must_use]
pub fn render_query_builder(
    ws: &mut TableMinusWorkspace,
    _window: &mut Window,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    ensure_initial_table_selected(ws);
    let preview_sql = build_current_sql(ws, cx);

    div()
        .id("query-builder-view")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme::panel_surface_bg(cx))
        .child(render_header(ws, cx))
        .child(render_top_bar(ws, cx))
        .children(ws.query_builder_ui.load_error.as_ref().map(|err| {
            div()
                .px(px(14.0))
                .pt(px(8.0))
                .child(Alert::new(err.clone()).status(HeroColor::Danger))
                .into_any_element()
        }))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_row()
                .child(render_builder_panes(ws, cx))
                .child(render_sql_preview_pane(ws, preview_sql, cx)),
        )
        .into_any_element()
}

fn render_header(ws: &TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    div()
        .h(px(42.0))
        .px(px(14.0))
        .border_b_1()
        .border_color(theme::hairline_border_color(cx))
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    Icon::new(IconName::SlidersHorizontal)
                        .size(IconSize::Sm)
                        .color(colors.accent.color),
                )
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(s.query_builder),
                ),
        )
        .child(
            Button::new("query-builder-close")
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .is_icon_only(true)
                .aria_label(s.close_label)
                .on_press(cx.listener(|ws, _, _, cx| {
                    ws.workspace_pane.close_tool();
                    cx.notify();
                }))
                .child(Icon::new(IconName::X).size(IconSize::Xs)),
        )
        .into_any_element()
}

fn render_top_bar(ws: &TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) -> AnyElement {
    let s = &ws.controller.strings;
    let tables = browsable_tables(&ws.controller.cached_objects);
    let table_items: Vec<PickerItem> = tables
        .iter()
        .map(|o| {
            let key = encode_table_key(o.schema.as_deref(), &o.name);
            PickerItem::new(key.clone(), key)
        })
        .collect();
    let current = ws.query_builder_ui.base_table_key.clone();

    div()
        .px(px(14.0))
        .py(px(8.0))
        .border_b_1()
        .border_color(theme::hairline_border_color(cx))
        .bg(theme::muted_surface_bg(cx))
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(10.0))
                .child(
                    div().w(px(210.0)).min_w(px(140.0)).child(
                        Select::new("qb-base-table-select", table_items)
                            .variant(FieldVariant::Secondary)
                            .placeholder(s.qb_add_table)
                            .full_width(true)
                            .value(current.map(SharedString::from))
                            .on_selection_change(cx.listener(
                                |ws, key: &Option<SharedString>, _, cx| {
                                    if let Some(k) = key.as_ref() {
                                        select_base_table(ws, k.to_string());
                                        cx.notify();
                                    }
                                },
                            )),
                    ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(cx.colors().muted)
                                .child(s.qb_limit),
                        )
                        .child(
                            div().w(px(84.0)).child(
                                Input::new(&ws.query_builder_ui.limit_input)
                                    .input_type(InputType::Number)
                                    .variant(FieldVariant::Secondary)
                                    .placeholder("∞")
                                    .full_width()
                                    .on_change(cx.listener(|ws, val: &str, _, cx| {
                                        ws.query_builder_ui.limit = val.trim().parse::<u32>().ok();
                                        cx.notify();
                                    })),
                            ),
                        ),
                ),
        )
        .child(
            Button::new("qb-refresh-cols-btn")
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .child(Icon::new(IconName::RefreshCw).size(IconSize::Xs))
                .label(s.refresh_label)
                .on_press(cx.listener(|ws, _, _, cx| {
                    if let Some(key) = ws.query_builder_ui.base_table_key.clone() {
                        ws.query_builder_ui.table_columns.remove(&key);
                        ensure_table_columns(ws, &key);
                    }
                    cx.notify();
                })),
        )
        .into_any_element()
}

fn render_builder_panes(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let available_cols = collect_available_columns(&ws.query_builder_ui);

    div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .border_r_1()
        .border_color(theme::hairline_border_color(cx))
        .flex()
        .flex_col()
        .child(
            div()
                .id("qb-clauses-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(14.0))
                .flex()
                .flex_col()
                .gap(px(14.0))
                .child(render_columns_section(ws, &available_cols, cx))
                .child(Separator::new())
                .child(render_joins_section(ws, cx))
                .child(Separator::new())
                .child(render_where_section(ws, &available_cols, cx))
                .child(Separator::new())
                .child(render_group_by_section(ws, &available_cols, cx))
                .child(Separator::new())
                .child(render_order_by_section(ws, &available_cols, cx)),
        )
        .into_any_element()
}

fn render_columns_section(
    ws: &TableMinusWorkspace,
    available_cols: &[String],
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let mut chips = div().flex().flex_wrap().items_center().gap(px(6.0));
    for (idx, col) in available_cols.iter().enumerate() {
        let selected = ws.query_builder_ui.selected_columns.contains(col);
        let col_name = col.clone();
        chips = chips.child(
            Button::new(SharedString::from(format!("qb-col-chip-{idx}")))
                .variant(if selected {
                    Variant::Primary
                } else {
                    Variant::Secondary
                })
                .size(HeroSize::Sm)
                .label(col.clone())
                .on_press(cx.listener(move |ws, _, _, cx| {
                    toggle_vec_item(&mut ws.query_builder_ui.selected_columns, &col_name);
                    cx.notify();
                })),
        );
    }

    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(s.qb_columns),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            Button::new("qb-cols-all")
                                .variant(Variant::Ghost)
                                .size(HeroSize::Sm)
                                .label(s.show_all)
                                .on_press(cx.listener(|ws, _, _, cx| {
                                    ws.query_builder_ui.selected_columns =
                                        collect_available_columns(&ws.query_builder_ui);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("qb-cols-clear")
                                .variant(Variant::Ghost)
                                .size(HeroSize::Sm)
                                .label(s.clear)
                                .on_press(cx.listener(|ws, _, _, cx| {
                                    ws.query_builder_ui.selected_columns.clear();
                                    cx.notify();
                                })),
                        ),
                ),
        )
        .child(if available_cols.is_empty() {
            div()
                .text_size(px(11.0))
                .text_color(colors.muted)
                .child(s.structure_no_columns_empty)
                .into_any_element()
        } else {
            chips.into_any_element()
        })
        .into_any_element()
}

fn render_joins_section(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let table_items: Vec<PickerItem> = browsable_tables(&ws.controller.cached_objects)
        .into_iter()
        .map(|o| {
            let k = encode_table_key(o.schema.as_deref(), &o.name);
            PickerItem::new(k.clone(), k)
        })
        .collect();
    let kind_items: Vec<PickerItem> = JOIN_KINDS
        .iter()
        .map(|(_, label)| PickerItem::new(*label, *label))
        .collect();
    let col_items = column_picker_items(&collect_available_columns(&ws.query_builder_ui));

    let mut section = div().flex().flex_col().gap(px(8.0)).child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.foreground)
                    .child(format!(
                        "{} ({})",
                        s.qb_joins,
                        ws.query_builder_ui.joins.len()
                    )),
            )
            .child(
                Button::new("qb-add-join")
                    .variant(Variant::Secondary)
                    .size(HeroSize::Sm)
                    .is_disabled(table_items.is_empty())
                    .child(Icon::new(IconName::Plus).size(IconSize::Xs))
                    .label(s.qb_add_table)
                    .on_press(cx.listener(|ws, _, _, cx| {
                        add_default_join(ws);
                        cx.notify();
                    })),
            ),
    );

    for (idx, join) in ws.query_builder_ui.joins.iter().enumerate() {
        section = section.child(render_join_row(
            idx,
            join,
            &kind_items,
            &table_items,
            &col_items,
            cx,
        ));
    }
    section.into_any_element()
}

fn render_join_row(
    idx: usize,
    join: &JoinDraft,
    kind_items: &[PickerItem],
    table_items: &[PickerItem],
    col_items: &[PickerItem],
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let jid = join.id;
    let kind_label = join_kind_label(join.kind);
    div()
        .p(px(8.0))
        .rounded(px(theme::CONTROL_RADIUS_PX))
        .border_1()
        .border_color(theme::hairline_border_color(cx))
        .bg(theme::muted_surface_bg(cx))
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.0))
        .child(render_join_kind_select(jid, kind_label, kind_items, cx))
        .child(render_join_table_select(
            jid,
            &join.table_key,
            table_items,
            cx,
        ))
        .child(render_join_col_select(
            SharedString::from(format!("qb-join-lcol-{jid}")),
            &join.left_col,
            col_items,
            jid,
            true,
            cx,
        ))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(colors.muted)
                .child("="),
        )
        .child(render_join_col_select(
            SharedString::from(format!("qb-join-rcol-{jid}")),
            &join.right_col,
            col_items,
            jid,
            false,
            cx,
        ))
        .child(
            Button::new(SharedString::from(format!("qb-join-del-{jid}")))
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .on_press(cx.listener(move |ws, _, _, cx| {
                    if idx < ws.query_builder_ui.joins.len() {
                        ws.query_builder_ui.joins.remove(idx);
                    }
                    cx.notify();
                }))
                .child(Icon::new(IconName::Trash2).size(IconSize::Xs)),
        )
        .into_any_element()
}

fn render_join_kind_select(
    jid: u64,
    kind_label: &'static str,
    kind_items: &[PickerItem],
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    div()
        .w(px(95.0))
        .child(
            Select::new(
                SharedString::from(format!("qb-join-kind-{jid}")),
                kind_items.to_vec(),
            )
            .variant(FieldVariant::Secondary)
            .full_width(true)
            .value(Some(SharedString::from(kind_label)))
            .on_selection_change(cx.listener(
                move |ws, key: &Option<SharedString>, _, cx| {
                    if let Some(k) = key.as_ref()
                        && let Some(entry) =
                            ws.query_builder_ui.joins.iter_mut().find(|j| j.id == jid)
                    {
                        entry.kind = parse_join_kind(k.as_ref());
                        cx.notify();
                    }
                },
            )),
        )
        .into_any_element()
}

fn render_join_table_select(
    jid: u64,
    table_key: &str,
    table_items: &[PickerItem],
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    div()
        .w(px(150.0))
        .child(
            Select::new(
                SharedString::from(format!("qb-join-tbl-{jid}")),
                table_items.to_vec(),
            )
            .variant(FieldVariant::Secondary)
            .full_width(true)
            .value(Some(SharedString::from(table_key.to_owned())))
            .on_selection_change(cx.listener(
                move |ws, key: &Option<SharedString>, _, cx| {
                    if let Some(k) = key.as_ref() {
                        let tbl_key = k.to_string();
                        ensure_table_columns(ws, &tbl_key);
                        if let Some(entry) =
                            ws.query_builder_ui.joins.iter_mut().find(|j| j.id == jid)
                        {
                            entry.table_key = tbl_key;
                        }
                        cx.notify();
                    }
                },
            )),
        )
        .into_any_element()
}

fn render_join_col_select(
    id: SharedString,
    value: &str,
    col_items: &[PickerItem],
    jid: u64,
    is_left: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    div()
        .w(px(140.0))
        .child(
            Select::new(id, col_items.to_vec())
                .variant(FieldVariant::Secondary)
                .full_width(true)
                .value((!value.is_empty()).then(|| SharedString::from(value.to_owned())))
                .on_selection_change(cx.listener(move |ws, key: &Option<SharedString>, _, cx| {
                    if let Some(k) = key.as_ref()
                        && let Some(entry) =
                            ws.query_builder_ui.joins.iter_mut().find(|j| j.id == jid)
                    {
                        if is_left {
                            entry.left_col = k.to_string();
                        } else {
                            entry.right_col = k.to_string();
                        }
                        cx.notify();
                    }
                })),
        )
        .into_any_element()
}

fn render_where_section(
    ws: &TableMinusWorkspace,
    available_cols: &[String],
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let col_items = column_picker_items(available_cols);
    let op_items: Vec<PickerItem> = FILTER_OPS
        .iter()
        .map(|op| PickerItem::new(*op, *op))
        .collect();

    let mut section = div().flex().flex_col().gap(px(8.0)).child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.foreground)
                    .child(format!(
                        "{} ({})",
                        s.qb_where,
                        ws.query_builder_ui.filters.len()
                    )),
            )
            .child(
                Button::new("qb-add-where")
                    .variant(Variant::Secondary)
                    .size(HeroSize::Sm)
                    .child(Icon::new(IconName::Plus).size(IconSize::Xs))
                    .label(s.qb_where)
                    .on_press(cx.listener(|ws, _, _, cx| {
                        add_default_where(ws, cx);
                        cx.notify();
                    })),
            ),
    );

    for (idx, flt) in ws.query_builder_ui.filters.iter().enumerate() {
        section = section.child(render_where_row(ws, idx, flt, &col_items, &op_items, cx));
    }
    section.into_any_element()
}

fn render_where_row(
    ws: &TableMinusWorkspace,
    idx: usize,
    flt: &WhereDraft,
    col_items: &[PickerItem],
    op_items: &[PickerItem],
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let fid = flt.id;
    let needs_value = !matches!(flt.operator.as_str(), "IS NULL" | "IS NOT NULL");
    let mut row = div()
        .p(px(8.0))
        .rounded(px(theme::CONTROL_RADIUS_PX))
        .border_1()
        .border_color(theme::hairline_border_color(cx))
        .bg(theme::muted_surface_bg(cx))
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.0));
    if idx > 0 {
        let next_logic = if flt.logical_op == "OR" { "AND" } else { "OR" };
        row = row.child(
            Button::new(SharedString::from(format!("qb-where-logic-{fid}")))
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .label(flt.logical_op.clone())
                .on_press(cx.listener(move |ws, _, _, cx| {
                    if let Some(entry) =
                        ws.query_builder_ui.filters.iter_mut().find(|f| f.id == fid)
                    {
                        entry.logical_op = next_logic.to_owned();
                        cx.notify();
                    }
                })),
        );
    }
    row = row
        .child(render_where_picker(
            SharedString::from(format!("qb-where-col-{fid}")),
            150.0,
            (!flt.column.is_empty()).then_some(flt.column.as_str()),
            col_items,
            fid,
            true,
            cx,
        ))
        .child(render_where_picker(
            SharedString::from(format!("qb-where-op-{fid}")),
            115.0,
            Some(flt.operator.as_str()),
            op_items,
            fid,
            false,
            cx,
        ));
    if needs_value {
        row = row.child(
            div().flex_1().min_w(px(120.0)).child(
                Input::new(&flt.value_input)
                    .variant(FieldVariant::Secondary)
                    .placeholder(s.filter_value_placeholder)
                    .full_width()
                    .on_change(cx.listener(|_, _: &str, _, cx| cx.notify())),
            ),
        );
    }
    row.child(
        Button::new(SharedString::from(format!("qb-where-del-{fid}")))
            .variant(Variant::Ghost)
            .size(HeroSize::Sm)
            .on_press(cx.listener(move |ws, _, _, cx| {
                if idx < ws.query_builder_ui.filters.len() {
                    ws.query_builder_ui.filters.remove(idx);
                }
                cx.notify();
            }))
            .child(Icon::new(IconName::Trash2).size(IconSize::Xs)),
    )
    .into_any_element()
}

fn render_where_picker(
    id: SharedString,
    width_px: f32,
    value: Option<&str>,
    items: &[PickerItem],
    fid: u64,
    is_column: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    div()
        .w(px(width_px))
        .child(
            Select::new(id, items.to_vec())
                .variant(FieldVariant::Secondary)
                .full_width(true)
                .value(value.map(|v| SharedString::from(v.to_owned())))
                .on_selection_change(cx.listener(move |ws, key: &Option<SharedString>, _, cx| {
                    if let Some(k) = key.as_ref()
                        && let Some(entry) =
                            ws.query_builder_ui.filters.iter_mut().find(|f| f.id == fid)
                    {
                        if is_column {
                            entry.column = k.to_string();
                        } else {
                            entry.operator = k.to_string();
                        }
                        cx.notify();
                    }
                })),
        )
        .into_any_element()
}

fn render_group_by_section(
    ws: &TableMinusWorkspace,
    available_cols: &[String],
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let mut chips = div().flex().flex_wrap().items_center().gap(px(6.0));
    for (idx, col) in available_cols.iter().enumerate() {
        let selected = ws.query_builder_ui.group_by.contains(col);
        let col_name = col.clone();
        chips = chips.child(
            Checkbox::new(SharedString::from(format!("qb-grp-{idx}")))
                .is_selected(selected)
                .label(col.clone())
                .on_change(cx.listener(move |ws, &on: &bool, _, cx| {
                    set_vec_member(&mut ws.query_builder_ui.group_by, &col_name, on);
                    cx.notify();
                })),
        );
    }

    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.foreground)
                .child(s.qb_group_by),
        )
        .child(chips)
        .into_any_element()
}

fn render_order_by_section(
    ws: &TableMinusWorkspace,
    available_cols: &[String],
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let col_items = column_picker_items(available_cols);
    let mut section = div().flex().flex_col().gap(px(8.0)).child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.foreground)
                    .child(format!(
                        "{} ({})",
                        s.qb_order_by,
                        ws.query_builder_ui.order_by.len()
                    )),
            )
            .child(
                Button::new("qb-add-order")
                    .variant(Variant::Secondary)
                    .size(HeroSize::Sm)
                    .is_disabled(available_cols.is_empty())
                    .child(Icon::new(IconName::Plus).size(IconSize::Xs))
                    .label(s.qb_order_by)
                    .on_press(cx.listener(|ws, _, _, cx| {
                        add_default_order(ws);
                        cx.notify();
                    })),
            ),
    );

    for (idx, ord) in ws.query_builder_ui.order_by.iter().enumerate() {
        section = section.child(render_order_row(idx, ord, &col_items, cx));
    }
    section.into_any_element()
}

fn render_order_row(
    idx: usize,
    ord: &OrderDraft,
    col_items: &[PickerItem],
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let oid = ord.id;
    div()
        .p(px(8.0))
        .rounded(px(theme::CONTROL_RADIUS_PX))
        .border_1()
        .border_color(theme::hairline_border_color(cx))
        .bg(theme::muted_surface_bg(cx))
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(
            div().flex_1().child(
                Select::new(
                    SharedString::from(format!("qb-ord-col-{oid}")),
                    col_items.to_vec(),
                )
                .variant(FieldVariant::Secondary)
                .full_width(true)
                .value(Some(SharedString::from(ord.column.clone())))
                .on_selection_change(cx.listener(
                    move |ws, key: &Option<SharedString>, _, cx| {
                        if let Some(k) = key.as_ref()
                            && let Some(entry) = ws
                                .query_builder_ui
                                .order_by
                                .iter_mut()
                                .find(|o| o.id == oid)
                        {
                            entry.column = k.to_string();
                            cx.notify();
                        }
                    },
                )),
            ),
        )
        .child(
            Button::new(SharedString::from(format!("qb-ord-dir-{oid}")))
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .label(if ord.asc { "ASC" } else { "DESC" })
                .on_press(cx.listener(move |ws, _, _, cx| {
                    if let Some(entry) = ws
                        .query_builder_ui
                        .order_by
                        .iter_mut()
                        .find(|o| o.id == oid)
                    {
                        entry.asc = !entry.asc;
                        cx.notify();
                    }
                })),
        )
        .child(
            Button::new(SharedString::from(format!("qb-ord-del-{oid}")))
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .on_press(cx.listener(move |ws, _, _, cx| {
                    if idx < ws.query_builder_ui.order_by.len() {
                        ws.query_builder_ui.order_by.remove(idx);
                    }
                    cx.notify();
                }))
                .child(Icon::new(IconName::Trash2).size(IconSize::Xs)),
        )
        .into_any_element()
}

fn render_sql_preview_pane(
    ws: &TableMinusWorkspace,
    preview_sql: String,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let editor_font = theme::editor_gpui_font(&ws.controller.settings);
    let grammar = ws.active_grammar();
    let has_sql = !preview_sql.trim().is_empty();
    let sql_for_copy = preview_sql.clone();

    div()
        .w(px(340.0))
        .min_w(px(240.0))
        .max_w(px(380.0))
        .flex_shrink(1.0)
        .min_h_0()
        .p(px(14.0))
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(px(6.0))
                .child(
                    Chip::new()
                        .variant(ChipVariant::Soft)
                        .size(HeroSize::Sm)
                        .color(HeroColor::Accent)
                        .child(ChipLabel::new().child("SQL")),
                )
                .child(
                    Button::new("qb-copy-sql")
                        .variant(Variant::Tertiary)
                        .size(HeroSize::Sm)
                        .is_disabled(!has_sql)
                        .child(Icon::new(IconName::Copy).size(IconSize::Xs))
                        .label(s.copy_label)
                        .on_press(cx.listener(move |ws, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(sql_for_copy.clone()));
                            push_success_toast(ws.controller.strings.copied, None, cx);
                        })),
                ),
        )
        .child(
            div()
                .id("qb-sql-preview-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(12.0))
                .rounded(px(theme::PANEL_RADIUS_PX))
                .border_1()
                .border_color(theme::hairline_border_color(cx))
                .bg(ws.content_palette.editor_background)
                .font(editor_font)
                .text_size(px(ws.controller.settings.editor_font.size.max(11.0)))
                .line_height(px(19.0))
                .text_color(ws.content_palette.editor_text)
                .child(if has_sql {
                    syntax_bridge::styled_sql_block(
                        preview_sql.clone(),
                        grammar,
                        &ws.content_palette,
                    )
                    .into_any_element()
                } else {
                    div()
                        .text_color(cx.colors().muted)
                        .child(s.qb_select_table_hint)
                        .into_any_element()
                }),
        )
        .child(render_sql_preview_actions(ws, preview_sql, has_sql, cx))
        .into_any_element()
}

fn render_sql_preview_actions(
    ws: &TableMinusWorkspace,
    preview_sql: String,
    has_sql: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let sql_for_run = preview_sql.clone();
    let sql_for_open = preview_sql;
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.0))
        .child(
            Button::new("qb-run-query-btn")
                .variant(Variant::Primary)
                .size(HeroSize::Sm)
                .is_disabled(!has_sql)
                .child(Icon::new(IconName::Play).size(IconSize::Xs))
                .label(s.qb_run_query)
                .on_press(cx.listener(move |ws, _, _, cx| {
                    run_built_query(ws, &sql_for_run, cx);
                })),
        )
        .child(
            Button::new("qb-open-editor-btn")
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .is_disabled(!has_sql)
                .child(Icon::new(IconName::Code).size(IconSize::Xs))
                .label(s.qb_open_in_editor)
                .on_press(cx.listener(move |ws, _, _, cx| {
                    open_built_query_in_editor(ws, &sql_for_open, cx);
                })),
        )
        .into_any_element()
}

fn ensure_initial_table_selected(ws: &mut TableMinusWorkspace) {
    if ws.query_builder_ui.base_table_key.is_some() {
        return;
    }
    let initial = ws
        .controller
        .active_table
        .as_ref()
        .map(|t| encode_table_key(t.schema.as_deref(), &t.name))
        .or_else(|| {
            browsable_tables(&ws.controller.cached_objects)
                .first()
                .map(|o| encode_table_key(o.schema.as_deref(), &o.name))
        });
    if let Some(key) = initial {
        select_base_table(ws, key);
    }
}

fn select_base_table(ws: &mut TableMinusWorkspace, table_key: String) {
    ws.query_builder_ui.base_table_key = Some(table_key.clone());
    ws.query_builder_ui.selected_columns.clear();
    ws.query_builder_ui.group_by.clear();
    ws.query_builder_ui.order_by.clear();
    ws.query_builder_ui.filters.clear();
    ws.query_builder_ui.joins.clear();
    ensure_table_columns(ws, &table_key);
}

fn ensure_table_columns(ws: &mut TableMinusWorkspace, table_key: &str) {
    if ws.query_builder_ui.table_columns.contains_key(table_key) {
        return;
    }
    let (schema, table) = decode_table_key(table_key);
    match ws
        .controller
        .fetch_table_definition_blocking(schema.as_deref(), &table)
    {
        Ok(def) => {
            ws.query_builder_ui.load_error = None;
            let cols: Vec<String> = def.columns.iter().map(|c| c.name().to_owned()).collect();
            ws.query_builder_ui
                .table_columns
                .insert(table_key.to_owned(), cols);
        }
        Err(err) => {
            ws.query_builder_ui.load_error = Some(err.to_string());
        }
    }
}

fn collect_available_columns(state: &QueryBuilderState) -> Vec<String> {
    let Some(base_key) = state.base_table_key.as_ref() else {
        return Vec::new();
    };
    let (_, base_name) = decode_table_key(base_key);
    let qualify = !state.joins.is_empty();
    let mut out = Vec::new();
    if let Some(cols) = state.table_columns.get(base_key) {
        for c in cols {
            if qualify {
                out.push(format!("{base_name}.{c}"));
            } else {
                out.push(c.clone());
            }
        }
    }
    for join in &state.joins {
        let (_, j_name) = decode_table_key(&join.table_key);
        if let Some(cols) = state.table_columns.get(&join.table_key) {
            for c in cols {
                let candidate = format!("{j_name}.{c}");
                if !out.contains(&candidate) {
                    out.push(candidate);
                }
            }
        }
    }
    out
}

fn build_current_sql(ws: &TableMinusWorkspace, cx: &App) -> String {
    let state = &ws.query_builder_ui;
    let Some(base_key) = state.base_table_key.as_ref() else {
        return String::new();
    };
    let (schema, table) = decode_table_key(base_key);
    let joins = state
        .joins
        .iter()
        .map(|j| {
            let (_, target_table) = decode_table_key(&j.table_key);
            JoinClause {
                kind: j.kind,
                table: target_table,
                alias: None,
                left_col: j.left_col.clone(),
                right_col: j.right_col.clone(),
            }
        })
        .collect();
    let filters = state
        .filters
        .iter()
        .filter(|f| !f.column.trim().is_empty())
        .map(|f| WhereClause {
            column: f.column.clone(),
            operator: f.operator.clone(),
            value: f.value_input.read(cx).value().to_owned(),
            logical_op: f.logical_op.clone(),
        })
        .collect();
    let order_by = state
        .order_by
        .iter()
        .filter(|o| !o.column.trim().is_empty())
        .map(|o| (o.column.clone(), o.asc))
        .collect();

    let spec = VisualQuerySpec {
        table,
        schema,
        selected_columns: state.selected_columns.clone(),
        joins,
        filters,
        group_by: state.group_by.clone(),
        order_by,
        limit: state.limit.map(|n| n as usize),
    };
    build_visual_query_sql(ws.controller.active_database_engine(), &spec)
}

fn add_default_join(ws: &mut TableMinusWorkspace) {
    let tables = browsable_tables(&ws.controller.cached_objects);
    let Some(target) = tables.first() else {
        return;
    };
    let target_name = target.name.clone();
    let target_key = encode_table_key(target.schema.as_deref(), &target_name);
    ensure_table_columns(ws, &target_key);
    let cols = collect_available_columns(&ws.query_builder_ui);
    let left_col = cols.first().cloned().unwrap_or_default();
    let right_col = ws
        .query_builder_ui
        .table_columns
        .get(&target_key)
        .and_then(|c| c.first())
        .map(|c| format!("{target_name}.{c}"))
        .unwrap_or_default();
    let id = ws.query_builder_ui.alloc_id();
    ws.query_builder_ui.joins.push(JoinDraft {
        id,
        kind: JoinKind::Inner,
        table_key: target_key,
        left_col,
        right_col,
    });
}

fn add_default_where(ws: &mut TableMinusWorkspace, cx: &mut App) {
    let cols = collect_available_columns(&ws.query_builder_ui);
    let default_col = cols.first().cloned().unwrap_or_default();
    let id = ws.query_builder_ui.alloc_id();
    ws.query_builder_ui.filters.push(WhereDraft {
        id,
        logical_op: "AND".to_owned(),
        column: default_col,
        operator: "=".to_owned(),
        value_input: cx.new(|cx| InputState::new(cx)),
    });
}

fn add_default_order(ws: &mut TableMinusWorkspace) {
    let cols = collect_available_columns(&ws.query_builder_ui);
    let Some(col) = cols.first().cloned() else {
        return;
    };
    let id = ws.query_builder_ui.alloc_id();
    ws.query_builder_ui.order_by.push(OrderDraft {
        id,
        column: col,
        asc: true,
    });
}

fn run_built_query(ws: &mut TableMinusWorkspace, sql: &str, cx: &mut Context<TableMinusWorkspace>) {
    ws.controller.set_editor_sql(sql);
    match ws.controller.execute_sql_blocking(sql) {
        Ok(_) => {
            let row_count = ws.controller.grid_rows.len();
            ws.workspace_pane.close_tool();
            ws.workspace_pane.sub_view = WorkspaceSubView::DataGrid;
            push_success_toast(
                ws.controller.strings.qb_run_query,
                Some(SharedString::from(format!(
                    "{row_count} {}",
                    ws.controller.strings.rows_suffix
                ))),
                cx,
            );
        }
        Err(err) => {
            push_error_toast(
                ws.controller.strings.query_failed,
                Some(SharedString::from(err.to_string())),
                cx,
            );
        }
    }
    cx.notify();
}

fn open_built_query_in_editor(
    ws: &mut TableMinusWorkspace,
    sql: &str,
    cx: &mut Context<TableMinusWorkspace>,
) {
    ws.controller.set_editor_sql(sql);
    ws.workspace_pane.close_tool();
    ws.workspace_pane.sub_view = WorkspaceSubView::SqlEditor;
    push_success_toast(ws.controller.strings.query_ready_toast, None, cx);
    cx.notify();
}

fn browsable_tables(objects: &[ObjectInfo]) -> Vec<&ObjectInfo> {
    objects
        .iter()
        .filter(|obj| obj.kind.is_browsable())
        .collect()
}

fn column_picker_items(columns: &[String]) -> Vec<PickerItem> {
    columns
        .iter()
        .map(|c| PickerItem::new(c.clone(), c.clone()))
        .collect()
}

fn toggle_vec_item(list: &mut Vec<String>, item: &str) {
    if let Some(pos) = list.iter().position(|v| v == item) {
        list.remove(pos);
    } else {
        list.push(item.to_owned());
    }
}

fn set_vec_member(list: &mut Vec<String>, item: &str, present: bool) {
    if present {
        if !list.iter().any(|v| v == item) {
            list.push(item.to_owned());
        }
    } else {
        list.retain(|v| v != item);
    }
}

const fn join_kind_label(kind: JoinKind) -> &'static str {
    match kind {
        JoinKind::Inner => "INNER",
        JoinKind::Left => "LEFT",
        JoinKind::Right => "RIGHT",
        JoinKind::Full => "FULL",
        JoinKind::Cross => "CROSS",
    }
}

fn parse_join_kind(label: &str) -> JoinKind {
    match label {
        "LEFT" => JoinKind::Left,
        "RIGHT" => JoinKind::Right,
        "FULL" => JoinKind::Full,
        "CROSS" => JoinKind::Cross,
        _ => JoinKind::Inner,
    }
}

fn encode_table_key(schema: Option<&str>, table: &str) -> String {
    match schema.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => format!("{s}.{table}"),
        None => table.to_owned(),
    }
}

fn decode_table_key(key: &str) -> (Option<String>, String) {
    match key.split_once('.') {
        Some((s, t)) => (Some(s.to_owned()), t.to_owned()),
        None => (None, key.to_owned()),
    }
}
