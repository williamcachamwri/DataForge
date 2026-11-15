//! Schema & Data Compare diff and synchronization workbench view.

use dataforge_services::{
    ColumnChangeKind, ColumnDiff, DataDiffReport, RowDiffEntry, SchemaDiffReport, diff_table_rows,
    diff_table_schemas,
};
use dataforge_state::connection_window::ConnectionWindowPhase;
use herogpui::components::{
    Alert, Button, Chip, ChipLabel, ChipVariant, Icon, IconName, IconSize, PickerItem, Select,
    Separator,
};
use herogpui::core::{Color as HeroColor, Size as HeroSize, Variant};
use herogpui::gpui::prelude::FluentBuilder as _;
use herogpui::gpui::{
    AnyElement, App, ClipboardItem, Context, FontWeight, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use herogpui::theme::ActiveTheme;
use tablepro_core::QueryResult;
use tablepro_core::ddl::TableDefinition;
use tablepro_core::meta::{ObjectKind, TableRef};

use super::DataForgeWorkspace;
use super::shell_state::{push_error_toast, push_success_toast};
use super::theme::{
    CONTROL_RADIUS_PX, PANEL_RADIUS_PX, editor_gpui_font, hairline_border_color, muted_surface_bg,
    panel_surface_bg, row_zebra_bg,
};

const MAX_DISPLAYED_DATA_ROWS: usize = 200;

/// Persistent UI state for the Schema & Data Compare view.
#[derive(Debug, Default)]
pub struct CompareViewState {
    pub data_mode: bool,
    source_table: Option<TableRef>,
    target_table: Option<TableRef>,
    schema_report: Option<SchemaDiffReport>,
    data_report: Option<DataDiffReport>,
    last_error: Option<String>,
    show_sync_sql: bool,
}

impl CompareViewState {
    /// Creates initial Compare view state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) -> Self {
        Self::default()
    }
}

/// Renders the Schema & Data Compare workbench tool view.
#[must_use]
pub fn render_compare_view(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let tables = collect_comparable_tables(&ws.controller.cached_objects);
    ensure_default_table_selection(
        &mut ws.compare_ui,
        &tables,
        ws.controller.active_table.as_ref(),
    );

    div()
        .id("compare-view")
        .size_full()
        .flex()
        .flex_col()
        .bg(panel_surface_bg(cx))
        .child(render_compare_header(ws, cx))
        .child(render_compare_controls(ws, &tables, cx))
        .child(Separator::new())
        .child(render_compare_body(ws, cx))
        .into_any_element()
}

fn collect_comparable_tables(objects: &[tablepro_core::meta::ObjectInfo]) -> Vec<TableRef> {
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

fn ensure_default_table_selection(
    state: &mut CompareViewState,
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
    if state
        .target_table
        .as_ref()
        .is_none_or(|t| !tables.contains(t))
    {
        state.target_table = tables
            .iter()
            .find(|t| state.source_table.as_ref() != Some(*t))
            .cloned()
            .or_else(|| tables.first().cloned());
    }
}

fn render_compare_header(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let data_mode = ws.compare_ui.data_mode;

    let mode_switch = div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(
            Button::new("compare-mode-schema")
                .variant(if data_mode {
                    Variant::Secondary
                } else {
                    Variant::Primary
                })
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _, cx| {
                    ws.compare_ui.data_mode = false;
                    cx.notify();
                }))
                .child(s.compare_schema),
        )
        .child(
            Button::new("compare-mode-data")
                .variant(if data_mode {
                    Variant::Primary
                } else {
                    Variant::Secondary
                })
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _, cx| {
                    ws.compare_ui.data_mode = true;
                    cx.notify();
                }))
                .child(s.compare_data),
        );

    div()
        .min_h(px(42.0))
        .px(px(14.0))
        .py(px(6.0))
        .border_b_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .items_center()
        .justify_between()
        .flex_wrap()
        .gap(px(12.0))
        .child(
            div()
                .min_w_0()
                .flex()
                .items_center()
                .flex_wrap()
                .gap(px(10.0))
                .child(
                    Icon::new(IconName::ArrowLeftRight)
                        .size(IconSize::Sm)
                        .color(colors.accent.color),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(s.compare_title),
                )
                .child(mode_switch),
        )
        .child(
            Button::new("compare-view-close")
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

fn build_table_select(
    id: &'static str,
    label: &'static str,
    tables: &[TableRef],
    picker_items: Vec<PickerItem>,
    selected: Option<&TableRef>,
    is_source: bool,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let val = selected
        .and_then(|t| tables.iter().position(|x| x == t))
        .map(|i| SharedString::from(i.to_string()));
    let tables_owned = tables.to_vec();

    Select::new(id, picker_items)
        .label(label)
        .full_width(true)
        .is_disabled(tables.is_empty())
        .value(val)
        .on_selection_change(cx.listener(move |ws, key: &Option<SharedString>, _w, cx| {
            if let Some(idx) = key.as_deref().and_then(|k| k.parse::<usize>().ok())
                && let Some(t) = tables_owned.get(idx).cloned()
            {
                if is_source {
                    ws.compare_ui.source_table = Some(t);
                } else {
                    ws.compare_ui.target_table = Some(t);
                }
                cx.notify();
            }
        }))
        .into_any_element()
}

fn render_compare_controls(
    ws: &DataForgeWorkspace,
    tables: &[TableRef],
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let is_connected = matches!(
        ws.controller
            .connections
            .phase(ws.controller.active_workspace),
        Some(ConnectionWindowPhase::Connected)
    );
    let can_run = is_connected
        && ws.compare_ui.source_table.is_some()
        && ws.compare_ui.target_table.is_some();

    let picker_items: Vec<PickerItem> = tables
        .iter()
        .enumerate()
        .map(|(i, t)| PickerItem::new(i.to_string(), format_table_ref(t)))
        .collect();

    let src_select = build_table_select(
        "compare-source-select",
        s.compare_source,
        tables,
        picker_items.clone(),
        ws.compare_ui.source_table.as_ref(),
        true,
        cx,
    );
    let tgt_select = build_table_select(
        "compare-target-select",
        s.compare_target,
        tables,
        picker_items,
        ws.compare_ui.target_table.as_ref(),
        false,
        cx,
    );

    div()
        .px(px(14.0))
        .py(px(10.0))
        .bg(muted_surface_bg(cx))
        .flex()
        .items_end()
        .flex_wrap()
        .gap(px(10.0))
        .child(div().flex_1().min_w(px(140.0)).child(src_select))
        .child(
            Button::new("compare-swap-tables")
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .is_icon_only(true)
                .is_disabled(!can_run)
                .aria_label(s.compare_swap_label)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    std::mem::swap(
                        &mut ws.compare_ui.source_table,
                        &mut ws.compare_ui.target_table,
                    );
                    cx.notify();
                }))
                .child(Icon::new(IconName::ArrowLeftRight).size(IconSize::Xs)),
        )
        .child(div().flex_1().min_w(px(140.0)).child(tgt_select))
        .child(
            Button::new("compare-run-btn")
                .variant(Variant::Primary)
                .size(HeroSize::Sm)
                .is_disabled(!can_run)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    run_compare(ws, cx);
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .child(Icon::new(IconName::Play).size(IconSize::Xs))
                        .child(s.compare_run),
                ),
        )
        .into_any_element()
}

fn render_compare_body(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let is_connected = matches!(
        ws.controller
            .connections
            .phase(ws.controller.active_workspace),
        Some(ConnectionWindowPhase::Connected)
    );

    if !is_connected {
        return render_centered_empty(s.no_active_connection, s.no_active_connection_desc, cx);
    }

    let mut root = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .p(px(14.0))
        .gap(px(10.0));

    if let Some(err) = ws.compare_ui.last_error.as_ref() {
        root = root.child(
            Alert::new(s.query_error_title)
                .status(HeroColor::Danger)
                .description(err.clone()),
        );
    }

    if ws.compare_ui.data_mode {
        if let Some(report) = ws.compare_ui.data_report.as_ref() {
            return root
                .child(render_data_summary_bar(ws, report, cx))
                .when(!report.warnings.is_empty(), |el| {
                    el.child(
                        Alert::new(s.compare_differences)
                            .status(HeroColor::Warning)
                            .description(report.warnings.join(" ")),
                    )
                })
                .child(render_data_diff_list(ws, report, cx))
                .when(
                    ws.compare_ui.show_sync_sql && !report.sync_sql.is_empty(),
                    |el| el.child(render_sync_sql_panel(ws, &report.sync_sql, cx)),
                )
                .into_any_element();
        }
    } else if let Some(report) = ws.compare_ui.schema_report.as_ref() {
        return root
            .child(render_schema_summary_bar(ws, report, cx))
            .child(render_schema_diff_list(ws, report, cx))
            .when(
                ws.compare_ui.show_sync_sql && !report.sync_ddl.is_empty(),
                |el| el.child(render_sync_sql_panel(ws, &report.sync_ddl, cx)),
            )
            .into_any_element();
    }

    div()
        .flex_1()
        .min_h_0()
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
                .child(s.compare_title),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(colors.muted)
                .child(format!(
                    "{} / {} — {}",
                    s.compare_source, s.compare_target, s.compare_run
                )),
        )
        .into_any_element()
}

fn render_schema_summary_bar(
    ws: &DataForgeWorkspace,
    report: &SchemaDiffReport,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let counts = schema_diff_counts(report);
    let sync_statements = report.sync_ddl.clone();

    let mut bar = div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .flex_wrap();

    let mut pills = div().flex().items_center().gap(px(6.0)).flex_wrap();
    if report.is_identical {
        pills = pills.child(diff_chip(
            format!("= {}", s.compare_identical),
            HeroColor::Success,
        ));
    } else {
        pills = pills
            .child(diff_chip(
                s.compare_differences.to_owned(),
                HeroColor::Warning,
            ))
            .child(diff_chip(
                format!("+ {} {}", counts.added, s.compare_added_label),
                HeroColor::Success,
            ))
            .child(diff_chip(
                format!("- {} {}", counts.removed, s.compare_removed_label),
                HeroColor::Danger,
            ))
            .child(diff_chip(
                format!("~ {} {}", counts.modified, s.compare_modified_label),
                HeroColor::Accent,
            ));
    }

    bar = bar.child(pills);
    if !sync_statements.is_empty() {
        bar = bar.child(render_sync_action_buttons(ws, sync_statements, cx));
    }
    bar.into_any_element()
}

fn render_data_summary_bar(
    ws: &DataForgeWorkspace,
    report: &DataDiffReport,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let added = report.only_in_source.len();
    let removed = report.only_in_target.len();
    let modified = report.modified.len();
    let identical = report.matching_count;
    let has_diffs = added > 0 || removed > 0 || modified > 0;

    let mut bar = div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .flex_wrap();

    let mut pills = div().flex().items_center().gap(px(6.0)).flex_wrap();
    if !has_diffs && report.warnings.is_empty() {
        pills = pills.child(diff_chip(
            format!("= {identical} {}", s.compare_identical),
            HeroColor::Success,
        ));
    } else {
        pills = pills
            .child(diff_chip(
                s.compare_differences.to_owned(),
                HeroColor::Warning,
            ))
            .child(diff_chip(
                format!("+ {added} {}", s.compare_added_label),
                HeroColor::Success,
            ))
            .child(diff_chip(
                format!("- {removed} {}", s.compare_removed_label),
                HeroColor::Danger,
            ))
            .child(diff_chip(
                format!("~ {modified} {}", s.compare_modified_label),
                HeroColor::Accent,
            ))
            .child(diff_chip(
                format!("= {identical} {}", s.compare_identical),
                HeroColor::Default,
            ));
    }

    bar = bar.child(pills);
    if !report.sync_sql.is_empty() {
        bar = bar.child(render_sync_action_buttons(ws, report.sync_sql.clone(), cx));
    }
    bar.into_any_element()
}

fn render_sync_action_buttons(
    ws: &DataForgeWorkspace,
    statements: Vec<String>,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(
            Button::new("compare-toggle-sync-sql")
                .variant(if ws.compare_ui.show_sync_sql {
                    Variant::Primary
                } else {
                    Variant::Secondary
                })
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    ws.compare_ui.show_sync_sql = !ws.compare_ui.show_sync_sql;
                    cx.notify();
                }))
                .child(s.compare_generate_sync),
        )
        .child(
            Button::new("compare-execute-sync")
                .variant(Variant::Primary)
                .size(HeroSize::Sm)
                .on_press(cx.listener(move |ws, _, _w, cx| {
                    execute_sync_statements(ws, &statements, cx);
                }))
                .child(s.compare_execute_sync),
        )
        .into_any_element()
}

struct SchemaDiffItem {
    badge: &'static str,
    kind: &'static str,
    name: String,
    detail: String,
    color: HeroColor,
}

fn collect_schema_diff_items(
    report: &SchemaDiffReport,
    s: &crate::i18n::Strings,
) -> Vec<SchemaDiffItem> {
    let mut items = Vec::new();
    let push_names = |items: &mut Vec<SchemaDiffItem>,
                      names: &[String],
                      badge: &'static str,
                      kind: &'static str,
                      detail: &str,
                      color: HeroColor| {
        for name in names {
            items.push(SchemaDiffItem {
                badge,
                kind,
                name: name.clone(),
                detail: detail.to_owned(),
                color,
            });
        }
    };

    let added_detail = format!("{}: {}", s.compare_added_label, s.compare_target);
    let removed_detail = format!("{}: {}", s.compare_removed_label, s.compare_target);

    push_names(
        &mut items,
        &report.added_columns,
        "+",
        s.struct_col_column,
        s.schema_add_column,
        HeroColor::Success,
    );
    push_names(
        &mut items,
        &report.removed_columns,
        "-",
        s.struct_col_column,
        s.schema_drop_column,
        HeroColor::Danger,
    );
    for diff in &report.modified_columns {
        items.push(SchemaDiffItem {
            badge: "~",
            kind: s.struct_col_column,
            name: diff.column_name.clone(),
            detail: format_column_diff(diff, s),
            color: HeroColor::Accent,
        });
    }
    push_names(
        &mut items,
        &report.added_indexes,
        "+",
        s.indexes_label,
        &added_detail,
        HeroColor::Success,
    );
    push_names(
        &mut items,
        &report.removed_indexes,
        "-",
        s.indexes_label,
        &removed_detail,
        HeroColor::Danger,
    );
    push_names(
        &mut items,
        &report.added_foreign_keys,
        "+",
        s.er_fk_badge,
        &added_detail,
        HeroColor::Success,
    );
    push_names(
        &mut items,
        &report.removed_foreign_keys,
        "-",
        s.er_fk_badge,
        &removed_detail,
        HeroColor::Danger,
    );
    items
}

fn render_schema_diff_list(
    ws: &DataForgeWorkspace,
    report: &SchemaDiffReport,
    cx: &App,
) -> AnyElement {
    let s = &ws.controller.strings;
    let mut list = div()
        .id("compare-diff-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(4.0));

    if report.is_identical {
        return list
            .child(render_centered_empty(
                s.compare_identical,
                &report.table_name,
                cx,
            ))
            .into_any_element();
    }

    for (idx, item) in collect_schema_diff_items(report, s).into_iter().enumerate() {
        list = list.child(render_diff_row(
            idx,
            item.badge,
            item.kind,
            &item.name,
            &item.detail,
            item.color,
            cx,
        ));
    }

    list.into_any_element()
}

fn render_data_diff_list(ws: &DataForgeWorkspace, report: &DataDiffReport, cx: &App) -> AnyElement {
    let s = &ws.controller.strings;
    let mut list = div()
        .id("compare-diff-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(4.0));

    let total_diffs =
        report.only_in_source.len() + report.only_in_target.len() + report.modified.len();
    if total_diffs == 0 {
        return list
            .child(render_centered_empty(
                s.compare_identical,
                &format!("{} {}", report.matching_count, s.rows_suffix),
                cx,
            ))
            .into_any_element();
    }

    let entries = report
        .only_in_source
        .iter()
        .chain(report.modified.iter())
        .chain(report.only_in_target.iter())
        .take(MAX_DISPLAYED_DATA_ROWS);

    for (idx, entry) in entries.enumerate() {
        list = list.child(render_data_diff_entry(idx, entry, s, cx));
    }
    list.into_any_element()
}

fn render_data_diff_entry(
    idx: usize,
    entry: &RowDiffEntry,
    s: &crate::i18n::Strings,
    cx: &App,
) -> AnyElement {
    let pk = format!(
        "{} ({})",
        s.schema_primary_key,
        entry.primary_key.join(", ")
    );
    let (badge, tag, detail, color) = match entry.kind {
        dataforge_services::RowDiffKind::OnlyInSource => (
            "+",
            s.compare_added_label,
            format!(
                "{}: [{}]",
                s.compare_source_prefix,
                entry.source_values.join(", ")
            ),
            HeroColor::Success,
        ),
        dataforge_services::RowDiffKind::OnlyInTarget => (
            "-",
            s.compare_removed_label,
            format!(
                "{}: [{}]",
                s.compare_target_prefix,
                entry.target_values.join(", ")
            ),
            HeroColor::Danger,
        ),
        dataforge_services::RowDiffKind::Modified => (
            "~",
            s.compare_modified_label,
            format!(
                "{}: {} | {} [{}] -> {} [{}]",
                s.compare_modified_label,
                entry.changed_columns.join(", "),
                s.compare_source_prefix,
                entry.source_values.join(", "),
                s.compare_target_prefix,
                entry.target_values.join(", ")
            ),
            HeroColor::Accent,
        ),
    };
    render_diff_row(idx, badge, tag, &pk, &detail, color, cx)
}

fn render_diff_row(
    idx: usize,
    symbol: &str,
    category: &str,
    title: &str,
    detail: &str,
    color: HeroColor,
    cx: &App,
) -> AnyElement {
    let colors = cx.colors();
    let mut row = div()
        .w_full()
        .px(px(10.0))
        .py(px(6.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .border_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(10.0));
    if let Some(zebra) = row_zebra_bg(idx, cx) {
        row = row.bg(zebra);
    }

    row.child(
        div()
            .min_w_0()
            .flex_1()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div().flex_shrink_0().child(
                    Chip::new()
                        .variant(ChipVariant::Soft)
                        .color(color)
                        .size(HeroSize::Sm)
                        .child(ChipLabel::new().child(format!("{symbol} {category}"))),
                ),
            )
            .child(
                div()
                    .max_w(px(240.0))
                    .truncate()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.foreground)
                    .child(title.to_owned()),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_size(px(11.5))
                    .text_color(colors.muted)
                    .child(detail.to_owned()),
            ),
    )
    .into_any_element()
}

fn render_sync_sql_panel(
    ws: &DataForgeWorkspace,
    statements: &[String],
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let sql_text = statements.join("\n");
    let copy_sql = sql_text.clone();
    let font = editor_gpui_font(&ws.controller.settings);

    div()
        .w_full()
        .max_h(px(180.0))
        .rounded(px(PANEL_RADIUS_PX))
        .border_1()
        .border_color(hairline_border_color(cx))
        .bg(muted_surface_bg(cx))
        .flex()
        .flex_col()
        .child(
            div()
                .px(px(10.0))
                .py(px(6.0))
                .border_b_1()
                .border_color(hairline_border_color(cx))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(format!(
                            "{} ({})",
                            s.compare_generate_sync,
                            statements.len()
                        )),
                )
                .child(
                    Button::new("compare-copy-sync-sql")
                        .variant(Variant::Ghost)
                        .size(HeroSize::Sm)
                        .on_press(cx.listener(move |ws, _, _w, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_sql.clone()));
                            let msg = ws.controller.strings.copied;
                            push_success_toast(msg, None, cx);
                        }))
                        .child(s.copy_label),
                ),
        )
        .child(
            div()
                .id("compare-sync-sql-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(10.0))
                .font(font)
                .text_size(px(11.5))
                .text_color(colors.foreground)
                .child(sql_text),
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
        .gap(px(4.0))
        .p(px(24.0))
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

fn diff_chip(label: String, color: HeroColor) -> Chip {
    Chip::new()
        .variant(ChipVariant::Soft)
        .color(color)
        .size(HeroSize::Sm)
        .child(ChipLabel::new().child(label))
}

fn run_compare(ws: &mut DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) {
    let (Some(src), Some(tgt)) = (
        ws.compare_ui.source_table.clone(),
        ws.compare_ui.target_table.clone(),
    ) else {
        return;
    };
    ws.compare_ui.last_error = None;
    let engine = ws.controller.active_database_engine();

    if !ws.compare_ui.data_mode {
        let src_def = match ws
            .controller
            .fetch_table_definition_blocking(src.schema.as_deref(), &src.name)
        {
            Ok(def) => def,
            Err(err) => {
                ws.compare_ui.last_error = Some(err.to_string());
                return;
            }
        };
        let tgt_def = match ws
            .controller
            .fetch_table_definition_blocking(tgt.schema.as_deref(), &tgt.name)
        {
            Ok(def) => def,
            Err(err) => {
                ws.compare_ui.last_error = Some(err.to_string());
                return;
            }
        };
        ws.compare_ui.schema_report = Some(diff_table_schemas(engine, &src_def, &tgt_def));
        ws.compare_ui.data_report = None;
        cx.notify();
        return;
    }

    let pk_cols = match ws
        .controller
        .fetch_table_definition_blocking(src.schema.as_deref(), &src.name)
    {
        Ok(def) => extract_primary_key_columns(&def),
        Err(err) => {
            ws.compare_ui.last_error = Some(err.to_string());
            return;
        }
    };

    let src_sql = format!(
        "SELECT * FROM {};",
        engine.qualify_table(src.schema.as_deref(), &src.name)
    );
    let tgt_sql = format!(
        "SELECT * FROM {};",
        engine.qualify_table(tgt.schema.as_deref(), &tgt.name)
    );
    let src_res = match ws.controller.execute_query_capture_blocking(&src_sql) {
        Ok((cols, rows)) => QueryResult::new(cols, rows),
        Err(err) => {
            ws.compare_ui.last_error = Some(err.to_string());
            return;
        }
    };
    let tgt_res = match ws.controller.execute_query_capture_blocking(&tgt_sql) {
        Ok((cols, rows)) => QueryResult::new(cols, rows),
        Err(err) => {
            ws.compare_ui.last_error = Some(err.to_string());
            return;
        }
    };

    ws.compare_ui.data_report = Some(diff_table_rows(
        engine,
        tgt.schema.as_deref(),
        &tgt.name,
        &pk_cols,
        &src_res,
        &tgt_res,
    ));
    ws.compare_ui.schema_report = None;
    cx.notify();
}

fn execute_sync_statements(
    ws: &mut DataForgeWorkspace,
    statements: &[String],
    cx: &mut Context<DataForgeWorkspace>,
) {
    match ws.controller.execute_ddl_statements_blocking(statements) {
        Ok(count) => {
            let title = ws.controller.strings.compare_execute_sync;
            let detail = format!("{count} {}", ws.controller.strings.statements_unit);
            push_success_toast(title, Some(detail.into()), cx);
            run_compare(ws, cx);
        }
        Err(err) => {
            ws.compare_ui.last_error = Some(err.to_string());
            let title = ws.controller.strings.compare_execute_sync;
            push_error_toast(title, Some(err.to_string().into()), cx);
        }
    }
    cx.notify();
}

fn extract_primary_key_columns(def: &TableDefinition) -> Vec<String> {
    let from_cols: Vec<String> = def
        .columns
        .iter()
        .filter(|c| c.info.primary_key)
        .map(|c| c.name().to_owned())
        .collect();
    if !from_cols.is_empty() {
        return from_cols;
    }
    def.primary_key
        .as_ref()
        .map(|pk| pk.columns.clone())
        .unwrap_or_default()
}

fn format_table_ref(t: &TableRef) -> String {
    match t.schema.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(schema) => format!("{schema}.{}", t.name),
        None => t.name.clone(),
    }
}

fn format_column_diff(diff: &ColumnDiff, s: &crate::i18n::Strings) -> String {
    diff.changes
        .iter()
        .map(|c| match c {
            ColumnChangeKind::TypeChanged {
                source_type,
                target_type,
            } => format!("{}: {target_type} -> {source_type}", s.schema_column_type),
            ColumnChangeKind::NullabilityChanged {
                source_nullable,
                target_nullable,
            } => format!(
                "{}: {target_nullable} -> {source_nullable}",
                s.schema_nullable
            ),
            ColumnChangeKind::DefaultChanged {
                source_default,
                target_default,
            } => format!(
                "{}: {} -> {}",
                s.schema_default_value,
                target_default.as_deref().unwrap_or(s.none_label),
                source_default.as_deref().unwrap_or(s.none_label)
            ),
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SchemaDiffCounts {
    added: usize,
    removed: usize,
    modified: usize,
}

fn schema_diff_counts(report: &SchemaDiffReport) -> SchemaDiffCounts {
    SchemaDiffCounts {
        added: report.added_columns.len()
            + report.added_indexes.len()
            + report.added_foreign_keys.len(),
        removed: report.removed_columns.len()
            + report.removed_indexes.len()
            + report.removed_foreign_keys.len(),
        modified: report.modified_columns.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tablepro_core::meta::{ObjectInfo, ObjectKind};

    #[test]
    fn compare_table_defaults_pick_distinct_tables() {
        let tables = collect_comparable_tables(&[
            ObjectInfo::new(Some("public".to_owned()), "users", ObjectKind::Table),
            ObjectInfo::new(Some("public".to_owned()), "active_users", ObjectKind::View),
            ObjectInfo::new(Some("public".to_owned()), "orders", ObjectKind::Table),
        ]);
        assert_eq!(tables.len(), 2);

        let mut state = CompareViewState::default();
        ensure_default_table_selection(&mut state, &tables, None);
        assert_eq!(
            state.source_table.as_ref().map(|t| t.name.as_str()),
            Some("users")
        );
        assert_eq!(
            state.target_table.as_ref().map(|t| t.name.as_str()),
            Some("orders")
        );
    }

    #[test]
    fn schema_diff_counts_totals_columns_indexes_and_fks() {
        let report = SchemaDiffReport {
            table_name: "users".to_owned(),
            added_columns: vec!["a".to_owned()],
            removed_columns: vec!["b".to_owned(), "c".to_owned()],
            modified_columns: vec![ColumnDiff {
                column_name: "d".to_owned(),
                changes: vec![ColumnChangeKind::NullabilityChanged {
                    source_nullable: false,
                    target_nullable: true,
                }],
            }],
            added_indexes: vec!["idx1".to_owned()],
            removed_indexes: Vec::new(),
            added_foreign_keys: Vec::new(),
            removed_foreign_keys: vec!["fk1".to_owned()],
            is_identical: false,
            sync_ddl: Vec::new(),
        };
        assert_eq!(
            schema_diff_counts(&report),
            SchemaDiffCounts {
                added: 2,
                removed: 3,
                modified: 1
            }
        );
    }
}
