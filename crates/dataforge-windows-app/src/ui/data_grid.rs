//! High-performance 2D-virtualized interactive Data Grid backed by
//! [`DataGridModel`], [`GridSelectionReducer`], and [`TableEdits`].
//!
//! Features uniform vertical row slicing, horizontal column slicing, pinned
//! `#` gutter + frozen leading columns (clamped `<= 2/3` viewport width),
//! draggable resize handles, column visibility & filter popovers, inline cell
//! editing, clickable foreign-key navigation chips, pagination, and context menus.

use std::collections::BTreeSet;
use std::sync::Arc;

use dataforge_gfx::grid_renderer::{CellKind, GridRow, RowState};
use dataforge_state::connection_window::ConnectionWindowPhase;
use dataforge_state::data_grid::{
    DataGridColumn, DataGridColumnID, GridCoord, GridSelectionAction, MAX_COLUMN_WIDTH,
    MIN_COLUMN_WIDTH, SortDirection as ModelSortDir,
};
use herogpui::components::{
    Button, Checkbox, Chip, ChipLabel, ChipVariant, ContextMenu, Dropdown, DropdownPlacement, Icon,
    IconName, IconSize, Input, InputState, MenuItem, Pagination, PickerItem, Popover,
    PopoverPlacement, Scrollbar, Select,
};
use herogpui::core::{
    Color as HeroColor, Orientation as HeroOrientation, Size as HeroSize, Variant, with_alpha,
};
use herogpui::gpui::{
    self, AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, FocusHandle, Focusable,
    FontFeatures, FontWeight, Hsla, InteractiveElement, IntoElement, MouseButton, ParentElement,
    ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use herogpui::prelude::FluentBuilder;
use herogpui::theme::ActiveTheme;
use tablepro_core::filter::FilterOp;
use tablepro_core::foreign_key_info::ForeignKeyInfo;
use tablepro_core::meta::{PagePosition, TableRef};
use tablepro_core::value::Value;

use super::results_pane::format_core_value;
use super::shell_state::{InlineCellEditState, ToastSeverity};
use super::structure_view::StructureViewState;
use super::{DataForgeWorkspace, theme};

const ROW_HEIGHT_PX: f32 = 28.0;
const HEADER_HEIGHT_PX: f32 = 32.0;
const ROW_GUTTER_WIDTH_PX: f32 = 54.0;
const OVERSCAN_ROWS: usize = 6;

type GridColumnSplit = (Vec<(usize, DataGridColumn)>, Vec<(usize, DataGridColumn)>);

/// Persistent UI state for the interactive 2D-virtualized Data Grid.
#[derive(Debug)]
pub struct DataGridViewState {
    pub scroll_row: usize,
    pub scroll_handle: ScrollHandle,
    pub h_scroll_offset_px: f32,
    pub focus_handle: FocusHandle,
    pub hidden_columns: BTreeSet<DataGridColumnID>,
    pub columns_popover_open: bool,
    pub filters_popover_open: bool,
    pub filter_col_idx: usize,
    pub filter_op: FilterOp,
    pub filter_value_input: Entity<InputState>,
    pub inline_input: Entity<InputState>,
    pub inline_focus_requested: Option<(usize, usize)>,
    pub resizing_col_idx: Option<usize>,
    pub resize_start_x: f32,
    pub resize_start_width: f32,
    pub dragging_selection: bool,
}

impl DataGridViewState {
    /// Creates initial Data Grid UI state with HeroGPUI input entities.
    pub fn new(_window: &mut Window, cx: &mut Context<DataForgeWorkspace>) -> Self {
        let filter_value_input = cx.new(|cx| InputState::new(cx));
        let inline_input = cx.new(|cx| InputState::new(cx));
        Self {
            scroll_row: 0,
            scroll_handle: ScrollHandle::new(),
            h_scroll_offset_px: 0.0,
            focus_handle: cx.focus_handle(),
            hidden_columns: BTreeSet::new(),
            columns_popover_open: false,
            filters_popover_open: false,
            filter_col_idx: 0,
            filter_op: FilterOp::Contains,
            filter_value_input,
            inline_input,
            inline_focus_requested: None,
            resizing_col_idx: None,
            resize_start_x: 0.0,
            resize_start_width: 140.0,
            dragging_selection: false,
        }
    }
}

/// Renders the interactive 2D-virtualized Data Grid.
pub fn render_data_grid(
    ws: &mut DataForgeWorkspace,
    window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    if let Some(table) = ws.controller.active_table.clone() {
        StructureViewState::ensure_table_loaded(ws, &table, cx);
    }

    if let Some((row_idx, col_idx)) = ws.data_grid_ui.inline_focus_requested.take()
        && ws
            .workspace_pane
            .inline_edit
            .as_ref()
            .is_some_and(|e| e.row_index == row_idx && e.col_index == col_idx)
    {
        let focus = ws.data_grid_ui.inline_input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    }

    let phase = ws
        .controller
        .connections
        .phase(ws.controller.active_workspace)
        .cloned();

    let has_data = !ws.controller.grid_columns.is_empty() || !ws.controller.grid_rows.is_empty();

    if !has_data && ws.controller.active_table.is_none() {
        return render_grid_empty_state(phase.as_ref(), cx);
    }

    let toolbar = render_grid_toolbar(ws, cx);
    let filter_strip = render_active_filter_strip(ws, cx);
    let viewport = render_grid_viewport(ws, cx);
    let footer = render_grid_footer(ws, cx);
    let focus_handle = ws.data_grid_ui.focus_handle.clone();

    div()
        .id("data-grid-root")
        .track_focus(&focus_handle)
        .size_full()
        .flex()
        .flex_col()
        .bg(ws.content_palette.window_background)
        .on_key_down(cx.listener(handle_grid_key_down))
        .on_mouse_move(cx.listener(handle_grid_mouse_move))
        .on_mouse_up(MouseButton::Left, cx.listener(handle_grid_mouse_up))
        .child(toolbar)
        .children(filter_strip)
        .child(div().flex_1().min_h_0().w_full().child(viewport))
        .child(footer)
        .into_any_element()
}

fn render_grid_empty_state(phase: Option<&ConnectionWindowPhase>, cx: &App) -> AnyElement {
    let (title, detail) = match phase {
        Some(ConnectionWindowPhase::Disconnected) | None => (
            "No database connection",
            "Connect to a database in Connections, then pick a table from the Schema Browser.",
        ),
        Some(ConnectionWindowPhase::Connecting { .. }) => (
            "Connecting to server...",
            "Schema objects will appear in the Schema Browser once connected.",
        ),
        Some(ConnectionWindowPhase::Reconnecting { .. }) => (
            "Reconnecting...",
            "Waiting for connection recovery before browsing table rows.",
        ),
        _ => (
            "No table open",
            "Select a table in the Schema Browser or execute a query in the SQL Editor.",
        ),
    };

    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .p(px(24.0))
        .bg(theme::panel_surface_bg(cx))
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
                .child(title),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(cx.colors().muted)
                .child(detail),
        )
        .into_any_element()
}

fn render_grid_toolbar(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let border = theme::hairline_border_color(cx);
    let left = render_grid_toolbar_left(ws, cx);
    let right = render_grid_toolbar_right(ws, cx);

    div()
        .h(px(38.0))
        .w_full()
        .px(px(12.0))
        .flex_shrink_0()
        .border_b_1()
        .border_color(border)
        .bg(theme::panel_surface_bg(cx))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .child(left)
        .child(right)
        .into_any_element()
}

fn render_grid_toolbar_left(ws: &DataForgeWorkspace, cx: &App) -> AnyElement {
    let has_pending = ws.controller.grid_model.edits.has_pending_edits();
    let pending_count = ws.controller.grid_model.edits.modified_cells.len()
        + ws.controller.grid_model.edits.inserted_rows.len()
        + ws.controller.grid_model.edits.deleted_rows.len();

    let table_chip_label = ws
        .controller
        .active_table
        .as_ref()
        .map(|t| match &t.schema {
            Some(s) => format!("{s}.{}", t.name),
            None => t.name.clone(),
        })
        .unwrap_or_else(|| "Query Result".to_owned());

    let row_count_text = match ws.controller.grid_model.page.table_row_count {
        Some(total) => format!("{} rows ({total} total)", ws.controller.grid_rows.len()),
        None => format!("{} rows", ws.controller.grid_rows.len()),
    };

    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .font_features(tnum_features())
        .child(
            Chip::new()
                .variant(ChipVariant::Secondary)
                .size(HeroSize::Sm)
                .child(ChipLabel::new().child(table_chip_label)),
        )
        .child(
            div()
                .text_size(px(11.0))
                .text_color(cx.colors().muted)
                .child(row_count_text),
        )
        .when(has_pending, |el| {
            el.child(
                Chip::new()
                    .variant(ChipVariant::Soft)
                    .size(HeroSize::Sm)
                    .color(HeroColor::Warning)
                    .child(ChipLabel::new().child(format!("{pending_count} pending"))),
            )
        })
        .into_any_element()
}

fn render_grid_toolbar_right(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let has_table = ws.controller.active_table.is_some();
    let has_pending = ws.controller.grid_model.edits.has_pending_edits();
    let has_selected_row = ws.controller.grid_selection.focused.is_some();

    let mut right = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(
            Button::new("grid-add-row")
                .variant(Variant::Tertiary)
                .size(HeroSize::Sm)
                .is_disabled(ws.controller.grid_columns.is_empty())
                .child(Icon::new(IconName::Plus).size(IconSize::Xs))
                .label("Row")
                .on_press(cx.listener(|ws, _ev, _w, cx| {
                    let count = ws.controller.grid_model.columns.columns().len();
                    if count > 0 {
                        let new_idx = ws.controller.grid_rows.len();
                        ws.controller.insert_grid_row(vec![Value::Null; count]);
                        ws.controller.select_grid_cell(new_idx, 0);
                        ws.data_grid_ui.scroll_handle.scroll_to_item(new_idx);
                        cx.notify();
                    }
                })),
        )
        .child(
            Button::new("grid-delete-row")
                .variant(Variant::Tertiary)
                .size(HeroSize::Sm)
                .is_disabled(!has_selected_row)
                .child(Icon::new(IconName::Trash2).size(IconSize::Xs))
                .label("Delete")
                .on_press(cx.listener(|ws, _ev, _w, cx| {
                    if let Some((row_idx, _)) = ws.controller.grid_selection.focused {
                        delete_row_at_index(ws, row_idx);
                        cx.notify();
                    }
                })),
        );

    if has_pending {
        right = append_pending_edit_buttons(right, has_table, cx);
    }

    right
        .child(render_columns_popover(ws, cx))
        .child(render_filters_popover(ws, cx))
        .child(
            Button::new("grid-export-btn")
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .is_disabled(ws.controller.grid_columns.is_empty())
                .child(Icon::new(IconName::Download).size(IconSize::Xs))
                .label("Export")
                .on_press(cx.listener(|ws, _ev, window, cx| {
                    ws.modals_ui.open_export(window, cx);
                    cx.notify();
                })),
        )
        .into_any_element()
}

fn append_pending_edit_buttons(
    row: gpui::Div,
    has_table: bool,
    cx: &mut Context<DataForgeWorkspace>,
) -> gpui::Div {
    row.child(
        Button::new("grid-rollback-edits")
            .variant(Variant::Secondary)
            .size(HeroSize::Sm)
            .child(Icon::new(IconName::RotateCcw).size(IconSize::Xs))
            .label("Rollback")
            .on_press(cx.listener(|ws, _ev, _w, cx| {
                ws.controller.rollback_grid_edits();
                ws.workspace_pane.inline_edit = None;
                ws.notify_toast(
                    ToastSeverity::Info,
                    "Rolled back pending row edits",
                    None,
                    cx,
                );
                cx.notify();
            })),
    )
    .child(
        Button::new("grid-commit-edits")
            .variant(Variant::Primary)
            .size(HeroSize::Sm)
            .is_disabled(!has_table)
            .child(Icon::new(IconName::Check).size(IconSize::Xs))
            .label("Commit")
            .on_press(cx.listener(|ws, _ev, _w, cx| {
                match ws.controller.commit_grid_edits_blocking() {
                    Ok(applied) => {
                        ws.notify_toast(
                            ToastSeverity::Success,
                            format!("Committed {applied} statement(s)"),
                            None,
                            cx,
                        );
                    }
                    Err(err) => {
                        ws.notify_toast(
                            ToastSeverity::Error,
                            "Commit failed",
                            Some(SharedString::from(err.to_string())),
                            cx,
                        );
                    }
                }
                cx.notify();
            })),
    )
}

fn render_columns_popover(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let cols = ws.controller.grid_model.columns.columns().to_vec();
    let hidden_count = ws.data_grid_ui.hidden_columns.len();
    let trigger_label = if hidden_count > 0 {
        format!(
            "Columns ({}/{})",
            cols.len().saturating_sub(hidden_count),
            cols.len()
        )
    } else {
        "Columns".to_owned()
    };

    let trigger = Button::new("grid-columns-trigger")
        .variant(Variant::Secondary)
        .size(HeroSize::Sm)
        .child(Icon::new(IconName::Columns2).size(IconSize::Xs))
        .label(trigger_label);

    let entity_open = cx.entity().clone();
    let list = build_column_visibility_list(ws, cols, cx);
    let footer_actions = render_columns_popover_footer(cx);

    Popover::new(trigger)
        .id("grid-columns-popover")
        .placement(PopoverPlacement::BottomEnd)
        .panel_width(px(260.0))
        .title("Visible Columns")
        .is_open(ws.data_grid_ui.columns_popover_open)
        .on_open_change(move |open, _w, cx| {
            let next = *open;
            entity_open.update(cx, |ws, cx| {
                ws.data_grid_ui.columns_popover_open = next;
                cx.notify();
            });
        })
        .child(list)
        .child(footer_actions)
        .into_any_element()
}

fn build_column_visibility_list(
    ws: &DataForgeWorkspace,
    cols: Vec<DataGridColumn>,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let mut list = div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .max_h(px(280.0))
        .id("grid-columns-popover-scroll")
        .overflow_y_scroll();

    for col in cols {
        let col_id = col.id;
        let visible = !ws.data_grid_ui.hidden_columns.contains(&col_id);
        let entity_cb = cx.entity().clone();
        let label_text = if col.is_frozen {
            format!("{} (Pinned)", col.name)
        } else {
            col.name
        };
        list = list.child(
            Checkbox::new(("grid-col-vis", col_id.0 as usize))
                .is_selected(visible)
                .label(label_text)
                .on_change(move |checked, _w, cx| {
                    let show = *checked;
                    entity_cb.update(cx, |ws, cx| {
                        if show {
                            ws.data_grid_ui.hidden_columns.remove(&col_id);
                        } else {
                            ws.data_grid_ui.hidden_columns.insert(col_id);
                        }
                        cx.notify();
                    });
                }),
        );
    }
    list.into_any_element()
}

fn render_columns_popover_footer(cx: &mut Context<DataForgeWorkspace>) -> AnyElement {
    div()
        .pt(px(6.0))
        .border_t_1()
        .border_color(theme::hairline_border_color(cx))
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(
            Button::new("grid-cols-show-all")
                .variant(Variant::Tertiary)
                .size(HeroSize::Sm)
                .label("Show All")
                .on_press(cx.listener(|ws, _ev, _w, cx| {
                    ws.data_grid_ui.hidden_columns.clear();
                    cx.notify();
                })),
        )
        .child(
            Button::new("grid-cols-unfreeze")
                .variant(Variant::Tertiary)
                .size(HeroSize::Sm)
                .label("Unfreeze All")
                .on_press(cx.listener(|ws, _ev, _w, cx| {
                    ws.controller.grid_model.columns.unfreeze_all();
                    ws.controller.sync_grid_view_from_model();
                    cx.notify();
                })),
        )
        .into_any_element()
}

fn render_filters_popover(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let active_count = ws.controller.grid_model.filter_bar.filter_set.rules.len();
    let label = if active_count > 0 {
        format!("Filters ({active_count})")
    } else {
        "Filters".to_owned()
    };

    let trigger = Button::new("grid-filters-trigger")
        .variant(if active_count > 0 {
            Variant::Primary
        } else {
            Variant::Secondary
        })
        .size(HeroSize::Sm)
        .child(Icon::new(IconName::ListFilter).size(IconSize::Xs))
        .label(label);

    let entity_open = cx.entity().clone();
    let form = build_filters_popover_form(ws, active_count, cx);

    Popover::new(trigger)
        .id("grid-filters-popover")
        .placement(PopoverPlacement::BottomEnd)
        .panel_width(px(280.0))
        .title("Filter Rows")
        .is_open(ws.data_grid_ui.filters_popover_open)
        .on_open_change(move |open, _w, cx| {
            let next = *open;
            entity_open.update(cx, |ws, cx| {
                ws.data_grid_ui.filters_popover_open = next;
                cx.notify();
            });
        })
        .child(form)
        .into_any_element()
}

fn build_filters_popover_form(
    ws: &DataForgeWorkspace,
    active_count: usize,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let col_items: Vec<PickerItem> = ws
        .controller
        .grid_columns
        .iter()
        .enumerate()
        .map(|(i, c)| PickerItem::new(format!("{i}"), c.title.clone()))
        .collect();
    let selected_col_key = col_items
        .get(ws.data_grid_ui.filter_col_idx)
        .map(|item| item.key().clone());

    let op_items = filter_op_picker_items();
    let selected_op_key = Some(SharedString::from(filter_op_key(ws.data_grid_ui.filter_op)));
    let needs_value = !matches!(
        ws.data_grid_ui.filter_op,
        FilterOp::IsNull | FilterOp::IsNotNull
    );

    let entity_col = cx.entity().clone();
    let entity_op = cx.entity().clone();
    let entity_submit = cx.entity().clone();

    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            Select::new("filter-col-select", col_items)
                .value(selected_col_key)
                .placeholder("Select column")
                .on_selection_change(move |key_opt, _w, cx| {
                    if let Some(k) = key_opt
                        && let Ok(idx) = k.parse::<usize>()
                    {
                        entity_col.update(cx, |ws, cx| {
                            ws.data_grid_ui.filter_col_idx = idx;
                            cx.notify();
                        });
                    }
                }),
        )
        .child(
            Select::new("filter-op-select", op_items)
                .value(selected_op_key)
                .on_selection_change(move |key_opt, _w, cx| {
                    if let Some(k) = key_opt
                        && let Some(op) = parse_filter_op_key(k.as_ref())
                    {
                        entity_op.update(cx, |ws, cx| {
                            ws.data_grid_ui.filter_op = op;
                            cx.notify();
                        });
                    }
                }),
        )
        .when(needs_value, |el| {
            el.child(
                Input::new(&ws.data_grid_ui.filter_value_input)
                    .placeholder("Filter value...")
                    .on_submit(move |_val, _w, cx| {
                        entity_submit.update(cx, |ws, cx| {
                            apply_pending_filter(ws, cx);
                        });
                    }),
            )
        })
        .child(render_filters_form_actions(ws, active_count, cx))
        .into_any_element()
}

fn render_filters_form_actions(
    ws: &DataForgeWorkspace,
    active_count: usize,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(6.0))
        .child(
            Button::new("grid-filter-clear")
                .variant(Variant::Tertiary)
                .size(HeroSize::Sm)
                .is_disabled(active_count == 0)
                .label("Clear All")
                .on_press(cx.listener(|ws, _ev, _w, cx| {
                    let _ = ws.controller.clear_grid_filters_blocking();
                    ws.data_grid_ui.filters_popover_open = false;
                    cx.notify();
                })),
        )
        .child(
            Button::new("grid-filter-apply")
                .variant(Variant::Primary)
                .size(HeroSize::Sm)
                .is_disabled(ws.controller.grid_columns.is_empty())
                .label("Apply")
                .on_press(cx.listener(|ws, _ev, _w, cx| {
                    apply_pending_filter(ws, cx);
                })),
        )
        .into_any_element()
}

fn render_active_filter_strip(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> Option<AnyElement> {
    let rules = &ws.controller.grid_model.filter_bar.filter_set.rules;
    if rules.is_empty() {
        return None;
    }

    let mut row = div()
        .w_full()
        .px(px(12.0))
        .py(px(4.0))
        .border_b_1()
        .border_color(theme::hairline_border_color(cx))
        .bg(theme::muted_surface_bg(cx))
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.0))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(cx.colors().muted)
                .child("WHERE"),
        );

    for (idx, rule) in rules.iter().enumerate() {
        let val_str = match &rule.value {
            Some(tablepro_core::filter::FilterValue::Single(v)) => format!(" '{v}'"),
            _ => String::new(),
        };
        let summary = format!("{} {}{}", rule.column, filter_op_label(rule.op), val_str);
        row = row.child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .size(HeroSize::Sm)
                .color(HeroColor::Accent)
                .child(ChipLabel::new().child(summary))
                .child(
                    Button::new(("remove-filter", idx))
                        .variant(Variant::Tertiary)
                        .size(HeroSize::Sm)
                        .is_icon_only(true)
                        .child(Icon::new(IconName::X).size(IconSize::Xs))
                        .on_press(cx.listener(move |ws, _ev, _w, cx| {
                            ws.controller.grid_model.filter_bar.remove_rule(idx);
                            if ws.controller.active_table.is_some() {
                                let _ = ws
                                    .controller
                                    .query_active_table_page_blocking(&PagePosition::First);
                                let _ = ws.controller.refresh_table_row_count_blocking();
                            }
                            cx.notify();
                        })),
                ),
        );
    }

    Some(row.into_any_element())
}

fn render_grid_viewport(
    ws: &mut DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let display_cols = ws.controller.grid_model.columns.columns().to_vec();
    let visible_cols: Vec<(usize, DataGridColumn)> = display_cols
        .into_iter()
        .enumerate()
        .filter(|(_, c)| !ws.data_grid_ui.hidden_columns.contains(&c.id))
        .collect();

    if visible_cols.is_empty() {
        return div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.0))
            .text_color(cx.colors().muted)
            .child("All columns are hidden. Open Columns to restore visibility.")
            .into_any_element();
    }

    let scroll_handle = ws.data_grid_ui.scroll_handle.clone();
    let offset = scroll_handle.offset();
    let vp_size = scroll_handle.bounds().size;
    let view_h = f32::from(vp_size.height).max(300.0);
    let view_w = f32::from(vp_size.width).max(760.0);
    let scroll_y = (-f32::from(offset.y)).max(0.0);

    let (frozen_cols, scroll_cols): GridColumnSplit =
        visible_cols.into_iter().partition(|(_, c)| c.is_frozen);

    let frozen_width_sum: f32 = frozen_cols.iter().map(|(_, c)| c.width as f32).sum();
    let max_frozen_px = view_w * dataforge_state::data_grid::MAXIMUM_FROZEN_SHARE as f32;
    let clamped_frozen_px = frozen_width_sum.min(max_frozen_px);
    let scrollable_vp_w = (view_w - ROW_GUTTER_WIDTH_PX - clamped_frozen_px).max(120.0);

    let total_scroll_w: f32 = scroll_cols.iter().map(|(_, c)| c.width as f32).sum();
    let max_h_scroll = (total_scroll_w - scrollable_vp_w).max(0.0);
    let scroll_x = ws.data_grid_ui.h_scroll_offset_px.clamp(0.0, max_h_scroll);
    ws.data_grid_ui.h_scroll_offset_px = scroll_x;

    let (slice_start, slice_end, left_pad_w, right_pad_w) =
        compute_variable_col_slice(&scroll_cols, scroll_x, scrollable_vp_w);
    let visible_scroll_cols = &scroll_cols[slice_start..slice_end];

    let total_rows = ws.controller.grid_rows.len();
    let first_row = ((scroll_y / ROW_HEIGHT_PX).floor() as usize).saturating_sub(OVERSCAN_ROWS);
    let vis_count = (view_h / ROW_HEIGHT_PX).ceil() as usize + OVERSCAN_ROWS * 2;
    let last_row = (first_row + vis_count).min(total_rows);
    ws.data_grid_ui.scroll_row = first_row;

    let header = render_grid_header_row(
        ws,
        &frozen_cols,
        visible_scroll_cols,
        left_pad_w,
        right_pad_w,
        view_w,
        cx,
    );
    let rows_col = build_grid_viewport_rows(
        ws,
        first_row..last_row,
        total_rows,
        &frozen_cols,
        visible_scroll_cols,
        left_pad_w,
        right_pad_w,
        cx,
    );

    wrap_grid_viewport_container(ws, scroll_handle, header, rows_col, max_h_scroll, cx)
}

#[expect(
    clippy::too_many_arguments,
    reason = "virtualized row slice builder needs vertical range plus frozen and horizontal slices"
)]
fn build_grid_viewport_rows(
    ws: &DataForgeWorkspace,
    row_range: std::ops::Range<usize>,
    total_rows: usize,
    frozen_cols: &[(usize, DataGridColumn)],
    visible_scroll_cols: &[(usize, DataGridColumn)],
    left_pad_w: f32,
    right_pad_w: f32,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let top_spacer = row_range.start as f32 * ROW_HEIGHT_PX;
    let bottom_spacer = total_rows.saturating_sub(row_range.end) as f32 * ROW_HEIGHT_PX;
    let mut rows_col = div().w_full().flex().flex_col();
    if top_spacer > 0.0 {
        rows_col = rows_col.child(div().w_full().h(px(top_spacer)));
    }
    for row_idx in row_range {
        if let Some(grid_row) = ws.controller.grid_rows.get(row_idx).cloned() {
            rows_col = rows_col.child(render_grid_data_row(
                ws,
                row_idx,
                &grid_row,
                frozen_cols,
                visible_scroll_cols,
                left_pad_w,
                right_pad_w,
                cx,
            ));
        }
    }
    if bottom_spacer > 0.0 {
        rows_col = rows_col.child(div().w_full().h(px(bottom_spacer)));
    }
    rows_col.into_any_element()
}

fn wrap_grid_viewport_container(
    ws: &DataForgeWorkspace,
    scroll_handle: ScrollHandle,
    header: AnyElement,
    rows_col: AnyElement,
    max_h_scroll: f32,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let font = theme::value_gpui_font(&ws.controller.settings);
    let font_size = ws.controller.settings.value_font.size.max(10.0);

    div()
        .id("data-grid-viewport")
        .relative()
        .size_full()
        .flex()
        .flex_col()
        .font(font)
        .text_size(px(font_size))
        .font_features(tnum_features())
        .on_scroll(cx.listener(move |ws, ev: &gpui::ScrollEvent, _w, cx| {
            let delta = ev.delta.pixel_delta(px(ROW_HEIGHT_PX));
            let dx = f32::from(delta.x);
            if dx.abs() > 0.5 || ev.modifiers.shift {
                let eff_dx = if dx.abs() > 0.5 {
                    dx
                } else {
                    f32::from(delta.y)
                };
                ws.data_grid_ui.h_scroll_offset_px =
                    (ws.data_grid_ui.h_scroll_offset_px - eff_dx).clamp(0.0, max_h_scroll);
                cx.notify();
            }
        }))
        .child(header)
        .child(
            div()
                .id("data-grid-scroll-body")
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_y_scroll()
                .track_scroll(&scroll_handle)
                .child(rows_col),
        )
        .child(
            div()
                .absolute()
                .top(px(HEADER_HEIGHT_PX))
                .bottom_0()
                .right_0()
                .w(px(10.0))
                .child(
                    Scrollbar::new("data-grid-vscroll", scroll_handle)
                        .orientation(HeroOrientation::Vertical),
                ),
        )
        .into_any_element()
}

fn render_grid_header_row(
    ws: &DataForgeWorkspace,
    frozen_cols: &[(usize, DataGridColumn)],
    scroll_cols: &[(usize, DataGridColumn)],
    left_pad_w: f32,
    right_pad_w: f32,
    viewport_w: f32,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let hdr_bg = ws.content_palette.data_grid_header_background;
    let hdr_fg = ws.content_palette.data_grid_header_text;
    let border = theme::hairline_border_color(cx);

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
                .text_color(cx.colors().muted)
                .child("#"),
        );

    for (display_pos, col) in frozen_cols {
        row = row.child(render_grid_header_cell(
            ws,
            *display_pos,
            col,
            hdr_fg,
            border,
            viewport_w,
            cx,
        ));
    }

    if !frozen_cols.is_empty() {
        row = row.child(
            div()
                .w(px(2.0))
                .h_full()
                .flex_shrink_0()
                .bg(with_alpha(cx.colors().accent.color, 0.45)),
        );
    }

    if left_pad_w > 0.0 {
        row = row.child(div().w(px(left_pad_w)).h_full().flex_shrink_0());
    }
    for (display_pos, col) in scroll_cols {
        row = row.child(render_grid_header_cell(
            ws,
            *display_pos,
            col,
            hdr_fg,
            border,
            viewport_w,
            cx,
        ));
    }
    if right_pad_w > 0.0 {
        row = row.child(div().w(px(right_pad_w)).h_full().flex_shrink_0());
    }

    row.into_any_element()
}

fn render_grid_header_cell(
    ws: &DataForgeWorkspace,
    display_pos: usize,
    col: &DataGridColumn,
    hdr_fg: Hsla,
    border: Hsla,
    viewport_w: f32,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let sort_dir = ws
        .controller
        .grid_model
        .sort
        .keys
        .iter()
        .find(|k| k.column_id == col.id)
        .map(|k| k.direction);
    let col_width = col.width as f32;
    let is_resizing = ws.data_grid_ui.resizing_col_idx == Some(display_pos);
    let menu_btn = render_header_column_menu(display_pos, col, viewport_w, cx);
    let resize_handle = render_header_resize_handle(display_pos, col_width, is_resizing, cx);

    div()
        .id(("grid-hdr", display_pos))
        .relative()
        .w(px(col_width))
        .h_full()
        .flex_shrink_0()
        .px(px(8.0))
        .border_r_1()
        .border_color(border)
        .hover(|s| s.bg(cx.colors().surface_secondary.opacity(0.6)))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(4.0))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, _ev, _w, cx| {
                let _ = ws.controller.toggle_grid_sort_blocking(display_pos);
                cx.notify();
            }),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(hdr_fg)
                        .child(col.name.clone()),
                )
                .when_some(sort_dir, |el, dir| {
                    el.child(
                        Icon::new(match dir {
                            ModelSortDir::Ascending => IconName::ArrowUp,
                            ModelSortDir::Descending => IconName::ArrowDown,
                        })
                        .size(IconSize::Xs)
                        .color(cx.colors().accent.color),
                    )
                }),
        )
        .child(menu_btn)
        .child(resize_handle)
        .into_any_element()
}

fn render_header_column_menu(
    display_pos: usize,
    col: &DataGridColumn,
    viewport_w: f32,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let menu_items = vec![
        MenuItem::new("sort-asc", "Sort Ascending"),
        MenuItem::new("sort-desc", "Sort Descending"),
        MenuItem::new("sort-clear", "Clear Sort"),
        MenuItem::Separator,
        MenuItem::new(
            "freeze",
            if col.is_frozen {
                "Unfreeze All"
            } else {
                "Freeze Up to Column"
            },
        ),
        MenuItem::new("auto-fit", "Auto-Fit Width"),
        MenuItem::new("hide", "Hide Column"),
    ];

    let entity_menu = cx.entity().clone();
    let title_for_fit = col.name.clone();
    let data_idx = col.data_index;
    let col_id = col.id;

    Dropdown::uncontrolled(
        ("col-menu", display_pos),
        div()
            .id(("col-menu-trig", display_pos))
            .w(px(18.0))
            .h(px(18.0))
            .rounded(px(4.0))
            .flex()
            .items_center()
            .justify_center()
            .hover(|s| s.bg(cx.colors().surface_secondary))
            .child(
                Icon::new(IconName::ChevronDown)
                    .size(IconSize::Xs)
                    .color(cx.colors().muted),
            ),
        menu_items,
    )
    .placement(DropdownPlacement::BottomEnd)
    .on_action(move |key, _w, cx| {
        let action_key = key.to_string();
        let fit_title = title_for_fit.clone();
        entity_menu.update(cx, |ws, cx| {
            handle_column_menu_action(
                ws,
                &action_key,
                display_pos,
                data_idx,
                col_id,
                &fit_title,
                viewport_w,
                cx,
            );
        });
    })
    .into_any_element()
}

fn render_header_resize_handle(
    display_pos: usize,
    col_width: f32,
    is_resizing: bool,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    div()
        .id(("col-resize-handle", display_pos))
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .w(px(6.0))
        .cursor_col_resize()
        .when(is_resizing, |el| el.bg(cx.colors().accent.color))
        .child(
            gpui::canvas(
                |bounds, window, _| window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal),
                move |_, hitbox, window, _| {
                    if is_resizing {
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
                ws.data_grid_ui.resizing_col_idx = Some(display_pos);
                ws.data_grid_ui.resize_start_x = f32::from(event.position.x);
                ws.data_grid_ui.resize_start_width = col_width;
                cx.stop_propagation();
                cx.notify();
            }),
        )
        .into_any_element()
}

#[expect(
    clippy::too_many_arguments,
    reason = "virtualized row renderer draws gutter, frozen slice, freeze divider, and scrollable slice"
)]
fn render_grid_data_row(
    ws: &DataForgeWorkspace,
    row_idx: usize,
    grid_row: &GridRow,
    frozen_cols: &[(usize, DataGridColumn)],
    scroll_cols: &[(usize, DataGridColumn)],
    left_pad_w: f32,
    right_pad_w: f32,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let border = theme::hairline_border_color(cx);
    let row_bg = match grid_row.state {
        RowState::Inserted => Some(ws.content_palette.data_grid_inserted),
        RowState::Modified => Some(ws.content_palette.data_grid_modified),
        RowState::Deleted => Some(ws.content_palette.data_grid_deleted),
        RowState::Clean => theme::row_zebra_bg(row_idx, cx),
    };
    let row_id_opt = ws.controller.grid_model.row_order.row_id(row_idx);

    let mut row = div()
        .id(("dg-row", row_idx))
        .w_full()
        .h(px(ROW_HEIGHT_PX))
        .flex_shrink_0()
        .border_b_1()
        .border_color(border)
        .when_some(row_bg, |el, bg| el.bg(bg))
        .flex()
        .items_center()
        .overflow_hidden()
        .child(render_grid_row_gutter(ws, row_idx, grid_row, border, cx));

    for (display_pos, col) in frozen_cols {
        row = row.child(render_grid_cell(
            ws,
            row_idx,
            *display_pos,
            col,
            grid_row,
            row_id_opt,
            cx,
        ));
    }

    if !frozen_cols.is_empty() {
        row = row.child(
            div()
                .w(px(2.0))
                .h_full()
                .flex_shrink_0()
                .bg(with_alpha(cx.colors().accent.color, 0.45)),
        );
    }

    if left_pad_w > 0.0 {
        row = row.child(div().w(px(left_pad_w)).h_full().flex_shrink_0());
    }
    for (display_pos, col) in scroll_cols {
        row = row.child(render_grid_cell(
            ws,
            row_idx,
            *display_pos,
            col,
            grid_row,
            row_id_opt,
            cx,
        ));
    }
    if right_pad_w > 0.0 {
        row = row.child(div().w(px(right_pad_w)).h_full().flex_shrink_0());
    }

    row.into_any_element()
}

fn render_grid_row_gutter(
    ws: &DataForgeWorkspace,
    row_idx: usize,
    grid_row: &GridRow,
    border: Hsla,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let gutter_badge = match grid_row.state {
        RowState::Inserted => Some("+"),
        RowState::Modified => Some("*"),
        RowState::Deleted => Some("-"),
        RowState::Clean => None,
    };
    div()
        .id(("dg-gutter", row_idx))
        .w(px(ROW_GUTTER_WIDTH_PX))
        .h_full()
        .flex_shrink_0()
        .px(px(8.0))
        .bg(ws.content_palette.data_grid_header_background)
        .border_r_1()
        .border_color(border)
        .flex()
        .items_center()
        .justify_between()
        .text_size(px(11.0))
        .text_color(cx.colors().muted)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, _ev, _w, cx| {
                if let Some(rid) = ws.controller.grid_model.row_order.row_id(row_idx) {
                    ws.controller
                        .grid_model
                        .apply_selection(GridSelectionAction::SelectRow(rid));
                }
                ws.controller.select_grid_cell(row_idx, 0);
                cx.notify();
            }),
        )
        .child(div().child(gutter_badge.unwrap_or("")))
        .child(div().child(format!("{}", grid_row.row_number)))
        .into_any_element()
}

fn render_grid_cell(
    ws: &DataForgeWorkspace,
    row_idx: usize,
    display_pos: usize,
    col: &DataGridColumn,
    grid_row: &GridRow,
    row_id_opt: Option<dataforge_state::data_grid::RowID>,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let col_id = col.id;
    let col_width = col.width as f32;
    let is_editing = ws
        .workspace_pane
        .inline_edit
        .as_ref()
        .is_some_and(|e| e.row_index == row_idx && e.col_index == display_pos);

    if is_editing {
        return render_inline_edit_cell(ws, row_idx, display_pos, col_width, cx);
    }

    let is_focused = ws.controller.grid_selection.focused == Some((row_idx, display_pos));
    let is_selected = is_cell_selected(ws, row_idx, display_pos, row_id_opt, col_id);
    let inner = build_grid_cell_content(ws, row_idx, display_pos, col, grid_row, is_selected, cx);
    let border = theme::hairline_border_color(cx);

    let cell_id = row_idx.saturating_mul(10_000).saturating_add(display_pos);
    let cell_box = div()
        .id(("dg-cell", cell_id))
        .w(px(col_width))
        .h_full()
        .flex_shrink_0()
        .px(px(8.0))
        .border_r_1()
        .border_color(border)
        .when(is_selected, |el| {
            el.bg(ws.content_palette.data_grid_selection)
        })
        .when(is_focused, |el| {
            el.border_1()
                .border_color(ws.content_palette.data_grid_focus_border)
        })
        .flex()
        .items_center()
        .overflow_hidden()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, event: &gpui::MouseDownEvent, window, cx| {
                handle_cell_click(
                    ws,
                    row_idx,
                    display_pos,
                    col_id,
                    row_id_opt,
                    event,
                    window,
                    cx,
                );
            }),
        )
        .on_mouse_move(cx.listener(move |ws, _ev, _w, cx| {
            if ws.data_grid_ui.dragging_selection
                && let Some(rid) = row_id_opt
            {
                ws.controller
                    .grid_model
                    .apply_selection(GridSelectionAction::DragTo(GridCoord {
                        row: rid,
                        col: col_id,
                    }));
                ws.controller.select_grid_cell(row_idx, display_pos);
                cx.notify();
            }
        }))
        .child(inner);

    wrap_grid_cell_context_menu(cell_id, cell_box, row_idx, display_pos, cx)
}

fn build_grid_cell_content(
    ws: &DataForgeWorkspace,
    row_idx: usize,
    display_pos: usize,
    col: &DataGridColumn,
    grid_row: &GridRow,
    is_selected: bool,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let raw_cell = grid_row.cells.get(col.data_index);
    let raw_val = resolve_raw_value(ws, row_idx, col.data_index);
    let is_deleted = grid_row.state == RowState::Deleted;
    let is_null = raw_val.as_ref().is_some_and(Value::is_null)
        || raw_cell.is_some_and(|c| c.kind == CellKind::Null);
    if is_null {
        return Chip::new()
            .variant(ChipVariant::Secondary)
            .size(HeroSize::Sm)
            .child(
                ChipLabel::new()
                    .sx(|s| s.italic().text_size(px(10.0)))
                    .child("NULL"),
            )
            .into_any_element();
    }
    if raw_cell.is_some_and(|c| c.kind == CellKind::Default) {
        return Chip::new()
            .variant(ChipVariant::Soft)
            .size(HeroSize::Sm)
            .color(HeroColor::Accent)
            .child(
                ChipLabel::new()
                    .sx(|s| s.italic().text_size(px(10.0)))
                    .child("DEFAULT"),
            )
            .into_any_element();
    }

    let is_num = raw_val.as_ref().is_some_and(Value::is_number)
        || raw_cell.is_some_and(|c| c.kind == CellKind::Number);
    let fk_match = ws
        .controller
        .active_table
        .as_ref()
        .and_then(|t| ws.structure_ui.cached_foreign_keys(t))
        .and_then(|fks| find_fk_for_column(fks, &col.name));
    let fg = if is_selected {
        ws.content_palette.data_grid_selection_text
    } else {
        ws.content_palette.editor_text
    };
    let display_text = raw_cell
        .map(|c| c.text.clone())
        .or_else(|| raw_val.as_ref().map(format_core_value))
        .unwrap_or_default();
    let val_for_fk = raw_val
        .as_ref()
        .map(format_core_value)
        .unwrap_or_else(|| display_text.clone());

    let text_el = div()
        .min_w_0()
        .flex_1()
        .truncate()
        .when(is_num, |el| el.text_right())
        .when(is_deleted, |el| el.line_through())
        .text_color(fg)
        .child(display_text);

    if let Some(fk) = fk_match.cloned() {
        div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(4.0))
            .child(text_el)
            .child(
                Button::new(("fk-jump", row_idx * 1000 + display_pos))
                    .variant(Variant::Tertiary)
                    .size(HeroSize::Sm)
                    .is_icon_only(true)
                    .child(
                        Icon::new(IconName::ArrowUpRight)
                            .size(IconSize::Xs)
                            .color(cx.colors().accent.color),
                    )
                    .on_press(cx.listener(move |ws, _ev, _w, cx| {
                        navigate_to_foreign_key(ws, &fk, &val_for_fk, cx);
                    })),
            )
            .into_any_element()
    } else {
        text_el.into_any_element()
    }
}

fn wrap_grid_cell_context_menu(
    cell_id: usize,
    cell_box: gpui::Stateful<gpui::Div>,
    row_idx: usize,
    display_pos: usize,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let ctx_items = vec![
        MenuItem::new("edit", "Edit Cell"),
        MenuItem::new("set-null", "Set NULL"),
        MenuItem::Separator,
        MenuItem::new("copy-cell", "Copy Cell"),
        MenuItem::new("copy-row", "Copy Row (TSV)"),
        MenuItem::Separator,
        MenuItem::new("delete-row", "Delete Row").danger(),
    ];
    let entity_ctx = cx.entity().clone();

    ContextMenu::new(("cell-ctx", cell_id), cell_box, ctx_items)
        .on_action(move |key, window, cx| {
            let k = key.to_string();
            entity_ctx.update(cx, |ws, cx| {
                handle_cell_context_action(ws, &k, row_idx, display_pos, window, cx);
            });
        })
        .into_any_element()
}

fn render_inline_edit_cell(
    ws: &DataForgeWorkspace,
    row_idx: usize,
    col_idx: usize,
    width: f32,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let entity_submit = cx.entity().clone();
    let entity_change = cx.entity().clone();

    div()
        .id(("dg-inline-wrap", row_idx * 1000 + col_idx))
        .w(px(width))
        .h_full()
        .flex_shrink_0()
        .border_1()
        .border_color(ws.content_palette.data_grid_focus_border)
        .bg(theme::elevated_overlay_bg(cx))
        .px(px(4.0))
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(
            div().flex_1().min_w_0().child(
                Input::new(&ws.data_grid_ui.inline_input)
                    .is_bare(true)
                    .height(px(22.0))
                    .text_size(px(12.0))
                    .on_change(move |text, _w, cx| {
                        let s = text.to_owned();
                        entity_change.update(cx, |ws, _cx| {
                            if let Some(edit) = &mut ws.workspace_pane.inline_edit {
                                edit.draft_text = s;
                                edit.set_null = false;
                            }
                        });
                    })
                    .on_submit(move |text, _w, cx| {
                        let s = text.to_owned();
                        entity_submit.update(cx, |ws, cx| {
                            commit_inline_edit(ws, s, false);
                            cx.notify();
                        });
                    }),
            ),
        )
        .child(
            Button::new("inline-set-null")
                .variant(Variant::Tertiary)
                .size(HeroSize::Sm)
                .label("NULL")
                .on_press(cx.listener(|ws, _ev, _w, cx| {
                    commit_inline_edit(ws, String::new(), true);
                    cx.notify();
                })),
        )
        .into_any_element()
}

fn render_grid_footer(ws: &DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) -> AnyElement {
    let page = ws.controller.grid_model.page;
    let (current_page, total_pages_opt) = page.page_info();
    let total_pages = total_pages_opt.unwrap_or(current_page.max(1));

    let shown = ws.controller.grid_rows.len() as u64;
    let range_text = if shown == 0 {
        "0 rows".to_owned()
    } else {
        let start = page.offset + 1;
        let end = page.offset + shown;
        match page.table_row_count {
            Some(total) => format!("Showing {start}–{end} of {total}"),
            None => format!("Showing {start}–{end}"),
        }
    };

    let entity_page = cx.entity().clone();
    let pagination = Pagination::new(
        "grid-pagination",
        current_page as usize,
        total_pages as usize,
    )
    .size(HeroSize::Sm)
    .summary(range_text)
    .is_disabled(ws.controller.active_table.is_none())
    .on_change(move |target_page, _w, cx| {
        let next_page = *target_page as u64;
        entity_page.update(cx, |ws, cx| {
            if next_page > current_page {
                let _ = ws.controller.next_grid_page_blocking();
            } else if next_page < current_page {
                let _ = ws.controller.prev_grid_page_blocking();
            }
            ws.data_grid_ui.scroll_handle.scroll_to_item(0);
            cx.notify();
        });
    });

    let size_items = vec![
        PickerItem::new("50", "50 rows"),
        PickerItem::new("100", "100 rows"),
        PickerItem::new("250", "250 rows"),
        PickerItem::new("500", "500 rows"),
    ];
    let current_limit = SharedString::from(format!("{}", page.limit));
    let entity_limit = cx.entity().clone();

    div()
        .h(px(38.0))
        .w_full()
        .px(px(12.0))
        .flex_shrink_0()
        .border_t_1()
        .border_color(theme::hairline_border_color(cx))
        .bg(theme::panel_surface_bg(cx))
        .font_features(tnum_features())
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(pagination)
        .child(
            div().w(px(116.0)).child(
                Select::new("grid-page-size", size_items)
                    .value(Some(current_limit))
                    .on_selection_change(move |key_opt, _w, cx| {
                        if let Some(k) = key_opt
                            && let Ok(limit) = k.parse::<u64>()
                        {
                            entity_limit.update(cx, |ws, cx| {
                                ws.controller.grid_model.page.limit = limit;
                                ws.controller.grid_model.page.offset = 0;
                                if ws.controller.active_table.is_some() {
                                    let _ = ws
                                        .controller
                                        .query_active_table_page_blocking(&PagePosition::First);
                                }
                                cx.notify();
                            });
                        }
                    }),
            ),
        )
        .into_any_element()
}

#[expect(
    clippy::too_many_arguments,
    reason = "cell click handles focus, shift selection, inspector sync, and double-click inline editor"
)]
fn handle_cell_click(
    ws: &mut DataForgeWorkspace,
    row_idx: usize,
    display_pos: usize,
    col_id: DataGridColumnID,
    row_id_opt: Option<dataforge_state::data_grid::RowID>,
    event: &gpui::MouseDownEvent,
    window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) {
    window.focus(&ws.data_grid_ui.focus_handle, cx);
    ws.data_grid_ui.dragging_selection = true;
    if let Some(rid) = row_id_opt {
        let coord = GridCoord {
            row: rid,
            col: col_id,
        };
        let action = if event.modifiers.shift {
            GridSelectionAction::ShiftClick(coord)
        } else {
            GridSelectionAction::Click(coord)
        };
        ws.controller.grid_model.apply_selection(action);
    }
    ws.controller.select_grid_cell(row_idx, display_pos);
    if event.click_count >= 2 {
        ws.data_grid_ui.dragging_selection = false;
        open_inline_editor(ws, row_idx, display_pos, cx);
    }
    cx.notify();
}

fn handle_grid_key_down(
    ws: &mut DataForgeWorkspace,
    event: &gpui::KeyDownEvent,
    window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) {
    if ws.workspace_pane.inline_edit.is_some() {
        if event.keystroke.key == "escape" {
            ws.workspace_pane.inline_edit = None;
            window.focus(&ws.data_grid_ui.focus_handle, cx);
            cx.notify();
        }
        return;
    }

    let Some((row, col)) = ws.controller.grid_selection.focused else {
        return;
    };
    let max_row = ws.controller.grid_rows.len().saturating_sub(1);
    let max_col = ws.controller.grid_columns.len().saturating_sub(1);

    match event.keystroke.key.as_str() {
        "enter" => {
            open_inline_editor(ws, row, col, cx);
            cx.notify();
        }
        "up" => {
            let next = row.saturating_sub(1);
            ws.controller.select_grid_cell(next, col);
            ws.data_grid_ui.scroll_handle.scroll_to_item(next);
            cx.notify();
        }
        "down" => {
            let next = (row + 1).min(max_row);
            ws.controller.select_grid_cell(next, col);
            ws.data_grid_ui.scroll_handle.scroll_to_item(next);
            cx.notify();
        }
        "left" => {
            ws.controller.select_grid_cell(row, col.saturating_sub(1));
            cx.notify();
        }
        "right" => {
            ws.controller.select_grid_cell(row, (col + 1).min(max_col));
            cx.notify();
        }
        _ => {}
    }
}

fn handle_grid_mouse_move(
    ws: &mut DataForgeWorkspace,
    event: &gpui::MouseMoveEvent,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) {
    if let Some(col_idx) = ws.data_grid_ui.resizing_col_idx {
        let dx = f32::from(event.position.x) - ws.data_grid_ui.resize_start_x;
        let clamped = (ws.data_grid_ui.resize_start_width + dx)
            .clamp(MIN_COLUMN_WIDTH as f32, MAX_COLUMN_WIDTH as f32);
        ws.controller.resize_grid_column(col_idx, clamped);
        cx.notify();
    }
}

fn handle_grid_mouse_up(
    ws: &mut DataForgeWorkspace,
    _event: &gpui::MouseUpEvent,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) {
    let changed = ws.data_grid_ui.resizing_col_idx.is_some() || ws.data_grid_ui.dragging_selection;
    ws.data_grid_ui.resizing_col_idx = None;
    ws.data_grid_ui.dragging_selection = false;
    if changed {
        cx.notify();
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "column dropdown handler covers sort, freeze, auto-fit, and visibility mutations"
)]
fn handle_column_menu_action(
    ws: &mut DataForgeWorkspace,
    action: &str,
    display_pos: usize,
    data_idx: usize,
    col_id: DataGridColumnID,
    col_title: &str,
    viewport_w: f32,
    cx: &mut Context<DataForgeWorkspace>,
) {
    match action {
        "sort-asc" | "sort-desc" => {
            let dir = if action == "sort-asc" {
                ModelSortDir::Ascending
            } else {
                ModelSortDir::Descending
            };
            ws.controller.grid_model.sort.keys = vec![dataforge_state::data_grid::SortKey {
                column_id: col_id,
                direction: dir,
            }];
            if ws.controller.active_table.is_some() {
                let _ = ws
                    .controller
                    .query_active_table_page_blocking(&PagePosition::First);
            } else {
                ws.controller.grid_model.sort_client_rows();
                ws.controller.sync_grid_view_from_model();
            }
        }
        "sort-clear" => {
            ws.controller.grid_model.sort.clear();
            if ws.controller.active_table.is_some() {
                let _ = ws
                    .controller
                    .query_active_table_page_blocking(&PagePosition::First);
            }
        }
        "freeze" => {
            let currently_frozen = ws
                .controller
                .grid_model
                .columns
                .columns()
                .get(display_pos)
                .is_some_and(|c| c.is_frozen);
            if currently_frozen {
                ws.controller.grid_model.columns.unfreeze_all();
            } else if !ws
                .controller
                .grid_model
                .columns
                .freeze_up_to(display_pos, f64::from(viewport_w))
            {
                ws.notify_toast(
                    ToastSeverity::Warning,
                    "Frozen columns cannot exceed 2/3 of viewport width",
                    None,
                    cx,
                );
            }
            ws.controller.sync_grid_view_from_model();
        }
        "auto-fit" => {
            let max_chars = ws
                .controller
                .grid_rows
                .iter()
                .take(100)
                .filter_map(|r| r.cells.get(data_idx))
                .map(|c| c.text.chars().count())
                .fold(col_title.chars().count(), usize::max);
            let width = ((max_chars as f32) * 8.0 + 36.0).clamp(80.0, 420.0);
            ws.controller.resize_grid_column(display_pos, width);
        }
        "hide" => {
            ws.data_grid_ui.hidden_columns.insert(col_id);
        }
        _ => {}
    }
    cx.notify();
}

fn handle_cell_context_action(
    ws: &mut DataForgeWorkspace,
    action: &str,
    row_idx: usize,
    col_idx: usize,
    window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) {
    match action {
        "edit" => open_inline_editor(ws, row_idx, col_idx, cx),
        "set-null" => set_edited_cell_value(ws, row_idx, col_idx, Value::Null),
        "copy-cell" => {
            let text = resolve_cell_text(ws, row_idx, col_idx);
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            ws.notify_toast(ToastSeverity::Info, "Cell copied", None, cx);
        }
        "copy-row" => {
            if let Some(row) = ws.controller.grid_rows.get(row_idx) {
                let line: String = row
                    .cells
                    .iter()
                    .map(|c| {
                        if c.kind == CellKind::Null {
                            "NULL"
                        } else {
                            c.text.as_str()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\t");
                cx.write_to_clipboard(ClipboardItem::new_string(line));
                ws.notify_toast(ToastSeverity::Info, "Row copied as TSV", None, cx);
            }
        }
        "delete-row" => delete_row_at_index(ws, row_idx),
        _ => {}
    }
    let _ = window;
    cx.notify();
}

fn open_inline_editor(
    ws: &mut DataForgeWorkspace,
    row_idx: usize,
    col_idx: usize,
    cx: &mut Context<DataForgeWorkspace>,
) {
    let text = resolve_raw_value(ws, row_idx, col_idx)
        .filter(|v| !v.is_null())
        .map(|v| format_core_value(&v))
        .unwrap_or_default();

    ws.data_grid_ui.inline_input.update(cx, |input, cx| {
        input.set_value(text.clone());
        cx.notify();
    });
    ws.workspace_pane.inline_edit = Some(InlineCellEditState {
        row_index: row_idx,
        col_index: col_idx,
        draft_text: text.clone(),
        original_text: text,
        set_null: false,
    });
    ws.data_grid_ui.inline_focus_requested = Some((row_idx, col_idx));
}

fn commit_inline_edit(ws: &mut DataForgeWorkspace, draft: String, set_null: bool) {
    let Some(edit) = ws.workspace_pane.inline_edit.take() else {
        return;
    };
    let val = if set_null {
        Value::Null
    } else {
        Value::Text(draft)
    };
    set_edited_cell_value(ws, edit.row_index, edit.col_index, val);
}

fn set_edited_cell_value(
    ws: &mut DataForgeWorkspace,
    row_idx: usize,
    display_pos: usize,
    val: Value,
) {
    let data_idx = ws
        .controller
        .grid_model
        .columns
        .columns()
        .get(display_pos)
        .map_or(display_pos, |c| c.data_index);
    let raw_count = ws.controller.grid_raw_rows.len();
    if row_idx < raw_count {
        ws.controller.edit_grid_cell(row_idx, display_pos, val);
    } else if let Some(inserted_row) = ws
        .controller
        .grid_model
        .edits
        .inserted_rows
        .values_mut()
        .nth(row_idx - raw_count)
        && let Some(slot) = inserted_row.get_mut(data_idx)
    {
        *slot = val;
        ws.controller.sync_grid_view_from_model();
    }
}

fn delete_row_at_index(ws: &mut DataForgeWorkspace, row_idx: usize) {
    let raw_count = ws.controller.grid_raw_rows.len();
    if row_idx < raw_count {
        ws.controller.delete_grid_row(row_idx);
    } else if let Some(key) = ws
        .controller
        .grid_model
        .edits
        .inserted_rows
        .keys()
        .nth(row_idx - raw_count)
        .copied()
    {
        ws.controller.grid_model.edits.delete_row(key);
        ws.controller.sync_grid_view_from_model();
    }
}

fn apply_pending_filter(ws: &mut DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) {
    let col_name = ws
        .controller
        .grid_columns
        .get(ws.data_grid_ui.filter_col_idx)
        .map(|c| c.title.clone());
    let Some(col_name) = col_name else {
        return;
    };
    let op = ws.data_grid_ui.filter_op;
    let val_opt = if matches!(op, FilterOp::IsNull | FilterOp::IsNotNull) {
        None
    } else {
        Some(
            ws.data_grid_ui
                .filter_value_input
                .read(cx)
                .value()
                .to_owned(),
        )
    };
    match ws
        .controller
        .add_grid_filter_blocking(&col_name, op, val_opt)
    {
        Ok(()) => {
            ws.data_grid_ui.filters_popover_open = false;
            ws.data_grid_ui.filter_value_input.update(cx, |s, cx| {
                s.set_value("");
                cx.notify();
            });
        }
        Err(err) => {
            ws.notify_toast(
                ToastSeverity::Error,
                "Filter error",
                Some(SharedString::from(err.to_string())),
                cx,
            );
        }
    }
    cx.notify();
}

fn navigate_to_foreign_key(
    ws: &mut DataForgeWorkspace,
    fk: &ForeignKeyInfo,
    key_value: &str,
    cx: &mut Context<DataForgeWorkspace>,
) {
    let target = TableRef {
        schema: fk.ref_schema.clone(),
        name: fk.ref_table.clone(),
    };
    if let Err(err) = ws.controller.open_table_tab_blocking(target) {
        ws.notify_toast(
            ToastSeverity::Error,
            "Failed to open referenced table",
            Some(SharedString::from(err.to_string())),
            cx,
        );
        return;
    }
    if let Some(ref_col) = fk.ref_columns.first() {
        let _ = ws.controller.add_grid_filter_blocking(
            ref_col,
            FilterOp::Eq,
            Some(key_value.to_owned()),
        );
    }
    cx.notify();
}

fn resolve_raw_value(ws: &DataForgeWorkspace, row_idx: usize, data_idx: usize) -> Option<Value> {
    let raw_count = ws.controller.grid_raw_rows.len();
    if row_idx < raw_count {
        let rid = ws.controller.grid_model.row_order.row_id(row_idx);
        let col_id = ws
            .controller
            .grid_model
            .columns
            .columns()
            .iter()
            .find(|c| c.data_index == data_idx)
            .map(|c| c.id);
        if let (Some(r), Some(c)) = (rid, col_id)
            && let Some(edited) = ws.controller.grid_model.edits.modified_cells.get(&(r, c))
        {
            return Some(edited.clone());
        }
        return ws
            .controller
            .grid_raw_rows
            .get(row_idx)
            .and_then(|r| r.get(data_idx))
            .cloned();
    }
    ws.controller
        .grid_model
        .edits
        .inserted_rows
        .values()
        .nth(row_idx - raw_count)
        .and_then(|r| r.get(data_idx))
        .cloned()
}

fn resolve_cell_text(ws: &DataForgeWorkspace, row_idx: usize, display_pos: usize) -> String {
    let data_idx = ws
        .controller
        .grid_model
        .columns
        .columns()
        .get(display_pos)
        .map_or(display_pos, |c| c.data_index);
    match resolve_raw_value(ws, row_idx, data_idx) {
        Some(v) => format_core_value(&v),
        None => ws
            .controller
            .grid_rows
            .get(row_idx)
            .and_then(|r| r.cells.get(data_idx))
            .map(|c| c.text.clone())
            .unwrap_or_default(),
    }
}

fn is_cell_selected(
    ws: &DataForgeWorkspace,
    row_idx: usize,
    display_pos: usize,
    row_id_opt: Option<dataforge_state::data_grid::RowID>,
    col_id: DataGridColumnID,
) -> bool {
    if let Some(rid) = row_id_opt {
        let sel = &ws.controller.grid_model.selection;
        if sel.selected_rows.contains(&rid) && sel.selected_cols.contains(&col_id) {
            return true;
        }
    }
    ws.controller.grid_selection.contains(row_idx, display_pos)
}

fn find_fk_for_column<'a>(fks: &'a [ForeignKeyInfo], col_name: &str) -> Option<&'a ForeignKeyInfo> {
    fks.iter()
        .find(|fk| fk.columns.first().is_some_and(|c| c == col_name))
}

fn compute_variable_col_slice(
    cols: &[(usize, DataGridColumn)],
    scroll_x: f32,
    viewport_w: f32,
) -> (usize, usize, f32, f32) {
    let mut start = 0usize;
    let mut acc_x = 0.0f32;
    while start < cols.len() {
        let w = cols[start].1.width as f32;
        if acc_x + w > scroll_x {
            break;
        }
        acc_x += w;
        start += 1;
    }
    let left_pad = acc_x;

    let mut end = start;
    let right_edge = scroll_x + viewport_w;
    while end < cols.len() && acc_x < right_edge {
        acc_x += cols[end].1.width as f32;
        end += 1;
    }
    let right_pad: f32 = cols[end..].iter().map(|(_, c)| c.width as f32).sum();
    (start, end, left_pad, right_pad)
}

fn filter_op_picker_items() -> Vec<PickerItem> {
    [
        FilterOp::Eq,
        FilterOp::NotEq,
        FilterOp::Contains,
        FilterOp::StartsWith,
        FilterOp::EndsWith,
        FilterOp::Lt,
        FilterOp::LtEq,
        FilterOp::Gt,
        FilterOp::GtEq,
        FilterOp::IsNull,
        FilterOp::IsNotNull,
    ]
    .into_iter()
    .map(|op| PickerItem::new(filter_op_key(op), filter_op_label(op)))
    .collect()
}

fn filter_op_key(op: FilterOp) -> &'static str {
    match op {
        FilterOp::Eq => "eq",
        FilterOp::NotEq => "neq",
        FilterOp::Contains => "contains",
        FilterOp::StartsWith => "starts_with",
        FilterOp::EndsWith => "ends_with",
        FilterOp::Lt => "lt",
        FilterOp::LtEq => "lte",
        FilterOp::Gt => "gt",
        FilterOp::GtEq => "gte",
        FilterOp::IsNull => "is_null",
        FilterOp::IsNotNull => "is_not_null",
        _ => "eq",
    }
}

fn parse_filter_op_key(key: &str) -> Option<FilterOp> {
    match key {
        "eq" => Some(FilterOp::Eq),
        "neq" => Some(FilterOp::NotEq),
        "contains" => Some(FilterOp::Contains),
        "starts_with" => Some(FilterOp::StartsWith),
        "ends_with" => Some(FilterOp::EndsWith),
        "lt" => Some(FilterOp::Lt),
        "lte" => Some(FilterOp::LtEq),
        "gt" => Some(FilterOp::Gt),
        "gte" => Some(FilterOp::GtEq),
        "is_null" => Some(FilterOp::IsNull),
        "is_not_null" => Some(FilterOp::IsNotNull),
        _ => None,
    }
}

fn filter_op_label(op: FilterOp) -> &'static str {
    match op {
        FilterOp::Eq => "=",
        FilterOp::NotEq => "!=",
        FilterOp::Contains => "contains",
        FilterOp::StartsWith => "starts with",
        FilterOp::EndsWith => "ends with",
        FilterOp::Lt => "<",
        FilterOp::LtEq => "<=",
        FilterOp::Gt => ">",
        FilterOp::GtEq => ">=",
        FilterOp::IsNull => "IS NULL",
        FilterOp::IsNotNull => "IS NOT NULL",
        _ => "=",
    }
}

fn tnum_features() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".to_owned(), 1)]))
}
