//! Non-blocking right-side Row & Column Inspector drawer displaying full
//! untruncated cell values, schema constraints, inline typed editing, and FK jumps.

use std::time::Duration;

use dataforge_gfx::grid_renderer::{CellKind, RowState};
use dataforge_state::connection_window::WorkspaceId;
use dataforge_state::data_grid::RowID;
use herogpui::components::anim::{Curve, Edge, Motion, entering_from};
use herogpui::components::{
    Button, Card, CardContent, Chip, ChipLabel, ChipVariant, Icon, IconName, IconSize, InputState,
    Scrollbar, Separator, Switch, TextArea, Variant,
};
use herogpui::core::{Color as HeroColor, Size as HeroSize, with_alpha};
use herogpui::gpui::prelude::FluentBuilder as _;
use herogpui::gpui::{
    AnyElement, App, AppContext as _, AsyncApp, ClipboardItem, Context, Entity, InteractiveElement,
    IntoElement, ParentElement, ScrollHandle, SharedString, StatefulInteractiveElement, Styled,
    Task, WeakEntity, Window, div, px,
};
use herogpui::theme::ActiveTheme;
use tablepro_core::browse::RowReference;
use tablepro_core::column::{ColumnDefault, ColumnInfo, ColumnType, ReadForm, ResultColumn};
use tablepro_core::ddl::OutboundForeignKey;
use tablepro_core::edit::{EditContext, EditText, edit_text, parse_edit_text};
use tablepro_core::export::value_text;
use tablepro_core::filter::FilterValue;
use tablepro_core::meta::TableRef;
use tablepro_core::value::Value;
use tablepro_session::operation::{OperationError, OperationKind};

use super::DataForgeWorkspace;
use super::shell_state::{push_error_toast, push_success_toast};
use super::theme::{
    COMPACT_GAP_PX, CONTROL_RADIUS_PX, HAIRLINE_BORDER_PX, MICRO_GAP_PX, drawer_duration,
    hairline_border_color, muted_surface_bg, panel_surface_bg, value_gpui_font,
};

/// Resolved snapshot of the currently focused DataGrid cell and its row context.
struct InspectedCell {
    row_idx: usize,
    col_idx: usize,
    col_name: String,
    col_type: ColumnType,
    col_info: ColumnInfo,
    _value: Value,
    original_value: Value,
    full_text: String,
    is_null: bool,
    is_read_only: bool,
    read_only_hint: Option<String>,
    is_modified: bool,
    is_inserted: bool,
    is_deleted: bool,
    edit_context: EditContext,
    outbound_fk: Option<OutboundForeignKey>,
    is_fk_cell: bool,
    row_columns: Vec<ResultColumn>,
    row_values: Vec<Value>,
}

/// Persistent UI state for the right-hand Row Inspector drawer.
pub struct InspectorViewState {
    pub raw_pretty_mode: bool,
    editor_input: Entity<InputState>,
    scroll_handle: ScrollHandle,
    last_sync_sig: Option<(usize, usize, String, bool)>,
    parse_error: Option<String>,
    copied_feedback: bool,
    copied_gen: u64,
    copy_reset_task: Option<Task<()>>,
    cached_table_key: Option<(WorkspaceId, Option<TableRef>)>,
    metadata_loading: bool,
    cached_columns: Vec<ColumnInfo>,
    cached_fks: Vec<OutboundForeignKey>,
    metadata_task: Option<Task<()>>,
}

impl InspectorViewState {
    /// Initializes inspector drawer state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, cx: &mut Context<DataForgeWorkspace>) -> Self {
        let editor_input = cx.new(|cx| InputState::new(cx));
        Self {
            raw_pretty_mode: false,
            editor_input,
            scroll_handle: ScrollHandle::new(),
            last_sync_sig: None,
            parse_error: None,
            copied_feedback: false,
            copied_gen: 0,
            copy_reset_task: None,
            cached_table_key: None,
            metadata_loading: false,
            cached_columns: Vec::new(),
            cached_fks: Vec::new(),
            metadata_task: None,
        }
    }
}

fn ensure_metadata_sync(ws: &mut DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) {
    let key = (
        ws.controller.active_workspace,
        ws.controller.active_table.clone(),
    );
    if ws.inspector_ui.cached_table_key.as_ref() == Some(&key) {
        return;
    }
    ws.inspector_ui.cached_table_key = Some(key.clone());
    ws.inspector_ui.cached_columns.clear();
    ws.inspector_ui.cached_fks.clear();
    ws.inspector_ui.metadata_task = None;

    let (workspace_id, Some(table)) = key else {
        ws.inspector_ui.metadata_loading = false;
        return;
    };
    let Some(services) = ws.controller.services.clone() else {
        ws.inspector_ui.metadata_loading = false;
        return;
    };

    ws.inspector_ui.metadata_loading = true;
    let runtime = services.runtime.clone();
    let sessions = services.sessions.clone();
    ws.inspector_ui.metadata_task = Some(cx.spawn(
        async move |this: WeakEntity<DataForgeWorkspace>, cx: &mut AsyncApp| {
            let result = runtime
                .spawn(async move {
                    let session = {
                        let guard = sessions.read().await;
                        guard.session(workspace_id)
                    };
                    let Some(session) = session else {
                        return (Vec::new(), Vec::new());
                    };
                    let cols = session
                        .columns(table.clone())
                        .await
                        .map(|arc| arc.to_vec())
                        .unwrap_or_default();
                    let fk_table = table.clone();
                    let fks = session
                        .run(OperationKind::Metadata, move |conn, opts| {
                            let t = fk_table.clone();
                            async move {
                                conn.fetch_outbound_foreign_keys(&t, opts)
                                    .await
                                    .map_err(OperationError::from)
                            }
                        })
                        .await
                        .unwrap_or_default();
                    (cols, fks)
                })
                .await
                .unwrap_or_default();

            let _ = this.update(cx, |ws, cx| {
                ws.inspector_ui.cached_columns = result.0;
                ws.inspector_ui.cached_fks = result.1;
                ws.inspector_ui.metadata_loading = false;
                cx.notify();
            });
        },
    ));
}

impl InspectorViewState {
    fn sync_editor_if_needed(&mut self, cell: &InspectedCell, cx: &mut App) {
        let sig = (
            cell.row_idx,
            cell.col_idx,
            cell.full_text.clone(),
            cell.is_null,
        );
        if self.last_sync_sig.as_ref() == Some(&sig) {
            return;
        }
        self.last_sync_sig = Some(sig);
        self.parse_error = None;
        let next_text = if cell.is_null {
            String::new()
        } else {
            cell.full_text.clone()
        };
        self.editor_input.update(cx, |input, cx| {
            input.set_value(next_text);
            cx.notify();
        });
    }

    fn trigger_copy_feedback(&mut self, text: String, cx: &mut Context<DataForgeWorkspace>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied_gen = self.copied_gen.wrapping_add(1);
        let generation = self.copied_gen;
        self.copied_feedback = true;
        self.copy_reset_task = Some(cx.spawn(
            async move |this: WeakEntity<DataForgeWorkspace>, cx: &mut AsyncApp| {
                cx.background_executor()
                    .timer(Duration::from_millis(1400))
                    .await;
                let _ = this.update(cx, |ws, cx| {
                    if ws.inspector_ui.copied_gen == generation {
                        ws.inspector_ui.copied_feedback = false;
                        cx.notify();
                    }
                });
            },
        ));
        cx.notify();
    }
}

/// Renders the non-blocking `300px` right-hand Row Inspector drawer.
#[must_use]
pub fn render_inspector(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    ensure_metadata_sync(ws, cx);
    let inspected = resolve_inspected_cell(ws);
    if let Some(cell) = inspected.as_ref() {
        ws.inspector_ui.sync_editor_if_needed(cell, cx);
    } else {
        ws.inspector_ui.last_sync_sig = None;
    }

    let header = render_header(ws, inspected.is_some(), cx);
    let body = match inspected {
        Some(ref cell) => render_cell_body(ws, cell, cx),
        None => render_empty_state(&ws.controller.strings, cx),
    };

    let drawer = div()
        .id("workspace-inspector-panel")
        .w(px(300.0))
        .h_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .bg(panel_surface_bg(cx))
        .border_l(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .child(header)
        .child(Separator::new())
        .child(body);

    let ms = u64::try_from(drawer_duration(cx).as_millis()).unwrap_or(0);
    let motion = Motion {
        ms,
        scale: 1.0,
        curve: Curve::OutFluid,
    };
    entering_from(
        drawer,
        "workspace-inspector-slide",
        Edge::Right,
        px(28.0),
        motion,
        cx,
    )
}

fn render_header(
    ws: &DataForgeWorkspace,
    has_cell: bool,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let table_label = ws.controller.active_table.as_ref().map(|t| t.name.clone());

    let mut row = div()
        .h(px(40.0))
        .px(px(12.0))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(COMPACT_GAP_PX))
        .child(
            div()
                .min_w(px(0.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .text_xs()
                        .font_weight(herogpui::gpui::FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(ws.controller.strings.inspector_short),
                )
                .when_some(table_label, |el, tbl| {
                    el.child(
                        Chip::new()
                            .variant(ChipVariant::Soft)
                            .color(HeroColor::Default)
                            .size(HeroSize::Sm)
                            .child(ChipLabel::new().child(tbl)),
                    )
                }),
        );

    let mut actions = div().flex().items_center().gap(px(MICRO_GAP_PX));
    if has_cell {
        let pretty = ws.inspector_ui.raw_pretty_mode;
        let mode_text = if pretty {
            ws.controller.strings.pretty_mode
        } else {
            ws.controller.strings.raw_mode
        };
        actions = actions.child(
            Button::new("inspector-pretty-toggle")
                .variant(if pretty {
                    Variant::Secondary
                } else {
                    Variant::Ghost
                })
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _window, cx| {
                    ws.inspector_ui.raw_pretty_mode = !ws.inspector_ui.raw_pretty_mode;
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(Icon::new(IconName::Code).size(IconSize::Xs))
                        .child(mode_text),
                ),
        );
    }

    actions = actions.child(
        Button::new("inspector-close")
            .variant(Variant::Ghost)
            .size(HeroSize::Sm)
            .is_icon_only(true)
            .aria_label(ws.controller.strings.close_inspector_aria)
            .on_press(cx.listener(|ws, _, _window, cx| {
                ws.controller.layout_config.inspector_visible = false;
                ws.controller.toolbar_status.inspector_open = false;
                ws.controller.sync_layout_counts();
                cx.notify();
            }))
            .child(Icon::new(IconName::X).size(IconSize::Sm)),
    );

    row = row.child(actions);
    row.into_any_element()
}

fn render_empty_state(s: &crate::i18n::Strings, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .flex_1()
        .p(px(20.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .child(
            div()
                .w(px(36.0))
                .h(px(36.0))
                .rounded(px(999.0))
                .bg(muted_surface_bg(cx))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(IconName::Rows2)
                        .size(IconSize::Md)
                        .color(colors.muted),
                ),
        )
        .child(
            div()
                .text_xs()
                .font_weight(herogpui::gpui::FontWeight::MEDIUM)
                .text_color(colors.foreground)
                .text_center()
                .child(s.inspector_empty_title),
        )
        .child(
            div()
                .text_xs()
                .text_color(colors.muted)
                .text_center()
                .child(s.inspector_empty_desc),
        )
        .into_any_element()
}

fn render_cell_body(
    ws: &DataForgeWorkspace,
    cell: &InspectedCell,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let scroll_handle = ws.inspector_ui.scroll_handle.clone();
    let content = div()
        .w_full()
        .p(px(12.0))
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(render_column_header_block(cell, &ws.controller.strings, cx))
        .child(render_value_viewer_card(ws, cell, cx))
        .when_some(render_fk_jump_row(ws, cell, cx), |el, fk_row| {
            el.child(fk_row)
        })
        .child(render_inline_editor_section(ws, cell, cx))
        .child(Separator::new())
        .child(render_row_summary_list(ws, cell, cx));

    div()
        .relative()
        .flex_1()
        .min_h(px(0.0))
        .child(
            div()
                .id("inspector-scroll-body")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&scroll_handle)
                .child(content),
        )
        .child(Scrollbar::new("inspector-scrollbar", scroll_handle))
        .into_any_element()
}

fn build_column_chips(cell: &InspectedCell, s: &crate::i18n::Strings) -> AnyElement {
    let mut chips = div().flex().flex_wrap().gap(px(MICRO_GAP_PX));
    let type_label = if cell.col_type.name.is_empty() {
        s.tx_unknown.to_owned()
    } else {
        cell.col_type.name.clone()
    };
    chips = chips.child(meta_chip(type_label, HeroColor::Default));
    chips = chips.child(meta_chip(
        if cell.col_info.nullable {
            s.nullable_badge
        } else {
            s.not_null_badge
        },
        if cell.col_info.nullable {
            HeroColor::Default
        } else {
            HeroColor::Warning
        },
    ));
    if cell.col_info.primary_key {
        chips = chips.child(meta_chip("PK", HeroColor::Accent));
    }
    if cell.outbound_fk.is_some() || cell.is_fk_cell {
        chips = chips.child(meta_chip("FK", HeroColor::Accent));
    }
    if cell.is_modified {
        chips = chips.child(meta_chip(s.cell_state_modified, HeroColor::Warning));
    } else if cell.is_inserted {
        chips = chips.child(meta_chip(s.cell_state_inserted, HeroColor::Success));
    } else if cell.is_deleted {
        chips = chips.child(meta_chip(s.cell_state_deleted, HeroColor::Danger));
    }
    chips.into_any_element()
}

fn meta_chip(label: impl Into<SharedString>, color: HeroColor) -> AnyElement {
    Chip::new()
        .variant(ChipVariant::Soft)
        .color(color)
        .size(HeroSize::Sm)
        .child(ChipLabel::new().child(label.into()))
        .into_any_element()
}

fn render_column_header_block(
    cell: &InspectedCell,
    s: &crate::i18n::Strings,
    cx: &App,
) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(6.0))
                .child(
                    div()
                        .min_w(px(0.0))
                        .text_sm()
                        .font_weight(herogpui::gpui::FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(cell.col_name.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted)
                        .child(s.row_number_tpl.replace("{num}", &(cell.row_idx + 1).to_string())),
                ),
        )
        .child(build_column_chips(cell, s))
        .into_any_element()
}

fn render_copy_value_button(
    ws: &DataForgeWorkspace,
    cell: &InspectedCell,
    display_text: &str,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let copy_payload = if cell.is_null {
        "NULL".to_owned()
    } else {
        display_text.to_owned()
    };
    let copied = ws.inspector_ui.copied_feedback;

    Button::new("inspector-copy-value")
        .variant(if copied {
            Variant::Secondary
        } else {
            Variant::Ghost
        })
        .size(HeroSize::Sm)
        .on_press(cx.listener(move |ws, _, _window, cx| {
            ws.inspector_ui
                .trigger_copy_feedback(copy_payload.clone(), cx);
        }))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(
                    Icon::new(if copied {
                        IconName::Check
                    } else {
                        IconName::Copy
                    })
                    .size(IconSize::Xs)
                    .color(if copied {
                        colors.success.color
                    } else {
                        colors.foreground
                    }),
                )
                .child(if copied {
                    ws.controller.strings.copied
                } else {
                    ws.controller.strings.copy_label
                }),
        )
        .into_any_element()
}

fn render_value_viewer_card(
    ws: &DataForgeWorkspace,
    cell: &InspectedCell,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let value_font = value_gpui_font(&ws.controller.settings);
    let display_text = format_display_value(cell, ws.inspector_ui.raw_pretty_mode);
    let copy_button = render_copy_value_button(ws, cell, &display_text, cx);

    let value_surface = div()
        .id("inspector-value-scroll")
        .w_full()
        .max_h(px(220.0))
        .overflow_y_scroll()
        .p(px(8.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(ws.content_palette.editor_background)
        .border_1()
        .border_color(hairline_border_color(cx))
        .font(value_font)
        .text_xs()
        .text_color(if cell.is_null {
            ws.content_palette.editor_syntax_null
        } else {
            ws.content_palette.editor_text
        })
        .when(cell.is_null, |el| el.italic())
        .child(if cell.is_null {
            "NULL".to_owned()
        } else if display_text.is_empty() {
            ws.controller.strings.empty_string_placeholder.to_owned()
        } else {
            display_text
        });

    let value_char_count = if cell.is_null {
        0
    } else {
        cell.full_text.chars().count()
    };

    Card::new()
        .child(
            CardContent::new()
                .sx(|d| d.p(px(10.0)).gap(px(6.0)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(herogpui::gpui::FontWeight::MEDIUM)
                                .text_color(colors.muted)
                                .child(
                                    ws.controller
                                        .strings
                                        .inspector_value_chars_tpl
                                        .replace("{count}", &value_char_count.to_string()),
                                ),
                        )
                        .child(copy_button),
                )
                .child(value_surface),
        )
        .into_any_element()
}

fn render_fk_jump_row(
    ws: &DataForgeWorkspace,
    cell: &InspectedCell,
    cx: &mut Context<DataForgeWorkspace>,
) -> Option<AnyElement> {
    if let Some(fk) = cell.outbound_fk.as_ref() {
        let target_label = if let Some(schema) = fk.key.ref_schema.as_ref() {
            format!("{schema}.{}", fk.key.ref_table)
        } else {
            fk.key.ref_table.clone()
        };
        let source_table = ws.controller.active_table.clone()?;
        let fk_clone = fk.clone();
        let row_cols = cell.row_columns.clone();
        let row_vals = cell.row_values.clone();
        let can_jump = !cell.is_null;

        return Some(
            Button::new("inspector-fk-jump")
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .full_width(true)
                .is_disabled(!can_jump)
                .on_press(cx.listener(move |ws, _, _window, cx| {
                    jump_to_foreign_key(ws, &source_table, &fk_clone, &row_cols, &row_vals, cx);
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(6.0))
                        .child(Icon::new(IconName::Link2).size(IconSize::Xs))
                        .child(if can_jump {
                            ws.controller
                                .strings
                                .fk_jump_to_tpl
                                .replace("{target}", &target_label)
                        } else {
                            ws.controller
                                .strings
                                .fk_null_target_tpl
                                .replace("{target}", &target_label)
                        }),
                )
                .into_any_element(),
        );
    }

    if cell.is_fk_cell {
        let colors = cx.colors().clone();
        return Some(
            div()
                .px(px(10.0))
                .py(px(6.0))
                .rounded(px(CONTROL_RADIUS_PX))
                .bg(muted_surface_bg(cx))
                .text_xs()
                .text_color(colors.muted)
                .child(if ws.inspector_ui.metadata_loading {
                    ws.controller.strings.fk_resolving_metadata
                } else {
                    ws.controller.strings.fk_metadata_unavailable_adhoc
                })
                .into_any_element(),
        );
    }

    None
}

fn jump_to_foreign_key(
    ws: &mut DataForgeWorkspace,
    source_table: &TableRef,
    fk: &OutboundForeignKey,
    columns: &[ResultColumn],
    values: &[Value],
    cx: &mut Context<DataForgeWorkspace>,
) {
    let reference = match RowReference::referenced(source_table, fk, columns, values) {
        Ok(r) => r,
        Err(err) => {
            push_error_toast(
                ws.controller.strings.fk_jump_unavailable_toast,
                Some(SharedString::from(err.to_string())),
                cx,
            );
            return;
        }
    };
    if let Err(err) = ws
        .controller
        .open_table_tab_blocking(reference.table.clone())
    {
        push_error_toast(
            ws.controller.strings.fk_open_failed_toast,
            Some(SharedString::from(err.to_string())),
            cx,
        );
        return;
    }
    for rule in reference.filter.rules {
        let val = match rule.value {
            Some(FilterValue::Single(v)) => Some(v),
            Some(FilterValue::Pair(a, _)) => Some(a),
            Some(FilterValue::List(items)) => items.into_iter().next(),
            None => None,
        };
        if let Err(err) = ws
            .controller
            .add_grid_filter_blocking(&rule.column, rule.op, val)
        {
            push_error_toast(
                ws.controller.strings.fk_filter_failed_toast,
                Some(SharedString::from(err.to_string())),
                cx,
            );
            return;
        }
    }
    push_success_toast(
        ws.controller.strings.fk_opened_row_toast,
        Some(SharedString::from(reference.table.name)),
        cx,
    );
    cx.notify();
}

fn render_inline_editor_section(
    ws: &DataForgeWorkspace,
    cell: &InspectedCell,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let row_idx = cell.row_idx;
    let col_idx = cell.col_idx;
    let col_info = cell.col_info.clone();
    let edit_ctx = cell.edit_context;
    let orig_val = cell.original_value.clone();
    let value_font = value_gpui_font(&ws.controller.settings);

    let null_switch = Switch::new("inspector-null-switch")
        .size(HeroSize::Sm)
        .is_selected(cell.is_null)
        .is_disabled(!cell.col_info.nullable || cell.is_read_only || cell.is_deleted)
        .label(ws.controller.strings.set_null)
        .on_change(cx.listener(move |ws, &make_null: &bool, _window, cx| {
            let next_val = if make_null {
                Value::Null
            } else {
                let draft = ws.inspector_ui.editor_input.read(cx).value().to_owned();
                match parse_edit_text(&draft, &col_info, edit_ctx, Some(&orig_val)) {
                    Ok(cell_in) => cell_in.value().cloned().unwrap_or(Value::Text(draft)),
                    Err(_) => Value::Text(String::new()),
                }
            };
            apply_inspector_cell_edit(ws, row_idx, col_idx, next_val);
            ws.inspector_ui.parse_error = None;
            cx.notify();
        }));

    let err_msg = ws.inspector_ui.parse_error.clone();
    let mut area = TextArea::new(&ws.inspector_ui.editor_input)
        .rows(4)
        .font_family(value_font.family)
        .full_width()
        .is_read_only(cell.is_read_only || cell.is_null || cell.is_deleted)
        .is_invalid(err_msg.is_some());
    if let Some(hint) = cell.read_only_hint.as_ref() {
        area = area.description(hint.clone());
    }
    if let Some(msg) = err_msg {
        area = area.error_message(msg);
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
                        .text_xs()
                        .font_weight(herogpui::gpui::FontWeight::MEDIUM)
                        .text_color(colors.muted)
                        .child(ws.controller.strings.edit_cell),
                )
                .child(null_switch),
        )
        .child(area)
        .child(render_inline_editor_actions(ws, cell, cx))
        .into_any_element()
}

fn render_inline_editor_actions(
    ws: &DataForgeWorkspace,
    cell: &InspectedCell,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let row_idx = cell.row_idx;
    let col_idx = cell.col_idx;
    let edit_ctx = cell.edit_context;
    let apply_col = cell.col_info.clone();
    let apply_orig = cell.original_value.clone();
    let apply_btn = Button::new("inspector-apply-edit")
        .variant(Variant::Primary)
        .size(HeroSize::Sm)
        .is_disabled(cell.is_read_only || cell.is_null || cell.is_deleted)
        .on_press(cx.listener(move |ws, _, _window, cx| {
            let draft = ws.inspector_ui.editor_input.read(cx).value().to_owned();
            match parse_edit_text(&draft, &apply_col, edit_ctx, Some(&apply_orig)) {
                Ok(cell_input) => {
                    let next_value = cell_input.value().cloned().unwrap_or(Value::Text(draft));
                    apply_inspector_cell_edit(ws, row_idx, col_idx, next_value);
                    ws.inspector_ui.parse_error = None;
                    cx.notify();
                }
                Err(err) => {
                    ws.inspector_ui.parse_error = Some(err.to_string());
                    cx.notify();
                }
            }
        }))
        .child(ws.controller.strings.apply_edit_label);

    let revert_orig = cell.original_value.clone();
    let show_revert = cell.is_modified || ws.inspector_ui.parse_error.is_some();

    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(apply_btn)
        .when(show_revert, |el| {
            el.child(
                Button::new("inspector-revert-edit")
                    .variant(Variant::Ghost)
                    .size(HeroSize::Sm)
                    .on_press(cx.listener(move |ws, _, _window, cx| {
                        apply_inspector_cell_edit(ws, row_idx, col_idx, revert_orig.clone());
                        ws.inspector_ui.parse_error = None;
                        ws.inspector_ui.last_sync_sig = None;
                        cx.notify();
                    }))
                    .child(ws.controller.strings.revert_label),
            )
        })
        .into_any_element()
}

fn render_row_summary_list(
    ws: &DataForgeWorkspace,
    active: &InspectedCell,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let value_font = value_gpui_font(&ws.controller.settings);
    let mut list = div().flex().flex_col().gap(px(4.0)).child(
        div()
            .text_xs()
            .font_weight(herogpui::gpui::FontWeight::MEDIUM)
            .text_color(colors.muted)
            .child(
                ws.controller
                    .strings
                    .inspector_row_fields_tpl
                    .replace("{count}", &active.row_columns.len().to_string()),
            ),
    );

    for (idx, (col, val)) in active
        .row_columns
        .iter()
        .zip(active.row_values.iter())
        .enumerate()
    {
        let is_selected = idx == active.col_idx;
        let is_null = matches!(val, Value::Null);
        let preview = if is_null {
            "NULL".to_owned()
        } else {
            value_text(val, &col.column_type).unwrap_or_else(|| format!("{val:?}"))
        };
        let row_idx = active.row_idx;
        let bg = if is_selected {
            with_alpha(colors.accent.color, 0.12)
        } else {
            muted_surface_bg(cx)
        };
        let border = if is_selected {
            colors.accent.color
        } else {
            hairline_border_color(cx)
        };

        list = list.child(
            div()
                .id(("inspector-row-field", idx))
                .cursor_pointer()
                .px(px(8.0))
                .py(px(6.0))
                .rounded(px(CONTROL_RADIUS_PX))
                .bg(bg)
                .border_1()
                .border_color(border)
                .hover(|s| s.opacity(0.92))
                .on_click(cx.listener(move |ws, _, _window, cx| {
                    ws.controller.select_grid_cell(row_idx, idx);
                    ws.inspector_ui.parse_error = None;
                    cx.notify();
                }))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .text_xs()
                        .font_weight(herogpui::gpui::FontWeight::MEDIUM)
                        .text_color(colors.foreground)
                        .child(col.name.clone()),
                )
                .child(
                    div()
                        .font(value_font.clone())
                        .text_xs()
                        .text_color(if is_null {
                            ws.content_palette.editor_syntax_null
                        } else {
                            colors.muted
                        })
                        .when(is_null, |el| el.italic())
                        .truncate()
                        .child(preview),
                ),
        );
    }

    list.into_any_element()
}

fn apply_inspector_cell_edit(
    ws: &mut DataForgeWorkspace,
    row_idx: usize,
    col_idx: usize,
    new_value: Value,
) {
    let raw_len = ws.controller.grid_raw_rows.len();
    if row_idx < raw_len {
        ws.controller.edit_grid_cell(row_idx, col_idx, new_value);
        return;
    }
    let inserted_offset = row_idx - raw_len;
    if let Some((_row_id, values)) = ws
        .controller
        .grid_model
        .edits
        .inserted_rows
        .iter_mut()
        .nth(inserted_offset)
        && let Some(slot) = values.get_mut(col_idx)
    {
        *slot = new_value;
        ws.controller.sync_grid_view_from_model();
    }
}

fn format_display_value(cell: &InspectedCell, pretty: bool) -> String {
    if !pretty || cell.is_null {
        return cell.full_text.clone();
    }
    serde_json::from_str::<serde_json::Value>(&cell.full_text)
        .and_then(|v| serde_json::to_string_pretty(&v))
        .unwrap_or_else(|_| cell.full_text.clone())
}

fn resolve_inspected_cell(ws: &DataForgeWorkspace) -> Option<InspectedCell> {
    let (row_idx, mut col_idx) = ws.controller.grid_selection.focused?;
    let total_rows =
        ws.controller.grid_raw_rows.len() + ws.controller.grid_model.edits.inserted_rows.len();
    if row_idx >= total_rows {
        return None;
    }

    let row_columns = build_result_columns(ws);
    if row_columns.is_empty() {
        return None;
    }
    if col_idx >= row_columns.len() {
        col_idx = 0;
    }

    let (row_values, original_row, is_inserted, is_deleted, row_id) =
        extract_row_values(ws, row_idx, row_columns.len());
    let value = row_values.get(col_idx).cloned().unwrap_or(Value::Null);
    let original_value = original_row.get(col_idx).cloned().unwrap_or(Value::Null);

    let col_id = ws
        .controller
        .grid_model
        .columns
        .columns()
        .get(col_idx)
        .map(|c| c.id);
    let is_modified = matches!((row_id, col_id), (Some(rid), Some(cid))
        if ws.controller.grid_model.edits.modified_cells.contains_key(&(rid, cid)));

    let rcol = &row_columns[col_idx];
    let col_info = resolve_column_info(ws, &rcol.name, &rcol.column_type);
    let edit_txt = edit_text(&value, &rcol.column_type);
    let (full_text, type_read_only_hint) = match edit_txt {
        EditText::Editable(s) => (s, None),
        EditText::ReadOnly(reason) => (
            value_text(&value, &rcol.column_type).unwrap_or_else(|| format!("{value:?}")),
            Some(
                ws.controller
                    .strings
                    .read_only_reason_tpl
                    .replace("{reason}", &format!("{reason:?}")),
            ),
        ),
    };
    let generated_hint = (!col_info.is_writable())
        .then(|| ws.controller.strings.generated_col_readonly.to_owned());
    let read_only_hint = type_read_only_hint.or(generated_hint);
    let is_read_only = read_only_hint.is_some();

    let outbound_fk = ws
        .inspector_ui
        .cached_fks
        .iter()
        .find(|fk| fk.key.columns.iter().any(|c| c == &rcol.name))
        .cloned();
    let is_fk_cell = ws
        .controller
        .grid_rows
        .get(row_idx)
        .and_then(|r| r.cells.get(col_idx))
        .is_some_and(|c| matches!(c.kind, CellKind::ForeignKey))
        || ws
            .controller
            .grid_rows
            .get(row_idx)
            .is_some_and(|r| r.state == RowState::Modified && outbound_fk.is_some());

    Some(InspectedCell {
        row_idx,
        col_idx,
        col_name: rcol.name.clone(),
        col_type: rcol.column_type.clone(),
        col_info,
        is_null: matches!(value, Value::Null),
        _value: value,
        original_value,
        full_text,
        is_read_only,
        read_only_hint,
        is_modified,
        is_inserted,
        is_deleted,
        edit_context: if is_inserted {
            EditContext::InsertDraft
        } else {
            EditContext::UpdateRow
        },
        outbound_fk,
        is_fk_cell,
        row_columns,
        row_values,
    })
}

fn build_result_columns(ws: &DataForgeWorkspace) -> Vec<ResultColumn> {
    if !ws.controller.grid_raw_columns.is_empty() {
        return ws.controller.grid_raw_columns.clone();
    }
    ws.controller
        .grid_columns
        .iter()
        .map(|col| ResultColumn::new(&col.title, ColumnType::unknown()))
        .collect()
}

fn extract_row_values(
    ws: &DataForgeWorkspace,
    row_idx: usize,
    col_len: usize,
) -> (Vec<Value>, Vec<Value>, bool, bool, Option<RowID>) {
    let raw_len = ws.controller.grid_raw_rows.len();
    if row_idx < raw_len {
        let orig = ws
            .controller
            .grid_raw_rows
            .get(row_idx)
            .cloned()
            .unwrap_or_default();
        let row_id = ws.controller.grid_model.row_order.row_id(row_idx);
        let is_deleted =
            row_id.is_some_and(|id| ws.controller.grid_model.edits.deleted_rows.contains(&id));
        let mut effective = Vec::with_capacity(col_len);
        for c_idx in 0..col_len {
            let col_id = ws
                .controller
                .grid_model
                .columns
                .columns()
                .get(c_idx)
                .map(|c| c.id);
            let val = match (row_id, col_id) {
                (Some(rid), Some(cid)) => ws
                    .controller
                    .grid_model
                    .edits
                    .modified_cells
                    .get(&(rid, cid))
                    .cloned()
                    .or_else(|| orig.get(c_idx).cloned())
                    .unwrap_or(Value::Null),
                _ => orig.get(c_idx).cloned().unwrap_or(Value::Null),
            };
            effective.push(val);
        }
        return (effective, orig, false, is_deleted, row_id);
    }

    let inserted = ws
        .controller
        .grid_model
        .edits
        .inserted_rows
        .iter()
        .nth(row_idx - raw_len)
        .map(|(id, vals)| (*id, vals.clone()));
    match inserted {
        Some((rid, vals)) => (vals.clone(), vals, true, false, Some(rid)),
        None => (vec![Value::Null; col_len], Vec::new(), true, false, None),
    }
}

fn resolve_column_info(ws: &DataForgeWorkspace, name: &str, rcol_type: &ColumnType) -> ColumnInfo {
    if let Some(info) = ws
        .inspector_ui
        .cached_columns
        .iter()
        .find(|c| c.name == name)
    {
        return info.clone();
    }
    if let Some(sc) = ws
        .controller
        .structure_columns
        .iter()
        .find(|c| c.name == name)
    {
        return ColumnInfo {
            name: sc.name.clone(),
            column_type: if rcol_type.name.is_empty() {
                ColumnType {
                    name: sc.data_type.clone(),
                    kind: rcol_type.kind.clone(),
                    catalog: rcol_type.catalog.clone(),
                    dynamic_storage: rcol_type.dynamic_storage,
                    read_form: ReadForm::Native,
                }
            } else {
                rcol_type.clone()
            },
            nullable: sc.nullable,
            primary_key: sc.is_primary_key,
            is_auto_increment: false,
            is_generated: false,
            partition_scoped: false,
            default: ColumnDefault::None,
            comment: None,
        };
    }
    ColumnInfo {
        name: name.to_owned(),
        column_type: rcol_type.clone(),
        nullable: true,
        primary_key: false,
        is_auto_increment: false,
        is_generated: false,
        partition_scoped: false,
        default: ColumnDefault::None,
        comment: None,
    }
}
