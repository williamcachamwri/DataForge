//! Workspace toolbar hosting real SQL execution, grid edits, export, and database/schema switching.

use herogpui::components::{
    Badge, BadgeAnchor, BadgeLabel, BadgePlacement, BadgeVariant, Button, ButtonGroup, Chip,
    ChipLabel, ChipVariant, Color, Icon, IconName, Kbd, KbdAbbr, KbdKey, KbdVariant, Orientation,
    PickerItem, Popover, PopoverPlacement, Select, Separator, Size, Spinner, Toolbar, Tooltip,
    TooltipPlacement, Variant,
};
use herogpui::gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString,
    Styled, Window, div, px,
};
use herogpui::theme::ActiveTheme;

use crate::accessibility::KeyCommand;
use crate::app::{AppController, ScreenMode};
use crate::views::library::engine_from_driver_id;
use dataforge_gfx::glyphs::EngineKind;
use dataforge_state::connection_window::ConnectionWindowPhase;
use tablepro_core::SafeModeLevel;
use tablepro_core::meta::ObjectScope;

use super::DataForgeWorkspace;
use super::engine_icons::engine_icon;
use super::shell_state::ToastSeverity;
use super::theme::{
    CONTROL_RADIUS_PX, HAIRLINE_BORDER_PX, hairline_border_color, is_reduced_motion,
    muted_surface_bg,
};

const ALL_SCHEMAS_KEY: &str = "__all_schemas__";

struct ToolbarActionState {
    is_connected: bool,
    is_connecting: bool,
    is_running: bool,
    has_selection: bool,
    has_export_rows: bool,
    has_grid_columns: bool,
    pending_edits: usize,
    skip_anim: bool,
}

/// Renders the top workspace toolbar with live connection state, SQL controls,
/// grid transaction actions, and a real database/schema switcher popover.
#[must_use]
pub fn render_toolbar(
    ws: &mut DataForgeWorkspace,
    window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let phase = ws
        .controller
        .connections
        .phase(ws.controller.active_workspace)
        .cloned();
    let state = ToolbarActionState {
        is_connected: matches!(phase, Some(ConnectionWindowPhase::Connected)),
        is_connecting: matches!(
            phase,
            Some(
                ConnectionWindowPhase::Connecting { .. }
                    | ConnectionWindowPhase::Reconnecting { .. }
            )
        ),
        is_running: ws.active_tab_result().is_some_and(|r| r.running),
        has_selection: selected_sql_text(&ws.controller).is_some(),
        has_export_rows: !ws.controller.grid_rows.is_empty()
            || ws
                .active_tab_result()
                .is_some_and(|r| !r.accumulator.result_sets.is_empty()),
        has_grid_columns: !ws.controller.grid_columns.is_empty(),
        pending_edits: pending_edit_count(&ws.controller),
        skip_anim: is_reduced_motion(cx),
    };

    let bar = Toolbar::new()
        .id("workspace-toolbar")
        .orientation(Orientation::Horizontal)
        .size(Size::Sm)
        .gap(px(6.0))
        .full_width(true)
        .sx({
            let bg = muted_surface_bg(cx);
            let border = hairline_border_color(cx);
            move |s| {
                s.h(px(42.0))
                    .px(px(12.0))
                    .items_center()
                    .justify_between()
                    .bg(bg)
                    .border_b(px(HAIRLINE_BORDER_PX))
                    .border_color(border)
            }
        })
        .child(render_left_actions(&state, cx))
        .child(render_right_controls(&ws.controller, &phase, window, cx));

    bar.into_any_element()
}

fn render_left_actions(state: &ToolbarActionState, cx: &Context<DataForgeWorkspace>) -> AnyElement {
    div()
        .id("toolbar-left-cluster")
        .flex()
        .items_center()
        .gap(px(6.0))
        .min_w(px(0.0))
        .child(render_execution_controls(state, cx))
        .child(
            Separator::new()
                .id("toolbar-sep-sql")
                .orientation(Orientation::Vertical)
                .sx(|s| s.h(px(18.0))),
        )
        .child(render_sql_file_group(state.has_export_rows, cx))
        .child(
            Separator::new()
                .id("toolbar-sep-grid")
                .orientation(Orientation::Vertical)
                .sx(|s| s.h(px(18.0))),
        )
        .child(render_grid_edit_controls(
            state.is_connected,
            state.has_grid_columns,
            state.pending_edits,
            state.skip_anim,
            cx,
        ))
        .into_any_element()
}

fn render_sql_file_group(has_export_rows: bool, cx: &Context<DataForgeWorkspace>) -> AnyElement {
    let colors = cx.colors().clone();
    ButtonGroup::new()
        .id("toolbar-sql-group")
        .variant(Variant::Outline)
        .size(Size::Sm)
        .separators(true)
        .button(
            Button::new("toolbar-new-query")
                .aria_label("New Query Tab (Ctrl+T)")
                .on_press(cx.listener(|ws, _, window, cx| {
                    ws.execute_key_command(KeyCommand::NewQueryTab, window, cx);
                }))
                .child(
                    Icon::new(IconName::FilePlus)
                        .size(px(13.0))
                        .color(colors.foreground),
                )
                .child("New"),
        )
        .button(
            Button::new("toolbar-save-sql")
                .aria_label("Save SQL Query (Ctrl+S)")
                .on_press(cx.listener(|ws, _, _, cx| {
                    save_active_tab_query(ws, cx);
                }))
                .child(
                    Icon::new(IconName::Save)
                        .size(px(13.0))
                        .color(colors.foreground),
                )
                .child("Save"),
        )
        .button(
            Button::new("toolbar-export")
                .is_disabled(!has_export_rows)
                .aria_label("Export Data")
                .on_press(cx.listener(|ws, _, window, cx| {
                    ws.modals_ui.open_export(window, cx);
                    cx.notify();
                }))
                .child(
                    Icon::new(IconName::Download)
                        .size(px(13.0))
                        .color(if has_export_rows {
                            colors.foreground
                        } else {
                            colors.muted
                        }),
                )
                .child("Export"),
        )
        .into_any_element()
}

fn render_execution_controls(
    state: &ToolbarActionState,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    if !state.is_connected {
        return render_disconnected_execution(state.is_connecting, state.skip_anim, cx);
    }
    render_connected_execution(state.is_running, state.has_selection, state.skip_anim, cx)
}

fn render_disconnected_execution(
    is_connecting: bool,
    skip_anim: bool,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let primary = if is_connecting {
        Button::new("toolbar-connecting")
            .variant(Variant::Secondary)
            .size(Size::Sm)
            .is_disabled(true)
            .child(Spinner::new("toolbar-conn-spin").size_px(px(12.0)))
            .child("Connecting...")
            .into_any_element()
    } else {
        Button::new("toolbar-connect-cta")
            .variant(Variant::Primary)
            .size(Size::Sm)
            .aria_label("Connect to a database")
            .on_press(cx.listener(|ws, _, _, cx| {
                ws.switch_screen(ScreenMode::ConnectionsLibrary, cx);
            }))
            .child(
                Icon::new(IconName::Server)
                    .size(px(13.0))
                    .color(colors.accent.foreground),
            )
            .child("Connect...")
            .into_any_element()
    };

    let secondary = if is_connecting {
        Button::new("toolbar-cancel-connect")
            .variant(Variant::DangerSoft)
            .size(Size::Sm)
            .aria_label("Cancel Connection")
            .on_press(cx.listener(|ws, _, _, cx| {
                ws.controller.cancel_active_connect();
                cx.notify();
            }))
            .child(
                Icon::new(IconName::X)
                    .size(px(13.0))
                    .color(colors.danger.color),
            )
            .child("Cancel")
            .into_any_element()
    } else {
        Tooltip::new("Connect to a database to run queries")
            .id("toolbar-run-disabled-tip")
            .placement(TooltipPlacement::Bottom)
            .delay(250)
            .close_delay(80)
            .should_skip_animation(skip_anim)
            .child(
                Button::new("toolbar-run-disabled")
                    .variant(Variant::Secondary)
                    .size(Size::Sm)
                    .is_disabled(true)
                    .aria_label("Run Query (Disconnected)")
                    .child(Icon::new(IconName::Play).size(px(13.0)).color(colors.muted))
                    .child("Run"),
            )
            .into_any_element()
    };

    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(primary)
        .child(secondary)
        .into_any_element()
}

fn render_connected_execution(
    is_running: bool,
    has_selection: bool,
    skip_anim: bool,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let run_label = if has_selection {
        "Run Selection"
    } else {
        "Run"
    };
    let run_btn = Tooltip::new("Execute SQL query (Ctrl+Enter)")
        .id("toolbar-run-tip")
        .placement(TooltipPlacement::Bottom)
        .delay(350)
        .close_delay(80)
        .should_skip_animation(skip_anim)
        .child(
            Button::new("toolbar-run")
                .variant(Variant::Primary)
                .size(Size::Sm)
                .is_disabled(is_running)
                .aria_label(run_label)
                .on_press(cx.listener(|ws, _, _, cx| {
                    run_active_or_selected_sql(ws, cx);
                }))
                .child(
                    Icon::new(IconName::Play)
                        .size(px(13.0))
                        .color(colors.accent.foreground),
                )
                .child(run_label)
                .child(
                    Kbd::new()
                        .variant(KbdVariant::Light)
                        .sx(|s| s.h(px(18.0)).px(px(4.0)).text_size(px(10.0)))
                        .child(KbdAbbr::new(KbdKey::Ctrl))
                        .child(KbdAbbr::new(KbdKey::Enter)),
                ),
        );

    let cancel_btn = Tooltip::new("Cancel active operation")
        .id("toolbar-cancel-tip")
        .placement(TooltipPlacement::Bottom)
        .delay(350)
        .close_delay(80)
        .should_skip_animation(skip_anim)
        .child(
            Button::new("toolbar-cancel")
                .variant(if is_running {
                    Variant::DangerSoft
                } else {
                    Variant::Outline
                })
                .size(Size::Sm)
                .is_disabled(!is_running)
                .aria_label("Cancel Query")
                .on_press(cx.listener(|ws, _, _, cx| {
                    ws.controller.cancel_active_connect();
                    if let Some(tab_id) = ws.controller.tabs.active_tab() {
                        ws.ensure_tab_result(tab_id).running = false;
                    }
                    cx.notify();
                }))
                .child(if is_running {
                    Spinner::new("toolbar-run-spinner")
                        .size_px(px(12.0))
                        .color(Color::Danger)
                        .into_any_element()
                } else {
                    Icon::new(IconName::Square)
                        .size(px(12.0))
                        .color(colors.muted)
                        .into_any_element()
                })
                .child("Cancel"),
        );

    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(run_btn)
        .child(cancel_btn)
        .into_any_element()
}

fn render_grid_edit_controls(
    is_connected: bool,
    has_grid_columns: bool,
    pending_edits: usize,
    skip_anim: bool,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let can_add_row = is_connected && has_grid_columns;
    let can_commit = is_connected && pending_edits > 0;

    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(render_grid_edit_group(can_add_row, pending_edits, cx))
        .child(render_commit_badge_button(
            can_commit,
            pending_edits,
            skip_anim,
            cx,
        ))
        .into_any_element()
}

fn render_grid_edit_group(
    can_add_row: bool,
    pending_edits: usize,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    ButtonGroup::new()
        .id("toolbar-edit-group")
        .variant(Variant::Outline)
        .size(Size::Sm)
        .separators(true)
        .button(
            Button::new("toolbar-add-row")
                .is_disabled(!can_add_row)
                .aria_label("Add Row")
                .on_press(cx.listener(|ws, _, _, cx| {
                    let col_count = ws.controller.grid_columns.len();
                    if col_count == 0 {
                        return;
                    }
                    let values = vec![tablepro_core::Value::Null; col_count];
                    ws.controller.insert_grid_row(values);
                    cx.notify();
                }))
                .child(
                    Icon::new(IconName::Plus)
                        .size(px(13.0))
                        .color(if can_add_row {
                            colors.foreground
                        } else {
                            colors.muted
                        }),
                )
                .child("Add Row"),
        )
        .button(
            Button::new("toolbar-rollback")
                .is_disabled(pending_edits == 0)
                .aria_label("Rollback Pending Edits")
                .on_press(cx.listener(|ws, _, _, cx| {
                    ws.controller.rollback_grid_edits();
                    ws.workspace_pane.inline_edit = None;
                    cx.notify();
                }))
                .child(
                    Icon::new(IconName::RotateCcw)
                        .size(px(13.0))
                        .color(if pending_edits > 0 {
                            colors.foreground
                        } else {
                            colors.muted
                        }),
                )
                .child("Rollback"),
        )
        .into_any_element()
}

fn render_commit_badge_button(
    can_commit: bool,
    pending_edits: usize,
    skip_anim: bool,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let commit_btn = Button::new("toolbar-commit")
        .variant(if can_commit {
            Variant::Secondary
        } else {
            Variant::Outline
        })
        .size(Size::Sm)
        .is_disabled(!can_commit)
        .aria_label("Commit Pending Grid Edits")
        .on_press(cx.listener(|ws, _, _, cx| {
            ws.workspace_pane.confirm_commit_open = true;
            cx.notify();
        }))
        .child(
            Icon::new(IconName::Check)
                .size(px(13.0))
                .color(if can_commit {
                    colors.warning.color
                } else {
                    colors.muted
                }),
        )
        .child("Commit");

    let wrapped = if pending_edits > 0 {
        BadgeAnchor::new()
            .child(commit_btn)
            .child(
                Badge::new()
                    .color(Color::Warning)
                    .variant(BadgeVariant::Primary)
                    .size(Size::Sm)
                    .placement(BadgePlacement::TopRight)
                    .child(BadgeLabel::new().child(pending_edits.to_string())),
            )
            .into_any_element()
    } else {
        commit_btn.into_any_element()
    };

    Tooltip::new(if pending_edits > 0 {
        format!("Commit {pending_edits} uncommitted edit(s)")
    } else {
        "No uncommitted grid edits".to_owned()
    })
    .id("toolbar-commit-tip")
    .placement(TooltipPlacement::Bottom)
    .delay(350)
    .close_delay(80)
    .should_skip_animation(skip_anim)
    .child(wrapped)
    .into_any_element()
}

fn render_right_controls(
    controller: &AppController,
    phase: &Option<ConnectionWindowPhase>,
    window: &mut Window,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let status_node = match phase {
        Some(ConnectionWindowPhase::Connected) => {
            return render_connected_right_controls(controller, window, cx);
        }
        Some(ConnectionWindowPhase::Connecting { .. }) => {
            render_right_spinner_chip("toolbar-status-spin", "Connecting...")
        }
        Some(ConnectionWindowPhase::Reconnecting { .. }) => {
            render_right_spinner_chip("toolbar-reconn-spin", "Reconnecting...")
        }
        Some(ConnectionWindowPhase::Unavailable { .. }) => Chip::new()
            .variant(ChipVariant::Soft)
            .color(Color::Danger)
            .size(Size::Sm)
            .child(
                Icon::new(IconName::TriangleAlert)
                    .size(px(12.0))
                    .color(colors.danger.color),
            )
            .child(ChipLabel::new().child("Unavailable"))
            .into_any_element(),
        Some(ConnectionWindowPhase::Disconnected) | None => Chip::new()
            .variant(ChipVariant::Secondary)
            .color(Color::Default)
            .size(Size::Sm)
            .child(
                Icon::new(IconName::WifiOff)
                    .size(px(12.0))
                    .color(colors.muted),
            )
            .child(ChipLabel::new().child("Disconnected"))
            .into_any_element(),
    };

    div()
        .id("toolbar-right-cluster")
        .flex()
        .items_center()
        .gap(px(6.0))
        .flex_shrink_0()
        .child(status_node)
        .into_any_element()
}

fn render_right_spinner_chip(id: &'static str, label: &'static str) -> AnyElement {
    Chip::new()
        .variant(ChipVariant::Soft)
        .color(Color::Accent)
        .size(Size::Sm)
        .child(Spinner::new(id).size_px(px(12.0)))
        .child(ChipLabel::new().child(label))
        .into_any_element()
}

fn render_connected_right_controls(
    controller: &AppController,
    window: &mut Window,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let mut right = div()
        .id("toolbar-right-cluster")
        .flex()
        .items_center()
        .gap(px(6.0))
        .flex_shrink_0();

    if controller.active_safe_mode() != SafeModeLevel::Off {
        right = right.child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .color(Color::Warning)
                .size(Size::Sm)
                .child(
                    Icon::new(IconName::ShieldCheck)
                        .size(px(12.0))
                        .color(colors.warning.color),
                )
                .child(ChipLabel::new().child("Safe Mode")),
        );
    }

    right
        .child(render_db_switcher_popover(controller, window, cx))
        .child(
            Button::new("toolbar-refresh-schema")
                .variant(Variant::Ghost)
                .size(Size::Sm)
                .is_icon_only(true)
                .aria_label("Refresh Schema Objects")
                .on_press(cx.listener(|ws, _, _, cx| {
                    match ws.controller.refresh_schema_sidebar_blocking() {
                        Ok(()) => {
                            ws.notify_toast(ToastSeverity::Success, "Schema refreshed", None, cx);
                        }
                        Err(err) => {
                            ws.notify_toast(
                                ToastSeverity::Error,
                                "Schema refresh failed",
                                Some(err.to_string().into()),
                                cx,
                            );
                        }
                    }
                    cx.notify();
                }))
                .child(
                    Icon::new(IconName::RefreshCw)
                        .size(px(13.0))
                        .color(colors.muted),
                ),
        )
        .into_any_element()
}

fn render_db_switcher_popover(
    controller: &AppController,
    window: &mut Window,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let schema_items = build_schema_picker_items(controller);
    let selected_schema = controller
        .cached_objects
        .first()
        .and_then(|obj| obj.schema.clone())
        .or_else(|| {
            controller
                .cached_schemas
                .iter()
                .find(|s| s.is_default)
                .map(|s| s.name.clone())
        });
    let trigger = build_db_popover_trigger(controller, cx);
    let saved_connections = build_saved_connection_tuples(controller);
    let host_summary = if controller.toolbar_status.host.is_empty() {
        "Active connection".to_owned()
    } else {
        format!(
            "{} · {}",
            controller.toolbar_status.host, controller.toolbar_status.engine_version
        )
    };

    let _ = window;
    Popover::new(trigger)
        .id("toolbar-db-popover")
        .placement(PopoverPlacement::BottomEnd)
        .panel_width(px(300.0))
        .padding(px(12.0))
        .title("Database & Schema")
        .show_close_button(true)
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(colors.muted)
                        .truncate()
                        .child(host_summary),
                )
                .child(render_schema_select(&schema_items, selected_schema, cx))
                .child(render_saved_connections_section(saved_connections, cx))
                .child(
                    Separator::new()
                        .id("toolbar-popover-sep")
                        .orientation(Orientation::Horizontal),
                )
                .child(render_popover_footer_actions(cx)),
        )
        .into_any_element()
}

fn build_db_popover_trigger(
    controller: &AppController,
    cx: &Context<DataForgeWorkspace>,
) -> Button {
    let colors = cx.colors().clone();
    let db_label = active_db_label(controller);
    let engine = active_engine(controller);
    Button::new("toolbar-db-trigger")
        .variant(Variant::Outline)
        .size(Size::Sm)
        .height(px(26.0))
        .padding_x(px(8.0))
        .radius(px(CONTROL_RADIUS_PX))
        .aria_label("Database and schema switcher")
        .child(engine.map_or_else(
            || {
                Icon::new(IconName::Database)
                    .size(px(13.0))
                    .color(colors.foreground)
                    .into_any_element()
            },
            |kind| engine_icon(kind, px(13.0), colors.foreground),
        ))
        .child(
            div()
                .max_w(px(160.0))
                .truncate()
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .child(db_label),
        )
        .child(
            Icon::new(IconName::ChevronDown)
                .size(px(12.0))
                .color(colors.muted),
        )
}

fn build_saved_connection_tuples(
    controller: &AppController,
) -> Vec<(String, String, String, EngineKind)> {
    controller
        .library
        .storage()
        .connections
        .values()
        .map(|conn| {
            let sub = format!("{}: {}", conn.host, conn.database);
            (
                conn.id.0.clone(),
                conn.name.clone(),
                sub,
                engine_from_driver_id(&conn.driver_id),
            )
        })
        .collect()
}

fn render_schema_select(
    schema_items: &[PickerItem],
    selected_schema: Option<String>,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let selected_key = selected_schema
        .map(SharedString::from)
        .or_else(|| schema_items.first().map(|first| first.key().clone()));

    Select::new("toolbar-schema-select", schema_items.to_vec())
        .label("Active Schema")
        .placeholder("No schemas loaded")
        .height(px(30.0))
        .trigger_text_size(px(12.0))
        .row_text_size(px(12.0))
        .full_width(true)
        .is_disabled(schema_items.is_empty())
        .value(selected_key)
        .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _, cx| {
            let Some(schema_key) = key.as_ref() else {
                return;
            };
            let scope = if schema_key.as_ref() == ALL_SCHEMAS_KEY {
                ObjectScope::AllSchemas
            } else {
                ObjectScope::Schema(schema_key.to_string())
            };
            reload_objects_for_scope(ws, scope, cx);
        }))
        .into_any_element()
}

fn render_saved_connections_section(
    saved: Vec<(String, String, String, EngineKind)>,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let mut section = div().w_full().flex().flex_col().gap(px(4.0)).child(
        div()
            .text_size(px(11.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(colors.muted)
            .child("Saved Connections"),
    );

    if saved.is_empty() {
        return section
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(colors.muted)
                    .child("No saved connections in library."),
            )
            .into_any_element();
    }

    for (idx, (id, name, detail, engine)) in saved.into_iter().take(6).enumerate() {
        section = section.child(
            Button::new(format!("toolbar-switch-conn-{idx}"))
                .variant(Variant::Ghost)
                .size(Size::Sm)
                .full_width(true)
                .on_press(cx.listener(move |ws, _, _, cx| {
                    if let Err(err) = ws.controller.connect_library_item_blocking(&id) {
                        super::toasts::push_retry_connection_toast(
                            cx.entity().downgrade(),
                            "Connection switch failed",
                            err.to_string(),
                            cx,
                        );
                    }
                    cx.notify();
                }))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.0))
                        .min_w(px(0.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .min_w(px(0.0))
                                .child(engine_icon(engine, px(13.0), colors.foreground))
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(12.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(name),
                                ),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(11.0))
                                .text_color(colors.muted)
                                .child(detail),
                        ),
                ),
        );
    }

    section.into_any_element()
}

fn render_popover_footer_actions(cx: &Context<DataForgeWorkspace>) -> AnyElement {
    div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(6.0))
        .child(
            Button::new("toolbar-popover-connections")
                .variant(Variant::Secondary)
                .size(Size::Sm)
                .on_press(cx.listener(|ws, _, _, cx| {
                    ws.switch_screen(ScreenMode::ConnectionsLibrary, cx);
                }))
                .child("Manage..."),
        )
        .child(
            Button::new("toolbar-popover-disconnect")
                .variant(Variant::DangerSoft)
                .size(Size::Sm)
                .on_press(cx.listener(|ws, _, _, cx| {
                    if let Err(err) = ws.controller.disconnect_active_workspace() {
                        ws.notify_toast(
                            ToastSeverity::Error,
                            "Disconnect failed",
                            Some(err.to_string().into()),
                            cx,
                        );
                    }
                    cx.notify();
                }))
                .child("Disconnect"),
        )
        .into_any_element()
}

fn build_schema_picker_items(controller: &AppController) -> Vec<PickerItem> {
    if controller.cached_schemas.is_empty() {
        return Vec::new();
    }
    let mut items = vec![PickerItem::new(ALL_SCHEMAS_KEY, "All Schemas")];
    for schema in &controller.cached_schemas {
        let label = if schema.is_default {
            format!("{} (default)", schema.name)
        } else {
            schema.name.clone()
        };
        items.push(PickerItem::new(schema.name.clone(), label));
    }
    items
}

fn active_db_label(controller: &AppController) -> String {
    let db = controller.toolbar_status.database.trim();
    if !db.is_empty() && db != "—" {
        return db.to_owned();
    }
    let form_db = controller.connection_form.database.trim();
    if !form_db.is_empty() {
        return form_db.to_owned();
    }
    "Default Database".to_owned()
}

fn active_engine(controller: &AppController) -> Option<EngineKind> {
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

fn pending_edit_count(controller: &AppController) -> usize {
    let edits = &controller.grid_model.edits;
    edits.modified_cells.len() + edits.inserted_rows.len() + edits.deleted_rows.len()
}

fn save_active_tab_query(ws: &mut DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) {
    let title = ws
        .controller
        .tabs
        .active_tab()
        .and_then(|id| ws.controller.tabs.tabs().iter().find(|t| t.id == id))
        .map_or_else(|| "Query".to_owned(), |t| t.title.clone());
    ws.controller.save_current_query(&title, None);
    ws.notify_toast(
        ToastSeverity::Success,
        "Query saved",
        Some(title.into()),
        cx,
    );
    cx.notify();
}

fn run_active_or_selected_sql(ws: &mut DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) {
    let active_tab = ws.controller.tabs.active_tab();
    if let Some(tab_id) = active_tab {
        ws.ensure_tab_result(tab_id).reset_for_run();
    }
    let selected = selected_sql_text(&ws.controller);
    let outcome = match selected.as_deref() {
        Some(sql) => ws.controller.execute_sql_blocking(sql),
        None => ws.controller.execute_active_sql_blocking(),
    };
    match outcome {
        Ok(summary) => {
            if let Some(tab_id) = active_tab {
                let elapsed_ms = u64::try_from(summary.elapsed.as_millis()).unwrap_or(u64::MAX);
                ws.ensure_tab_result(tab_id).finish(summary, elapsed_ms);
            }
        }
        Err(err) => {
            if let Some(tab_id) = active_tab {
                ws.ensure_tab_result(tab_id).fail(err.to_string());
            }
            ws.notify_toast(
                ToastSeverity::Error,
                "Query failed",
                Some(err.to_string().into()),
                cx,
            );
        }
    }
    cx.notify();
}

fn reload_objects_for_scope(
    ws: &mut DataForgeWorkspace,
    scope: ObjectScope,
    cx: &mut Context<DataForgeWorkspace>,
) {
    let Some(services) = ws.controller.services.clone() else {
        return;
    };
    let active_ws = ws.controller.active_workspace;
    let res = services.runtime.block_on(async {
        let guard = services.sessions.read().await;
        if let Some(session) = guard.session(active_ws) {
            session
                .refresh_objects(scope)
                .await
                .map_err(|err| err.to_string())
        } else {
            guard
                .list_objects(active_ws, scope)
                .await
                .map_err(|err| err.to_string())
        }
    });
    match res {
        Ok(objects) => {
            ws.controller.cached_objects = objects.to_vec();
            ws.controller.rebuild_sidebar_rows();
        }
        Err(err) => {
            ws.notify_toast(
                ToastSeverity::Error,
                "Failed to load schema objects",
                Some(err.into()),
                cx,
            );
        }
    }
    cx.notify();
}

fn selected_sql_text(controller: &AppController) -> Option<String> {
    let sel = controller.editor_selections.first()?;
    let ((start_line, start_col), (end_line, end_col)) = if sel.anchor <= sel.head {
        (sel.anchor, sel.head)
    } else {
        (sel.head, sel.anchor)
    };
    if start_line == end_line && start_col == end_col {
        return None;
    }
    let mut out = String::new();
    for line_idx in start_line..=end_line {
        let line = controller.editor_lines.get(line_idx)?;
        let chars: Vec<char> = line.text.chars().collect();
        let from = if line_idx == start_line {
            start_col.min(chars.len())
        } else {
            0
        };
        let to = if line_idx == end_line {
            end_col.min(chars.len())
        } else {
            chars.len()
        };
        if line_idx > start_line {
            out.push('\n');
        }
        for ch in chars.get(from..to).unwrap_or_default() {
            out.push(*ch);
        }
    }
    if out.trim().is_empty() {
        None
    } else {
        Some(out)
    }
}
