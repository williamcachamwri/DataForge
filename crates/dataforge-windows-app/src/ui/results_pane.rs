//! Execution results, notices, and diagnostics pane for SQL query tabs.
//!
//! Driven exclusively by [`QueryTabResultState`] ([`RunEventAccumulator`]),
//! displaying live streaming state, multi-result-set tabs, tabular numeric
//! summaries, server notices, errors, and 2D-virtualized result grids.

use std::sync::Arc;

use dataforge_state::editor::AccumulatedResultSet;
use herogpui::components::{
    Alert, Button, Chip, ChipLabel, ChipVariant, Icon, IconName, IconSize, ProgressBar, Scrollbar,
    Spinner, SpinnerSize, TabItem, Tabs, TabsSize, TabsVariant,
};
use herogpui::core::{
    Color as HeroColor, Orientation as HeroOrientation, Size as HeroSize, Variant,
};
use herogpui::gpui::{
    self, AnyElement, App, ClipboardItem, Context, FontFeatures, Hsla, InteractiveElement,
    IntoElement, MouseButton, ParentElement, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use herogpui::prelude::FluentBuilder;
use herogpui::theme::ActiveTheme;
use tablepro_core::value::{Temporal, Value};
use tablepro_core::{TransactionState, Truncation};

use super::shell_state::{QueryTabResultState, ResultPaneSubView, ToastSeverity};
use super::{DataForgeWorkspace, theme};

const DEFAULT_RESULTS_HEIGHT_PX: f32 = 240.0;
const MIN_RESULTS_HEIGHT_PX: f32 = 120.0;
const MAX_RESULTS_HEIGHT_PX: f32 = 520.0;
const ROW_HEIGHT_PX: f32 = 26.0;
const HEADER_HEIGHT_PX: f32 = 28.0;
const ROW_GUTTER_WIDTH_PX: f32 = 52.0;
const RESULT_COL_WIDTH_PX: f32 = 160.0;
const OVERSCAN_ROWS: usize = 6;

/// Persistent UI state for the SQL execution results pane.
#[derive(Debug)]
pub struct ResultsPaneState {
    pub height_px: Option<f32>,
    pub scroll_handle: ScrollHandle,
    pub h_scroll_offset_px: f32,
    pub selected_cell: Option<(usize, usize)>,
    pub resizing_sash: bool,
    pub drag_start_y: Option<f32>,
    pub drag_start_height: f32,
    pub active_cancel: Option<tokio_util::sync::CancellationToken>,
}

impl Default for ResultsPaneState {
    fn default() -> Self {
        Self {
            height_px: None,
            scroll_handle: ScrollHandle::new(),
            h_scroll_offset_px: 0.0,
            selected_cell: None,
            resizing_sash: false,
            drag_start_y: None,
            drag_start_height: DEFAULT_RESULTS_HEIGHT_PX,
            active_cancel: None,
        }
    }
}

impl ResultsPaneState {
    /// Initializes results pane state for the workspace.
    pub fn new(_window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) -> Self {
        Self::default()
    }
}

/// Renders the execution results & messages pane anchored below the SQL editor.
pub fn render_results_pane(
    ws: &mut DataForgeWorkspace,
    window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let height = ws
        .results_ui
        .height_px
        .unwrap_or(DEFAULT_RESULTS_HEIGHT_PX)
        .clamp(MIN_RESULTS_HEIGHT_PX, MAX_RESULTS_HEIGHT_PX);
    let inner = render_results_pane_body(ws, window, cx);
    let sash = render_top_resize_sash(ws, height, cx);

    div()
        .id("results-pane-root")
        .w_full()
        .h(px(height))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .bg(theme::panel_surface_bg(cx))
        .on_mouse_move(cx.listener(|ws, event: &gpui::MouseMoveEvent, _w, cx| {
            if ws.results_ui.resizing_sash
                && let Some(start_y) = ws.results_ui.drag_start_y
            {
                let delta = start_y - f32::from(event.position.y);
                let next = (ws.results_ui.drag_start_height + delta)
                    .clamp(MIN_RESULTS_HEIGHT_PX, MAX_RESULTS_HEIGHT_PX);
                ws.results_ui.height_px = Some(next);
                cx.notify();
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|ws, _event, _w, cx| {
                if ws.results_ui.resizing_sash {
                    ws.results_ui.resizing_sash = false;
                    ws.results_ui.drag_start_y = None;
                    cx.notify();
                }
            }),
        )
        .child(sash)
        .child(div().flex_1().min_h_0().w_full().child(inner))
        .into_any_element()
}

/// Renders results content filling its parent container (for embedding inside `ResizablePanel`).
pub(crate) fn render_results_pane_embedded(
    ws: &mut DataForgeWorkspace,
    window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    div()
        .id("results-pane-embedded")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme::panel_surface_bg(cx))
        .border_t_1()
        .border_color(theme::hairline_border_color(cx))
        .child(render_results_pane_body(ws, window, cx))
        .into_any_element()
}

fn render_top_resize_sash(
    ws: &mut DataForgeWorkspace,
    current_height: f32,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let is_dragging = ws.results_ui.resizing_sash;
    let border = theme::hairline_border_color(cx);
    let accent = cx.colors().accent.color;

    div()
        .id("results-pane-sash")
        .relative()
        .w_full()
        .h(px(6.0))
        .flex_shrink_0()
        .cursor_row_resize()
        .flex()
        .items_center()
        .child(
            div()
                .w_full()
                .h(px(1.0))
                .bg(if is_dragging { accent } else { border }),
        )
        .child(
            gpui::canvas(
                |bounds, window, _| window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal),
                move |_, hitbox, window, _| {
                    if is_dragging {
                        window.capture_pointer(hitbox.id);
                    }
                },
            )
            .absolute()
            .inset_0(),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, event: &gpui::MouseDownEvent, _w, cx| {
                ws.results_ui.resizing_sash = true;
                ws.results_ui.drag_start_y = Some(f32::from(event.position.y));
                ws.results_ui.drag_start_height = current_height;
                cx.stop_propagation();
                cx.notify();
            }),
        )
        .into_any_element()
}

fn render_results_pane_body(
    ws: &mut DataForgeWorkspace,
    window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let active_tab = ws.controller.tabs.active_tab();
    let result_snapshot = active_tab.and_then(|id| ws.tab_results.get(&id).cloned());

    let Some(res) = result_snapshot else {
        return render_honest_empty_results(ws, cx);
    };

    let has_any_output = res.running
        || res.active_error.is_some()
        || !res.accumulator.result_sets.is_empty()
        || !res.accumulator.notices.is_empty()
        || !res.accumulator.commands_completed.is_empty()
        || res.accumulator.summary.is_some();

    if !has_any_output {
        return render_honest_empty_results(ws, cx);
    }

    let header = render_results_header(ws, &res, cx);
    let progress_strip = if res.running {
        Some(
            div()
                .w_full()
                .h(px(2.0))
                .child(
                    ProgressBar::new("results-streaming-progress")
                        .is_indeterminate(true)
                        .size(HeroSize::Sm)
                        .color(HeroColor::Accent),
                )
                .into_any_element(),
        )
    } else {
        None
    };

    let content = match res.sub_view {
        ResultPaneSubView::Results => {
            if let Some(set) = res.accumulator.result_sets.get(res.active_result_set) {
                render_result_set_grid(ws, set, window, cx)
            } else if res.active_error.is_some()
                || !res.accumulator.notices.is_empty()
                || !res.accumulator.commands_completed.is_empty()
            {
                render_messages_view(&res, ws, cx)
            } else if res.running {
                render_running_placeholder(ws, cx)
            } else {
                render_honest_empty_results(ws, cx)
            }
        }
        ResultPaneSubView::Messages => render_messages_view(&res, ws, cx),
        ResultPaneSubView::Summary => render_summary_view(&res, ws, cx),
    };

    div()
        .size_full()
        .flex()
        .flex_col()
        .child(header)
        .children(progress_strip)
        .child(div().flex_1().min_h_0().w_full().child(content))
        .into_any_element()
}

fn render_honest_empty_results(ws: &DataForgeWorkspace, cx: &App) -> AnyElement {
    let muted = cx.colors().muted;
    let border = theme::hairline_border_color(cx);

    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(theme::muted_surface_bg(cx))
        .child(
            div()
                .h(px(30.0))
                .px(px(12.0))
                .border_b_1()
                .border_color(border)
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    Icon::new(IconName::Terminal)
                        .size(IconSize::Xs)
                        .color(muted),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(muted)
                        .child(ws.controller.strings.result_prefix),
                ),
        )
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .px(px(16.0))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(cx.colors().foreground)
                        .child(ws.controller.strings.results_empty_title),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(muted)
                        .child(ws.controller.strings.results_empty_desc),
                ),
        )
        .into_any_element()
}

fn render_running_placeholder(ws: &DataForgeWorkspace, cx: &App) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .child(Spinner::new("results-wait-spinner").size(SpinnerSize::Sm))
        .child(
            div()
                .text_size(px(12.0))
                .text_color(cx.colors().muted)
                .child(ws.controller.strings.executing_awaiting),
        )
        .into_any_element()
}

fn render_results_header(
    ws: &mut DataForgeWorkspace,
    res: &QueryTabResultState,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let tab_id = res.tab_id;
    let mut tab_items = Vec::new();
    for (idx, set) in res.accumulator.result_sets.iter().enumerate() {
        let key = format!("result-{idx}");
        let label = format!("{} {} ({})", s.result_prefix, idx + 1, set.rows.len());
        tab_items.push(TabItem::new(key, label));
    }
    let msg_count = res.accumulator.notices.len()
        + res.accumulator.commands_completed.len()
        + usize::from(res.active_error.is_some());
    tab_items.push(TabItem::new(
        "messages",
        if msg_count > 0 {
            format!("{} ({msg_count})", s.messages)
        } else {
            s.messages.to_owned()
        },
    ));
    tab_items.push(TabItem::new("summary", s.summary));

    let selected_key: SharedString = match res.sub_view {
        ResultPaneSubView::Results if !res.accumulator.result_sets.is_empty() => {
            let idx = res
                .active_result_set
                .min(res.accumulator.result_sets.len().saturating_sub(1));
            format!("result-{idx}").into()
        }
        ResultPaneSubView::Results | ResultPaneSubView::Messages => "messages".into(),
        ResultPaneSubView::Summary => "summary".into(),
    };

    let entity = cx.entity().clone();
    let tabs = Tabs::new("results-subtabs", tab_items, selected_key.clone())
        .selected_key(selected_key)
        .variant(TabsVariant::Secondary)
        .size(TabsSize::Sm)
        .on_selection_change(move |key, _w, cx| {
            let key_str = key.to_string();
            entity.update(cx, |ws, cx| {
                let target = ws.ensure_tab_result(tab_id);
                if let Some(rest) = key_str.strip_prefix("result-")
                    && let Ok(idx) = rest.parse::<usize>()
                {
                    target.sub_view = ResultPaneSubView::Results;
                    target.active_result_set = idx;
                } else if key_str == "messages" {
                    target.sub_view = ResultPaneSubView::Messages;
                } else if key_str == "summary" {
                    target.sub_view = ResultPaneSubView::Summary;
                }
                ws.results_ui.selected_cell = None;
                ws.results_ui.h_scroll_offset_px = 0.0;
                ws.results_ui.scroll_handle.scroll_to_item(0);
                cx.notify();
            });
        });

    let badges_and_actions = render_results_header_right(ws, res, cx);

    div()
        .h(px(36.0))
        .w_full()
        .px(px(10.0))
        .flex_shrink_0()
        .border_b_1()
        .border_color(theme::hairline_border_color(cx))
        .bg(theme::panel_surface_bg(cx))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .child(div().min_w_0().overflow_hidden().child(tabs))
        .child(badges_and_actions)
        .into_any_element()
}

fn render_results_header_right(
    ws: &DataForgeWorkspace,
    res: &QueryTabResultState,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let mut row = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .flex_shrink_0()
        .font_features(tabular_nums_features());

    if res.running {
        return render_running_header_controls(row, ws, cx);
    }

    row = append_result_status_chips(row, ws, res);
    append_result_action_buttons(row, ws, res, cx).into_any_element()
}

fn render_running_header_controls(
    row: gpui::Div,
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    row.child(Spinner::new("results-running-spin").size(SpinnerSize::Sm))
        .child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .size(HeroSize::Sm)
                .color(HeroColor::Accent)
                .child(ChipLabel::new().child(s.running)),
        )
        .child(
            Button::new("results-cancel-query")
                .variant(Variant::Danger)
                .size(HeroSize::Sm)
                .label(s.cancel_label)
                .on_press(cx.listener(|ws, _ev, _w, cx| {
                    if let Some(cancel) = ws.results_ui.active_cancel.take() {
                        cancel.cancel();
                    }
                    if let Some(tab_id) = ws.controller.tabs.active_tab() {
                        let cancel_msg = ws.controller.strings.query_cancelled_msg.to_owned();
                        let tab_res = ws.ensure_tab_result(tab_id);
                        tab_res.running = false;
                        if tab_res.active_error.is_none() {
                            tab_res.active_error = Some(cancel_msg);
                        }
                    }
                    cx.notify();
                })),
        )
        .into_any_element()
}

fn append_result_status_chips(
    mut row: gpui::Div,
    ws: &DataForgeWorkspace,
    res: &QueryTabResultState,
) -> gpui::Div {
    let s = &ws.controller.strings;
    if res.active_error.is_some() {
        row = row.child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .size(HeroSize::Sm)
                .color(HeroColor::Danger)
                .child(ChipLabel::new().child(s.failed)),
        );
    }

    if let Some(ms) = res.elapsed_ms {
        row = row.child(
            Chip::new()
                .variant(ChipVariant::Secondary)
                .size(HeroSize::Sm)
                .child(ChipLabel::new().child(format_duration_ms(ms))),
        );
    }

    if let Some(set) = res.accumulator.result_sets.get(res.active_result_set)
        && res.sub_view == ResultPaneSubView::Results
    {
        let row_label = match set.total_rows {
            Some(total) if total > set.retained => {
                format!("{} / {} {}", set.rows.len(), total, s.rows_suffix)
            }
            _ => format!("{} {}", set.rows.len(), s.rows_suffix),
        };
        row = row.child(
            Chip::new()
                .variant(ChipVariant::Secondary)
                .size(HeroSize::Sm)
                .child(ChipLabel::new().child(row_label)),
        );
        if set.row_limit_reached
            || set.byte_limit_reached
            || matches!(
                set.truncation,
                Some(Truncation::ReadPastLimit | Truncation::ServerStopped | Truncation::ByteLimit)
            )
        {
            row = row.child(
                Chip::new()
                    .variant(ChipVariant::Soft)
                    .size(HeroSize::Sm)
                    .color(HeroColor::Warning)
                    .child(ChipLabel::new().child(s.truncated)),
            );
        }
    }
    row
}

fn append_result_action_buttons(
    mut row: gpui::Div,
    ws: &DataForgeWorkspace,
    res: &QueryTabResultState,
    cx: &mut Context<DataForgeWorkspace>,
) -> gpui::Div {
    let s = &ws.controller.strings;
    let has_active_set = res
        .accumulator
        .result_sets
        .get(res.active_result_set)
        .is_some();
    if has_active_set || ws.results_ui.selected_cell.is_some() {
        let active_set_idx = res.active_result_set;
        row = row.child(
            Button::new("results-copy-btn")
                .variant(Variant::Tertiary)
                .size(HeroSize::Sm)
                .child(Icon::new(IconName::Copy).size(IconSize::Xs))
                .label(if ws.results_ui.selected_cell.is_some() {
                    s.copy_cell
                } else {
                    s.copy_tsv
                })
                .on_press(cx.listener(move |ws, _ev, _w, cx| {
                    copy_results_to_clipboard(ws, active_set_idx, cx);
                })),
        );
    }
    if has_active_set {
        let active_set_idx = res.active_result_set;
        row = row.child(
            Button::new("results-export-btn")
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .child(Icon::new(IconName::Download).size(IconSize::Xs))
                .label(s.export)
                .on_press(cx.listener(move |ws, _ev, window, cx| {
                    sync_result_set_for_export(ws, active_set_idx);
                    ws.modals_ui.open_export(window, cx);
                    cx.notify();
                })),
        );
    }
    row
}

#[derive(Clone, Copy)]
struct ResultGridColors {
    hdr_bg: Hsla,
    sel_bg: Hsla,
    sel_fg: Hsla,
    focus_border: Hsla,
    text_fg: Hsla,
    muted: Hsla,
    border: Hsla,
}

fn render_result_set_grid(
    ws: &mut DataForgeWorkspace,
    set: &AccumulatedResultSet,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let cols = &set.columns;
    let total_rows = set.rows.len();
    if cols.is_empty() && total_rows == 0 {
        return div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.0))
            .text_color(cx.colors().muted)
            .child(ws.controller.strings.results_zero_cols_rows)
            .into_any_element();
    }

    let font = theme::value_gpui_font(&ws.controller.settings);
    let font_size = ws.controller.settings.value_font.size.max(10.0);
    let bg = ws.content_palette.window_background;
    let colors = ResultGridColors {
        hdr_bg: ws.content_palette.data_grid_header_background,
        sel_bg: ws.content_palette.data_grid_selection,
        sel_fg: ws.content_palette.data_grid_selection_text,
        focus_border: ws.content_palette.data_grid_focus_border,
        text_fg: ws.content_palette.editor_text,
        muted: cx.colors().muted,
        border: theme::hairline_border_color(cx),
    };

    let scroll_handle = ws.results_ui.scroll_handle.clone();
    let offset = scroll_handle.offset();
    let viewport = scroll_handle.bounds().size;
    let view_h = f32::from(viewport.height).max(240.0);
    let view_w = f32::from(viewport.width).max(720.0);
    let scroll_y = (-f32::from(offset.y)).max(0.0);
    let scroll_x = ws.results_ui.h_scroll_offset_px.max(0.0);

    let visible_start = ((scroll_y / ROW_HEIGHT_PX).floor() as usize).saturating_sub(OVERSCAN_ROWS);
    let visible_count = (view_h / ROW_HEIGHT_PX).ceil() as usize + OVERSCAN_ROWS * 2;
    let visible_end = (visible_start + visible_count).min(total_rows);
    let col_slice = compute_visible_col_slice(
        cols.len(),
        scroll_x,
        (view_w - ROW_GUTTER_WIDTH_PX).max(120.0),
    );

    let header_row = render_result_grid_header(
        cols,
        col_slice.0,
        col_slice.1,
        col_slice.2,
        col_slice.3,
        colors.hdr_bg,
        ws.content_palette.data_grid_header_text,
        colors.muted,
        colors.border,
    );
    let body_rows = render_result_grid_body(
        set,
        visible_start..visible_end,
        col_slice,
        ws.results_ui.selected_cell,
        colors,
        cx,
    );

    let total_cols_width = cols.len() as f32 * RESULT_COL_WIDTH_PX + ROW_GUTTER_WIDTH_PX;
    let max_h_scroll = (total_cols_width - view_w).max(0.0);

    wrap_result_set_grid_container(
        bg,
        font,
        font_size,
        max_h_scroll,
        scroll_handle,
        header_row,
        body_rows,
        cx,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "grid scroll container wires resolved theme font, scroll handle, and header/body slots"
)]
fn wrap_result_set_grid_container(
    bg: Hsla,
    font: herogpui::gpui::Font,
    font_size: f32,
    max_h_scroll: f32,
    scroll_handle: ScrollHandle,
    header_row: AnyElement,
    body_rows: AnyElement,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    div()
        .id("results-grid-container")
        .relative()
        .size_full()
        .flex()
        .flex_col()
        .bg(bg)
        .font(font)
        .text_size(px(font_size))
        .font_features(tabular_nums_features())
        .on_scroll(cx.listener(move |ws, ev: &gpui::ScrollEvent, _w, cx| {
            let delta = ev.delta.pixel_delta(px(ROW_HEIGHT_PX));
            let dx = f32::from(delta.x);
            if dx.abs() > 0.5 || ev.modifiers.shift {
                let shift_dx = if dx.abs() > 0.5 {
                    dx
                } else {
                    f32::from(delta.y)
                };
                ws.results_ui.h_scroll_offset_px =
                    (ws.results_ui.h_scroll_offset_px - shift_dx).clamp(0.0, max_h_scroll);
                cx.notify();
            }
        }))
        .child(header_row)
        .child(
            div()
                .id("results-grid-scroll")
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_y_scroll()
                .track_scroll(&scroll_handle)
                .child(body_rows),
        )
        .child(
            div()
                .absolute()
                .top(px(HEADER_HEIGHT_PX))
                .bottom_0()
                .right_0()
                .w(px(10.0))
                .child(
                    Scrollbar::new("results-grid-vbar", scroll_handle)
                        .orientation(HeroOrientation::Vertical),
                ),
        )
        .into_any_element()
}

fn render_result_grid_body(
    set: &AccumulatedResultSet,
    row_range: std::ops::Range<usize>,
    col_slice: (usize, usize, f32, f32),
    selected_cell: Option<(usize, usize)>,
    colors: ResultGridColors,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let top_spacer_h = row_range.start as f32 * ROW_HEIGHT_PX;
    let bottom_spacer_h = set.rows.len().saturating_sub(row_range.end) as f32 * ROW_HEIGHT_PX;

    let mut body_rows = div().w_full().flex().flex_col();
    if top_spacer_h > 0.0 {
        body_rows = body_rows.child(div().w_full().h(px(top_spacer_h)));
    }
    for row_idx in row_range {
        if let Some(row_vals) = set.rows.get(row_idx) {
            body_rows = body_rows.child(render_result_grid_row(
                row_idx,
                row_vals,
                col_slice,
                selected_cell,
                colors,
                cx,
            ));
        }
    }
    if bottom_spacer_h > 0.0 {
        body_rows = body_rows.child(div().w_full().h(px(bottom_spacer_h)));
    }
    body_rows.into_any_element()
}

#[expect(
    clippy::too_many_arguments,
    reason = "virtualized result header needs slice bounds, widths, and resolved palette colors"
)]
fn render_result_grid_header(
    cols: &[tablepro_core::column::ResultColumn],
    col_start: usize,
    col_end: usize,
    left_pad_w: f32,
    right_pad_w: f32,
    hdr_bg: Hsla,
    hdr_fg: Hsla,
    muted: Hsla,
    border: Hsla,
) -> AnyElement {
    let mut row = div()
        .w_full()
        .h(px(HEADER_HEIGHT_PX))
        .flex_shrink_0()
        .bg(hdr_bg)
        .border_b_1()
        .border_color(border)
        .flex()
        .items_center()
        .overflow_hidden()
        .child(
            div()
                .w(px(ROW_GUTTER_WIDTH_PX))
                .h_full()
                .flex_shrink_0()
                .px(px(8.0))
                .border_r_1()
                .border_color(border)
                .flex()
                .items_center()
                .justify_end()
                .text_size(px(11.0))
                .text_color(muted)
                .child("#"),
        );

    if left_pad_w > 0.0 {
        row = row.child(div().w(px(left_pad_w)).h_full().flex_shrink_0());
    }
    for col in &cols[col_start..col_end] {
        row = row.child(
            div()
                .w(px(RESULT_COL_WIDTH_PX))
                .h_full()
                .flex_shrink_0()
                .px(px(8.0))
                .border_r_1()
                .border_color(border)
                .flex()
                .items_center()
                .justify_between()
                .gap(px(4.0))
                .overflow_hidden()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_size(px(12.0))
                        .text_color(hdr_fg)
                        .child(col.name.clone()),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(10.0))
                        .text_color(muted)
                        .child(col.column_type.name.clone()),
                ),
        );
    }
    if right_pad_w > 0.0 {
        row = row.child(div().w(px(right_pad_w)).h_full().flex_shrink_0());
    }
    row.into_any_element()
}

fn render_result_grid_row(
    row_idx: usize,
    row_vals: &[Value],
    col_slice: (usize, usize, f32, f32),
    selected_cell: Option<(usize, usize)>,
    colors: ResultGridColors,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let (col_start, col_end, left_pad_w, right_pad_w) = col_slice;
    let zebra = theme::row_zebra_bg(row_idx, cx);
    let mut row = div()
        .id(("result-row", row_idx))
        .w_full()
        .h(px(ROW_HEIGHT_PX))
        .flex_shrink_0()
        .border_b_1()
        .border_color(colors.border)
        .when_some(zebra, |el, bg_tint| el.bg(bg_tint))
        .flex()
        .items_center()
        .overflow_hidden()
        .child(
            div()
                .w(px(ROW_GUTTER_WIDTH_PX))
                .h_full()
                .flex_shrink_0()
                .px(px(8.0))
                .bg(colors.hdr_bg)
                .border_r_1()
                .border_color(colors.border)
                .flex()
                .items_center()
                .justify_end()
                .text_size(px(11.0))
                .text_color(colors.muted)
                .child(format!("{}", row_idx + 1)),
        );

    if left_pad_w > 0.0 {
        row = row.child(div().w(px(left_pad_w)).h_full().flex_shrink_0());
    }
    for col_idx in col_start..col_end {
        let val = row_vals.get(col_idx).unwrap_or(&Value::Null);
        let is_selected = selected_cell == Some((row_idx, col_idx));
        row = row.child(render_result_grid_cell(
            row_idx,
            col_idx,
            val,
            is_selected,
            colors,
            cx,
        ));
    }
    if right_pad_w > 0.0 {
        row = row.child(div().w(px(right_pad_w)).h_full().flex_shrink_0());
    }
    row.into_any_element()
}

fn render_result_grid_cell(
    row_idx: usize,
    col_idx: usize,
    val: &Value,
    is_selected: bool,
    colors: ResultGridColors,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let is_num = val.is_number();
    let cell_id = row_idx.saturating_mul(10_000).saturating_add(col_idx);

    let content: AnyElement = if val.is_null() {
        Chip::new()
            .variant(ChipVariant::Secondary)
            .size(HeroSize::Sm)
            .child(
                ChipLabel::new()
                    .sx(|s| s.italic().text_size(px(10.0)))
                    .child("NULL"),
            )
            .into_any_element()
    } else {
        div()
            .w_full()
            .truncate()
            .when(is_num, |el| el.text_right())
            .text_color(if is_selected {
                colors.sel_fg
            } else {
                colors.text_fg
            })
            .child(format_core_value(val))
            .into_any_element()
    };

    div()
        .id(("result-cell", cell_id))
        .w(px(RESULT_COL_WIDTH_PX))
        .h_full()
        .flex_shrink_0()
        .px(px(8.0))
        .border_r_1()
        .border_color(colors.border)
        .when(is_selected, |el| {
            el.bg(colors.sel_bg)
                .border_1()
                .border_color(colors.focus_border)
        })
        .flex()
        .items_center()
        .overflow_hidden()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, event: &gpui::MouseDownEvent, _w, cx| {
                ws.results_ui.selected_cell = Some((row_idx, col_idx));
                if event.click_count >= 2
                    && let Some(tab_id) = ws.controller.tabs.active_tab()
                    && let Some(tab_res) = ws.tab_results.get(&tab_id)
                    && let Some(set) = tab_res
                        .accumulator
                        .result_sets
                        .get(tab_res.active_result_set)
                    && let Some(cell_val) = set.rows.get(row_idx).and_then(|r| r.get(col_idx))
                {
                    let text = format_core_value(cell_val);
                    let toast_msg = ws.controller.strings.cell_copied_toast;
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    ws.notify_toast(ToastSeverity::Info, toast_msg, None, cx);
                }
                cx.notify();
            }),
        )
        .child(content)
        .into_any_element()
}

fn render_messages_view(
    res: &QueryTabResultState,
    ws: &DataForgeWorkspace,
    cx: &App,
) -> AnyElement {
    let s = &ws.controller.strings;
    let font = theme::value_gpui_font(&ws.controller.settings);
    let border = theme::hairline_border_color(cx);
    let mut stack = div()
        .id("results-messages-scroll")
        .size_full()
        .overflow_y_scroll()
        .p(px(12.0))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .font(font)
        .font_features(tabular_nums_features());

    if let Some(err) = &res.active_error {
        stack = stack.child(
            Alert::new(s.query_error_title)
                .status(HeroColor::Danger)
                .description(err.clone()),
        );
    }

    for (idx, rows_affected) in res.accumulator.commands_completed.iter().enumerate() {
        let msg = match rows_affected {
            Some(n) => format!(
                "{} {} — {} ({n} {})",
                s.statement_prefix,
                idx + 1,
                s.status_completed,
                s.rows_affected_suffix
            ),
            None => format!("{} {} — {}", s.statement_prefix, idx + 1, s.status_completed),
        };
        stack = stack.child(render_command_completed_banner(s.status_ok, msg, border, cx));
    }

    for notice in &res.accumulator.notices {
        stack = stack.child(render_server_notice_banner(notice, border, cx));
    }

    if let Some(retained) = res.accumulator.notice_limit_reached {
        stack = stack.child(
            Alert::new(s.notice_limit_title)
                .status(HeroColor::Warning)
                .description(format!("{}: {retained}", s.notice_limit_retained_prefix)),
        );
    }

    if res.active_error.is_none()
        && res.accumulator.commands_completed.is_empty()
        && res.accumulator.notices.is_empty()
    {
        stack = stack.child(
            div()
                .text_size(px(12.0))
                .text_color(cx.colors().muted)
                .child(s.no_server_notices),
        );
    }

    stack.into_any_element()
}

fn render_command_completed_banner(
    ok_label: &'static str,
    msg: String,
    border: Hsla,
    cx: &App,
) -> AnyElement {
    div()
        .px(px(10.0))
        .py(px(6.0))
        .rounded(px(theme::CONTROL_RADIUS_PX))
        .border_1()
        .border_color(border)
        .bg(theme::muted_surface_bg(cx))
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .size(HeroSize::Sm)
                .color(HeroColor::Success)
                .child(ChipLabel::new().child(ok_label)),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(cx.colors().foreground)
                .child(msg),
        )
        .into_any_element()
}

fn render_server_notice_banner(
    notice: &tablepro_core::ServerNotice,
    border: Hsla,
    cx: &App,
) -> AnyElement {
    let is_warn = notice.severity.to_ascii_uppercase().contains("WARN");
    let color = if is_warn {
        HeroColor::Warning
    } else {
        HeroColor::Default
    };
    div()
        .px(px(10.0))
        .py(px(6.0))
        .rounded(px(theme::CONTROL_RADIUS_PX))
        .border_1()
        .border_color(border)
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .size(HeroSize::Sm)
                .color(color)
                .child(ChipLabel::new().child(notice.severity.clone())),
        )
        .when_some(notice.code.as_ref(), |el, code| {
            el.child(
                div()
                    .text_size(px(11.0))
                    .text_color(cx.colors().muted)
                    .child(format!("[{code}]")),
            )
        })
        .child(
            div()
                .flex_1()
                .text_size(px(12.0))
                .text_color(cx.colors().foreground)
                .child(notice.message.clone()),
        )
        .into_any_element()
}

fn render_summary_view(res: &QueryTabResultState, ws: &DataForgeWorkspace, cx: &App) -> AnyElement {
    let s = &ws.controller.strings;
    let font = theme::value_gpui_font(&ws.controller.settings);
    let total_rows: usize = res
        .accumulator
        .result_sets
        .iter()
        .map(|s| s.rows.len())
        .sum();
    let elapsed = res
        .elapsed_ms
        .map(format_duration_ms)
        .unwrap_or_else(|| s.in_progress_label.to_owned());
    let status = if res.running {
        s.status_running_label
    } else if res.active_error.is_some() {
        s.failed
    } else {
        s.status_completed
    };
    let tx_label = res
        .accumulator
        .summary
        .as_ref()
        .map(|summary| match summary.transaction_state {
            TransactionState::Idle => s.tx_idle_autocommit,
            TransactionState::InTransaction => s.tx_in_transaction,
            TransactionState::Failed => s.tx_aborted,
            TransactionState::Unknown => s.tx_unknown,
        })
        .unwrap_or(s.tx_unknown);

    let pairs = [
        (s.summary_status, status.to_owned()),
        (s.summary_elapsed, elapsed),
        (
            s.summary_result_sets,
            res.accumulator.result_sets.len().to_string(),
        ),
        (s.summary_retained_rows, total_rows.to_string()),
        (
            s.summary_statements_completed,
            res.accumulator.commands_completed.len().to_string(),
        ),
        (
            s.summary_server_notices,
            res.accumulator.notices.len().to_string(),
        ),
        (s.summary_tx_state, tx_label.to_owned()),
    ];

    let border = theme::hairline_border_color(cx);
    let mut table = div()
        .id("results-summary-scroll")
        .size_full()
        .overflow_y_scroll()
        .p(px(12.0))
        .font(font)
        .font_features(tabular_nums_features())
        .flex()
        .flex_col()
        .gap(px(2.0));

    for (k, v) in pairs {
        table = table.child(
            div()
                .h(px(28.0))
                .px(px(10.0))
                .border_b_1()
                .border_color(border)
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(cx.colors().muted)
                        .child(k),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(cx.colors().foreground)
                        .child(v),
                ),
        );
    }

    table.into_any_element()
}

fn compute_visible_col_slice(
    total_cols: usize,
    scroll_x: f32,
    viewport_w: f32,
) -> (usize, usize, f32, f32) {
    if total_cols == 0 {
        return (0, 0, 0.0, 0.0);
    }
    let start = ((scroll_x / RESULT_COL_WIDTH_PX).floor() as usize).min(total_cols);
    let visible = (viewport_w / RESULT_COL_WIDTH_PX).ceil() as usize + 2;
    let end = (start + visible).min(total_cols);
    let left_pad = start as f32 * RESULT_COL_WIDTH_PX;
    let right_pad = total_cols.saturating_sub(end) as f32 * RESULT_COL_WIDTH_PX;
    (start, end, left_pad, right_pad)
}

fn copy_results_to_clipboard(
    ws: &mut DataForgeWorkspace,
    active_set_idx: usize,
    cx: &mut Context<DataForgeWorkspace>,
) {
    let Some(tab_id) = ws.controller.tabs.active_tab() else {
        return;
    };
    let Some(res) = ws.tab_results.get(&tab_id) else {
        return;
    };
    let Some(set) = res.accumulator.result_sets.get(active_set_idx) else {
        return;
    };
    if let Some((r, c)) = ws.results_ui.selected_cell
        && let Some(val) = set.rows.get(r).and_then(|row| row.get(c))
    {
        let text = if val.is_null() {
            "NULL".to_owned()
        } else {
            format_core_value(val)
        };
        let toast_msg = ws.controller.strings.cell_copied_toast;
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        ws.notify_toast(ToastSeverity::Info, toast_msg, None, cx);
        return;
    }

    let mut out = String::new();
    for (idx, col) in set.columns.iter().enumerate() {
        if idx > 0 {
            out.push('\t');
        }
        out.push_str(&col.name);
    }
    for row in &set.rows {
        out.push('\n');
        for (idx, val) in row.iter().enumerate() {
            if idx > 0 {
                out.push('\t');
            }
            if val.is_null() {
                out.push_str("NULL");
            } else {
                out.push_str(&format_core_value(val));
            }
        }
    }
    cx.write_to_clipboard(ClipboardItem::new_string(out));
    let s = &ws.controller.strings;
    ws.notify_toast(
        ToastSeverity::Success,
        format!("{}: {} {} (TSV)", s.copied, set.rows.len(), s.rows_suffix),
        None,
        cx,
    );
}

fn sync_result_set_for_export(ws: &mut DataForgeWorkspace, active_set_idx: usize) {
    let Some(tab_id) = ws.controller.tabs.active_tab() else {
        return;
    };
    let Some(res) = ws.tab_results.get(&tab_id) else {
        return;
    };
    let Some(set) = res.accumulator.result_sets.get(active_set_idx) else {
        return;
    };
    ws.controller.grid_raw_columns = set.columns.to_vec();
    ws.controller.grid_raw_rows = set.rows.clone();
}

pub(crate) fn format_core_value(val: &Value) -> String {
    match val {
        Value::Null => "NULL".to_owned(),
        Value::Bool(b) => if *b { "true" } else { "false" }.to_owned(),
        Value::Int32(i) => i.to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::WideInt(w) => w.to_string(),
        Value::Float32(f) => f.to_string(),
        Value::Float64(f) => f.to_string(),
        Value::Decimal(d) => d.to_string(),
        Value::Text(s) => s.clone(),
        Value::Bytes(bytes) => format!("<{} bytes>", bytes.len()),
        Value::Uuid(u) => u.to_string(),
        Value::Json(j) => j.as_str().to_owned(),
        Value::Date(Temporal::Finite(d)) => d.to_string(),
        Value::Date(Temporal::Infinity) => "infinity".to_owned(),
        Value::Date(Temporal::NegInfinity) => "-infinity".to_owned(),
        Value::Time(t) => t.format(None),
        Value::TimeTz(t) => format!("{}{}", t.time.format(None), t.offset),
        Value::Timestamp(Temporal::Finite(ts)) => ts.format("%Y-%m-%d %H:%M:%S%.f").to_string(),
        Value::Timestamp(Temporal::Infinity) => "infinity".to_owned(),
        Value::Timestamp(Temporal::NegInfinity) => "-infinity".to_owned(),
        Value::TimestampTz(Temporal::Finite(ts)) => ts.to_string(),
        Value::TimestampTz(Temporal::Infinity) => "infinity".to_owned(),
        Value::TimestampTz(Temporal::NegInfinity) => "-infinity".to_owned(),
        Value::Interval(i) => format!("{i}"),
        Value::Bits(b) => format!("{b}"),
        Value::Array(arr) => {
            let parts: Vec<String> = arr.iter().map(format_core_value).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Other(other) => other.text.clone(),
        Value::Undecodable(_) => "<undecodable>".to_owned(),
    }
}

fn format_duration_ms(ms: u64) -> String {
    if ms >= 1000 {
        format!("{:.2} s", ms as f64 / 1000.0)
    } else {
        format!("{ms} ms")
    }
}

fn tabular_nums_features() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".to_owned(), 1)]))
}
