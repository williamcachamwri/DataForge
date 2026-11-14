//! Bottom workspace status bar displaying real connection, query, cursor, and grid metrics.

use herogpui::components::{
    Button, Chip, ChipLabel, ChipVariant, Color, Icon, IconName, Orientation, ProgressBar,
    Separator, Size, Spinner, ToggleButton, ToggleVariant, Variant,
};
use herogpui::gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, Styled,
    Window, div, px,
};
use herogpui::theme::ActiveTheme;

use crate::accessibility::KeyCommand;
use crate::app::AppController;
use dataforge_gfx::glyphs::EngineKind;
use dataforge_state::connection_window::ConnectionWindowPhase;
use tablepro_core::SafeModeLevel;
use tablepro_core::meta::TableRef;

use super::DataForgeWorkspace;
use super::engine_icons::engine_icon;
use super::shell_state::{ToastSeverity, WorkspaceSubView};
use super::theme::{
    HAIRLINE_BORDER_PX, MICRO_GAP_PX, hairline_border_color, muted_surface_bg, value_gpui_font,
};

/// Renders the slim `26px` precision status footer driven by real controller and tab state.
#[must_use]
pub fn render_status_bar(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let numeric_font = value_gpui_font(&ws.controller.settings);
    let phase = ws
        .controller
        .connections
        .phase(ws.controller.active_workspace)
        .cloned();
    let active_result = ws.active_tab_result().cloned();

    div()
        .id("workspace-status-bar")
        .h(px(26.0))
        .w_full()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_between()
        .px(px(10.0))
        .gap(px(10.0))
        .bg(muted_surface_bg(cx))
        .border_t(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .text_size(px(11.5))
        .text_color(colors.muted)
        .child(render_left_status(ws, &phase, cx))
        .child(render_right_metrics(
            ws,
            &phase,
            active_result.as_ref(),
            numeric_font,
            cx,
        ))
        .into_any_element()
}

fn render_left_status(
    ws: &DataForgeWorkspace,
    phase: &Option<ConnectionWindowPhase>,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let controller = &ws.controller;
    let colors = cx.colors().clone();
    let mut left = div()
        .id("status-bar-left")
        .flex()
        .items_center()
        .gap(px(8.0))
        .min_w(px(0.0));

    let phase_node = match phase {
        Some(ConnectionWindowPhase::Connected) => render_connected_left_status(controller, cx),
        Some(ConnectionWindowPhase::Connecting { .. }) => {
            render_phase_spinner("status-bar-conn-spin", "Connecting...", colors.foreground)
        }
        Some(ConnectionWindowPhase::Reconnecting { .. }) => render_phase_spinner(
            "status-bar-reconn-spin",
            "Reconnecting...",
            colors.foreground,
        ),
        Some(ConnectionWindowPhase::Unavailable { .. }) => render_phase_dot(
            colors.danger.color,
            "Connection unavailable",
            colors.danger.color,
        ),
        Some(ConnectionWindowPhase::Disconnected) | None => {
            render_phase_dot(colors.muted, "Disconnected • Ready", colors.muted)
        }
    };
    left = left.child(phase_node);

    let sub_view_label = match ws.workspace_pane.sub_view {
        WorkspaceSubView::SqlEditor => "SQL Editor",
        WorkspaceSubView::DataGrid => "Data Grid",
        WorkspaceSubView::Structure => "Structure",
        WorkspaceSubView::SplitSqlAndGrid => "Split SQL + Grid",
    };
    left = left
        .child(
            Separator::new()
                .id("status-sep-view")
                .orientation(Orientation::Vertical)
                .sx(|s| s.h(px(12.0))),
        )
        .child(
            div()
                .text_color(colors.muted)
                .child(sub_view_label),
        );

    if matches!(phase, Some(ConnectionWindowPhase::Connected)) {
        if let Some(ref schema) = ws.sidebar_ui.selected_schema
            && !schema.is_empty()
        {
            left = left
                .child(
                    Separator::new()
                        .id("status-sep-schema")
                        .orientation(Orientation::Vertical)
                        .sx(|s| s.h(px(12.0))),
                )
                .child(
                    div()
                        .truncate()
                        .text_color(colors.muted)
                        .child(format!("schema: {schema}")),
                );
        }
        if let Some(table) = &controller.active_table {
            left = left
                .child(
                    Separator::new()
                        .id("status-sep-table")
                        .orientation(Orientation::Vertical)
                        .sx(|s| s.h(px(12.0))),
                )
                .child(
                    div()
                        .truncate()
                        .text_color(colors.foreground)
                        .child(format_table_ref(table)),
                );
        }
        let obj_count = controller.cached_objects.len();
        if obj_count > 0 {
            left = left
                .child(
                    Separator::new()
                        .id("status-sep-objs")
                        .orientation(Orientation::Vertical)
                        .sx(|s| s.h(px(12.0))),
                )
                .child(
                    div()
                        .text_color(colors.muted)
                        .child(format!(
                            "{obj_count} {}",
                            if obj_count == 1 { "object" } else { "objects" }
                        )),
                );
        }
        if controller.toolbar_status.safe_mode {
            left = left.child(
                Chip::new()
                    .variant(ChipVariant::Soft)
                    .color(Color::Warning)
                    .size(Size::Sm)
                    .text_size(px(10.0))
                    .child(ChipLabel::new().child("Safe Mode")),
            );
        }
    }

    left.into_any_element()
}

fn render_connected_left_status(
    controller: &AppController,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let engine = detected_engine(controller);
    let conn_label = format_connection_target(controller);

    let mut conn = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .min_w(px(0.0))
        .child(
            div()
                .w(px(6.0))
                .h(px(6.0))
                .rounded(px(99.0))
                .flex_shrink_0()
                .bg(colors.success.color),
        );
    if let Some(kind) = engine {
        conn = conn.child(engine_icon(kind, px(12.0), colors.foreground));
    }
    conn = conn.child(
        div()
            .truncate()
            .font_weight(FontWeight::MEDIUM)
            .text_color(colors.foreground)
            .child(conn_label),
    );

    let mut row = div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .min_w(px(0.0))
        .child(conn);
    if controller.active_safe_mode() != SafeModeLevel::Off {
        row = row.child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .color(Color::Warning)
                .size(Size::Sm)
                .text_size(px(10.5))
                .child(
                    Icon::new(IconName::ShieldCheck)
                        .size(px(11.0))
                        .color(colors.warning.color),
                )
                .child(ChipLabel::new().child("Safe Mode")),
        );
    }
    row.into_any_element()
}

fn render_phase_spinner(
    id: &'static str,
    label: &'static str,
    text_color: herogpui::gpui::Hsla,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(Spinner::new(id).size_px(px(12.0)))
        .child(div().text_color(text_color).child(label))
        .into_any_element()
}

fn render_phase_dot(
    dot_color: herogpui::gpui::Hsla,
    label: &'static str,
    text_color: herogpui::gpui::Hsla,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(
            div()
                .w(px(6.0))
                .h(px(6.0))
                .rounded(px(99.0))
                .flex_shrink_0()
                .bg(dot_color),
        )
        .child(div().text_color(text_color).child(label))
        .into_any_element()
}

fn render_right_metrics(
    ws: &DataForgeWorkspace,
    phase: &Option<ConnectionWindowPhase>,
    active_result: Option<&super::shell_state::QueryTabResultState>,
    numeric_font: herogpui::gpui::Font,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let controller = &ws.controller;
    let is_connected = matches!(phase, Some(ConnectionWindowPhase::Connected));
    let pending_edits = pending_edit_count(controller);

    let mut right = div()
        .id("status-bar-right")
        .flex()
        .items_center()
        .gap(px(8.0))
        .flex_shrink_0();

    if let Some(metric) = render_query_timing_metric(active_result, numeric_font.clone(), cx) {
        right = right.child(metric);
    }

    if let Some(row_label) = real_row_count_label(controller, is_connected, active_result) {
        right = right.child(
            div()
                .font(numeric_font.clone())
                .text_color(colors.muted)
                .child(row_label),
        );
    }

    if controller.status_bar.selection_count > 0 {
        let count = controller.status_bar.selection_count;
        right = right.child(
            div()
                .font(numeric_font.clone())
                .text_color(colors.foreground)
                .child(format!("{count} selected")),
        );
    }

    if pending_edits > 0 {
        right = right.child(render_pending_edits_chip(pending_edits));
    }

    if is_connected && (controller.status_bar.total_pages > 1 || !controller.grid_rows.is_empty()) {
        right = right.child(render_pagination_controls(
            controller,
            numeric_font.clone(),
            cx,
        ));
    }

    if matches!(
        ws.workspace_pane.sub_view,
        WorkspaceSubView::SqlEditor | WorkspaceSubView::SplitSqlAndGrid
    ) {
        right = right.child(render_cursor_and_vim(controller, numeric_font.clone(), cx));
    }

    let dialect = super::sql_editor::grammar_label(ws.active_grammar());
    let tab_count = controller.tabs.tabs().len();
    let editor_font_px = controller.settings.editor_font.size.round() as u32;

    right
        .child(
            Separator::new()
                .id("status-sep-meta")
                .orientation(Orientation::Vertical)
                .sx(|s| s.h(px(12.0))),
        )
        .child(div().text_color(colors.muted).child(dialect))
        .child(
            div()
                .font(numeric_font.clone())
                .text_color(colors.muted)
                .child(format!("{editor_font_px}px")),
        )
        .child(div().text_color(colors.muted).child("UTF-8"))
        .child(
            div()
                .font(numeric_font)
                .text_color(colors.muted)
                .child(format!(
                    "{tab_count} {}",
                    if tab_count == 1 { "tab" } else { "tabs" }
                )),
        )
        .child(
            Separator::new()
                .id("status-sep-panels")
                .orientation(Orientation::Vertical)
                .sx(|s| s.h(px(12.0))),
        )
        .child(render_panel_quick_toggles(controller, cx))
        .into_any_element()
}

fn render_query_timing_metric(
    active_result: Option<&super::shell_state::QueryTabResultState>,
    numeric_font: herogpui::gpui::Font,
    cx: &Context<DataForgeWorkspace>,
) -> Option<AnyElement> {
    let res = active_result?;
    if res.running {
        return Some(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    ProgressBar::new("status-query-progress")
                        .is_indeterminate(true)
                        .size(Size::Sm)
                        .color(Color::Accent)
                        .sx(|s| s.w(px(56.0))),
                )
                .child("Running...")
                .into_any_element(),
        );
    }
    let ms = elapsed_ms_for_display(res)?;
    Some(
        div()
            .font(numeric_font)
            .text_color(cx.colors().foreground)
            .child(format_elapsed_ms(ms))
            .into_any_element(),
    )
}

fn render_pending_edits_chip(pending_edits: usize) -> AnyElement {
    Chip::new()
        .variant(ChipVariant::Soft)
        .color(Color::Warning)
        .size(Size::Sm)
        .text_size(px(10.5))
        .child(ChipLabel::new().child(format!(
            "{pending_edits} {}",
            if pending_edits == 1 { "edit" } else { "edits" }
        )))
        .into_any_element()
}

fn render_pagination_controls(
    controller: &AppController,
    numeric_font: herogpui::gpui::Font,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let page = controller.status_bar.current_page.max(1);
    let total = controller.status_bar.total_pages.max(1);
    let can_prev = controller.grid_page_prev.is_some() && page > 1;
    let can_next = controller.grid_page_next.is_some() && page < total;

    div()
        .id("status-bar-pagination")
        .flex()
        .items_center()
        .gap(px(MICRO_GAP_PX))
        .child(
            Button::new("status-prev-page")
                .variant(Variant::Ghost)
                .size(Size::Sm)
                .is_icon_only(true)
                .width(px(18.0))
                .height(px(18.0))
                .is_disabled(!can_prev)
                .aria_label("Previous Page")
                .on_press(cx.listener(|ws, _, _, cx| {
                    if let Err(err) = ws.controller.prev_grid_page_blocking() {
                        ws.notify_toast(
                            ToastSeverity::Error,
                            "Previous page failed",
                            Some(err.to_string().into()),
                            cx,
                        );
                    }
                    cx.notify();
                }))
                .child(
                    Icon::new(IconName::ChevronLeft)
                        .size(px(12.0))
                        .color(if can_prev {
                            colors.foreground
                        } else {
                            colors.muted
                        }),
                ),
        )
        .child(
            div()
                .font(numeric_font)
                .text_size(px(11.0))
                .text_color(colors.muted)
                .child(format!("p.{page}/{total}")),
        )
        .child(
            Button::new("status-next-page")
                .variant(Variant::Ghost)
                .size(Size::Sm)
                .is_icon_only(true)
                .width(px(18.0))
                .height(px(18.0))
                .is_disabled(!can_next)
                .aria_label("Next Page")
                .on_press(cx.listener(|ws, _, _, cx| {
                    if let Err(err) = ws.controller.next_grid_page_blocking() {
                        ws.notify_toast(
                            ToastSeverity::Error,
                            "Next page failed",
                            Some(err.to_string().into()),
                            cx,
                        );
                    }
                    cx.notify();
                }))
                .child(
                    Icon::new(IconName::ChevronRight)
                        .size(px(12.0))
                        .color(if can_next {
                            colors.foreground
                        } else {
                            colors.muted
                        }),
                ),
        )
        .into_any_element()
}

fn render_cursor_and_vim(
    controller: &AppController,
    numeric_font: herogpui::gpui::Font,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let (line, col) = controller
        .editor_cursors
        .iter()
        .find(|c| c.is_primary)
        .or_else(|| controller.editor_cursors.first())
        .map_or((1, 1), |c| (c.line + 1, c.col + 1));

    let mut row = div()
        .id("status-bar-cursor")
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(
            div()
                .font(numeric_font)
                .text_color(colors.muted)
                .child(format!("Ln {line}, Col {col}")),
        );

    if let Some(mode) = controller.vim_mode {
        row = row.child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .color(Color::Accent)
                .size(Size::Sm)
                .text_size(px(10.0))
                .child(ChipLabel::new().child(mode.label())),
        );
    }

    row.into_any_element()
}

fn format_table_ref(table: &TableRef) -> String {
    match table.schema.as_deref() {
        Some(schema) if !schema.is_empty() => format!("{schema}.{}", table.name),
        _ => table.name.clone(),
    }
}

fn render_panel_quick_toggles(
    controller: &AppController,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let inspector_on = controller.layout_config.inspector_visible;
    let ai_on = controller.layout_config.ai_drawer_visible;

    div()
        .id("status-bar-panel-toggles")
        .flex()
        .items_center()
        .gap(px(2.0))
        .child(
            ToggleButton::new("status-toggle-inspector")
                .variant(ToggleVariant::Ghost)
                .size(Size::Sm)
                .is_icon_only(true)
                .sx(|s| s.h(px(20.0)).w(px(22.0)).px(px(0.0)))
                .is_selected(inspector_on)
                .aria_label("Toggle Row Inspector")
                .on_press(cx.listener(|ws, _, window, cx| {
                    ws.execute_key_command(KeyCommand::ToggleInspector, window, cx);
                }))
                .child(
                    Icon::new(IconName::Columns2)
                        .size(px(12.0))
                        .color(if inspector_on {
                            colors.accent.color
                        } else {
                            colors.muted
                        }),
                ),
        )
        .child(
            ToggleButton::new("status-toggle-ai")
                .variant(ToggleVariant::Ghost)
                .size(Size::Sm)
                .is_icon_only(true)
                .sx(|s| s.h(px(20.0)).w(px(22.0)).px(px(0.0)))
                .is_selected(ai_on)
                .aria_label("Toggle AI Assistant")
                .on_press(cx.listener(|ws, _, window, cx| {
                    ws.execute_key_command(KeyCommand::ToggleAiDrawer, window, cx);
                }))
                .child(
                    Icon::new(IconName::Sparkles)
                        .size(px(12.0))
                        .color(if ai_on {
                            colors.accent.color
                        } else {
                            colors.muted
                        }),
                ),
        )
        .into_any_element()
}

fn detected_engine(controller: &AppController) -> Option<EngineKind> {
    let version = controller.toolbar_status.engine_version.as_str();
    EngineKind::ALL
        .iter()
        .copied()
        .find(|kind| version.starts_with(kind.display_name()))
        .or_else(|| {
            controller
                .rail_items
                .iter()
                .find(|item| item.connected)
                .map(|item| item.engine)
        })
}

fn format_connection_target(controller: &AppController) -> String {
    let host = controller.toolbar_status.host.trim();
    let db = controller.toolbar_status.database.trim();
    match (!host.is_empty() && host != "—", !db.is_empty() && db != "—") {
        (true, true) => format!("{host} / {db}"),
        (true, false) => host.to_owned(),
        (false, true) => db.to_owned(),
        (false, false) => "Connected".to_owned(),
    }
}

fn elapsed_ms_for_display(res: &super::shell_state::QueryTabResultState) -> Option<u64> {
    res.elapsed_ms.or_else(|| {
        res.accumulator
            .summary
            .as_ref()
            .map(|summary| u64::try_from(summary.elapsed.as_millis()).unwrap_or(u64::MAX))
    })
}

fn format_elapsed_ms(ms: u64) -> String {
    if ms >= 1000 {
        let secs = ms / 1000;
        let hundredths = (ms % 1000) / 10;
        format!("{secs}.{hundredths:02} s")
    } else {
        format!("{ms} ms")
    }
}

fn real_row_count_label(
    controller: &AppController,
    is_connected: bool,
    active_result: Option<&super::shell_state::QueryTabResultState>,
) -> Option<String> {
    if let Some(res) = active_result
        && let Some(set) = res.accumulator.result_sets.get(res.active_result_set)
    {
        let n = set.total_rows.unwrap_or(set.rows.len() as u64);
        return Some(pluralize_rows(n));
    }
    if let Some(total) = controller.grid_model.page.table_row_count {
        return Some(pluralize_rows(total));
    }
    if !controller.grid_rows.is_empty() {
        return Some(pluralize_rows(controller.grid_rows.len() as u64));
    }
    if is_connected && controller.active_table.is_some() {
        return Some("0 rows".to_owned());
    }
    None
}

fn pluralize_rows(n: u64) -> String {
    if n == 1 {
        "1 row".to_owned()
    } else {
        format!("{n} rows")
    }
}

fn pending_edit_count(controller: &AppController) -> usize {
    let edits = &controller.grid_model.edits;
    edits.modified_cells.len() + edits.inserted_rows.len() + edits.deleted_rows.len()
}
