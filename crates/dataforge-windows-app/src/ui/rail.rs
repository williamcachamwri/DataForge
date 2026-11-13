//! Left vertical navigation rail for screens, live rail workspaces, and panel toggles.

use herogpui::components::{
    Badge, BadgeAnchor, BadgeLabel, BadgePlacement, BadgeVariant, Button, Color, Icon, IconName,
    Orientation, Separator, Size, ToggleButton, ToggleVariant, Tooltip, TooltipPlacement, Variant,
};
use herogpui::gpui::{
    AnyElement, App, ClickEvent, Context, InteractiveElement, IntoElement, ParentElement, Styled,
    Window, div, px,
};
use herogpui::theme::ActiveTheme;

use crate::accessibility::KeyCommand;
use crate::app::{AppController, ScreenMode};
use crate::views::rail::RailConnectionItem;
use dataforge_state::connection_window::ConnectionWindowPhase;

use super::DataForgeWorkspace;
use super::engine_icons::engine_icon;
use super::theme::{
    HAIRLINE_BORDER_PX, MICRO_GAP_PX, hairline_border_color, is_reduced_motion, panel_surface_bg,
};

/// Renders the compact left vertical navigation rail.
#[must_use]
pub fn render_rail(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let skip_anim = is_reduced_motion(cx);
    let pending_edits = pending_edit_count(&ws.controller);
    let connected = matches!(
        ws.controller
            .connections
            .phase(ws.controller.active_workspace),
        Some(ConnectionWindowPhase::Connected)
    );

    let mut rail = div()
        .id("workspace-rail")
        .w(px(52.0))
        .h_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .items_center()
        .py(px(8.0))
        .px(px(6.0))
        .gap(px(MICRO_GAP_PX))
        .bg(panel_surface_bg(cx))
        .border_r(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .child(render_workspace_nav_button(
            &ws.controller,
            connected,
            pending_edits,
            skip_anim,
            cx,
        ))
        .child(render_rail_toggle(
            "rail-nav-connections",
            "Connections Library (Ctrl+Shift+C)",
            IconName::Server,
            ws.controller.screen_mode == ScreenMode::ConnectionsLibrary,
            skip_anim,
            cx,
            cx.listener(|ws, _, _, cx| {
                ws.switch_screen(ScreenMode::ConnectionsLibrary, cx);
            }),
        ))
        .child(
            Separator::new()
                .id("rail-sep-top")
                .orientation(Orientation::Horizontal)
                .sx(|s| s.my(px(4.0)).w(px(28.0))),
        );

    if !ws.controller.rail_items.is_empty() {
        rail = rail.child(render_rail_workspaces(&ws.controller, skip_anim, cx));
    }

    rail = rail
        .child(div().flex_1())
        .child(
            Separator::new()
                .id("rail-sep-bottom")
                .orientation(Orientation::Horizontal)
                .sx(|s| s.my(px(4.0)).w(px(28.0))),
        )
        .child(render_rail_bottom_controls(&ws.controller, skip_anim, cx));

    rail.into_any_element()
}

fn pending_edit_count(controller: &AppController) -> usize {
    let edits = &controller.grid_model.edits;
    edits.modified_cells.len() + edits.inserted_rows.len() + edits.deleted_rows.len()
}

fn render_workspace_nav_button(
    controller: &AppController,
    connected: bool,
    pending_edits: usize,
    skip_anim: bool,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let selected = controller.screen_mode == ScreenMode::Workspace;
    let fg = if selected {
        colors.accent.color
    } else {
        colors.muted
    };

    let btn = ToggleButton::new("rail-nav-workspace")
        .variant(ToggleVariant::Ghost)
        .size(Size::Md)
        .is_icon_only(true)
        .is_selected(selected)
        .aria_label("SQL Workspace")
        .on_press(cx.listener(|ws, _, _, cx| {
            ws.switch_screen(ScreenMode::Workspace, cx);
        }))
        .child(Icon::new(IconName::Terminal).size(px(16.0)).color(fg));

    let anchored = if pending_edits > 0 {
        BadgeAnchor::new()
            .child(btn)
            .child(
                Badge::new()
                    .color(Color::Warning)
                    .variant(BadgeVariant::Primary)
                    .size(Size::Sm)
                    .placement(BadgePlacement::TopRight)
                    .child(BadgeLabel::new().child(pending_edits.to_string())),
            )
            .into_any_element()
    } else if connected {
        BadgeAnchor::new()
            .child(btn)
            .child(
                Badge::new()
                    .color(Color::Success)
                    .variant(BadgeVariant::Primary)
                    .size(Size::Sm)
                    .placement(BadgePlacement::TopRight),
            )
            .into_any_element()
    } else {
        btn.into_any_element()
    };

    Tooltip::new("Workspace")
        .id("rail-nav-workspace-tip")
        .placement(TooltipPlacement::Right)
        .delay(300)
        .close_delay(80)
        .should_skip_animation(skip_anim)
        .child(anchored)
        .into_any_element()
}

fn render_rail_workspaces(
    controller: &AppController,
    skip_anim: bool,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let mut col = div()
        .id("rail-workspaces")
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(MICRO_GAP_PX));

    for (idx, item) in controller.rail_items.iter().enumerate() {
        col = col.child(render_rail_workspace_item(
            idx,
            item,
            controller.screen_mode == ScreenMode::Workspace,
            skip_anim,
            cx,
        ));
    }

    col.into_any_element()
}

fn render_rail_workspace_item(
    idx: usize,
    item: &RailConnectionItem,
    in_workspace: bool,
    skip_anim: bool,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let is_active = in_workspace && item.active;
    let status_text = if item.connected {
        "Connected"
    } else {
        "Click to connect"
    };
    let tooltip_text = format!("{} · {status_text}", item.label);
    let conn_id = item.id.clone();
    let is_connected = item.connected;
    let icon_color = if is_active {
        colors.accent.color
    } else {
        colors.foreground
    };

    let btn = ToggleButton::new(format!("rail-ws-{idx}"))
        .variant(ToggleVariant::Ghost)
        .size(Size::Md)
        .is_icon_only(true)
        .is_selected(is_active)
        .aria_label(item.label.clone())
        .on_press(cx.listener(move |ws, _, _, cx| {
            ws.controller.screen_mode = ScreenMode::Workspace;
            for (i, entry) in ws.controller.rail_items.iter_mut().enumerate() {
                entry.active = i == idx;
            }
            if !is_connected && let Err(err) = ws.controller.connect_library_item_blocking(&conn_id)
            {
                super::toasts::push_retry_connection_toast(
                    cx.entity().downgrade(),
                    "Connection failed",
                    err.to_string(),
                    cx,
                );
            }
            cx.notify();
        }))
        .child(engine_icon(item.engine, px(16.0), icon_color));

    let wrapped = if is_connected {
        BadgeAnchor::new()
            .child(btn)
            .child(
                Badge::new()
                    .color(Color::Success)
                    .variant(BadgeVariant::Primary)
                    .size(Size::Sm)
                    .placement(BadgePlacement::BottomRight),
            )
            .into_any_element()
    } else {
        btn.into_any_element()
    };

    Tooltip::new(tooltip_text)
        .id(format!("rail-ws-tip-{idx}"))
        .placement(TooltipPlacement::Right)
        .delay(300)
        .close_delay(80)
        .should_skip_animation(skip_anim)
        .child(wrapped)
        .into_any_element()
}

fn render_rail_bottom_controls(
    controller: &AppController,
    skip_anim: bool,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let dark = controller.uses_dark_titlebar();
    let sidebar_open =
        controller.screen_mode == ScreenMode::Workspace && controller.layout_config.sidebar_visible;

    div()
        .id("rail-bottom-controls")
        .flex()
        .flex_col()
        .items_center()
        .gap(px(MICRO_GAP_PX))
        .child(render_rail_toggle(
            "rail-toggle-sidebar",
            "Toggle Sidebar (Ctrl+B)",
            IconName::PanelLeft,
            sidebar_open,
            skip_anim,
            cx,
            cx.listener(|ws, _, window, cx| {
                ws.controller.screen_mode = ScreenMode::Workspace;
                ws.execute_key_command(KeyCommand::ToggleSidebar, window, cx);
            }),
        ))
        .child(render_rail_toggle(
            "rail-toggle-inspector",
            "Toggle Row Inspector (Ctrl+I)",
            IconName::Columns2,
            controller.layout_config.inspector_visible,
            skip_anim,
            cx,
            cx.listener(|ws, _, window, cx| {
                ws.execute_key_command(KeyCommand::ToggleInspector, window, cx);
            }),
        ))
        .child(render_rail_toggle(
            "rail-toggle-ai",
            "Toggle AI Assistant (Ctrl+Shift+A)",
            IconName::Sparkles,
            controller.layout_config.ai_drawer_visible,
            skip_anim,
            cx,
            cx.listener(|ws, _, window, cx| {
                ws.execute_key_command(KeyCommand::ToggleAiDrawer, window, cx);
            }),
        ))
        .child(render_rail_theme_toggle(dark, skip_anim, cx))
        .child(render_rail_toggle(
            "rail-nav-settings",
            "Settings (Ctrl+,)",
            IconName::Settings,
            controller.screen_mode == ScreenMode::Settings,
            skip_anim,
            cx,
            cx.listener(|ws, _, _, cx| {
                let target = if ws.controller.screen_mode == ScreenMode::Settings {
                    ScreenMode::Workspace
                } else {
                    ScreenMode::Settings
                };
                ws.switch_screen(target, cx);
            }),
        ))
        .into_any_element()
}

fn render_rail_theme_toggle(
    dark: bool,
    skip_anim: bool,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    Tooltip::new(if dark {
        "Switch to Light Theme (Ctrl+Shift+M)"
    } else {
        "Switch to Dark Theme (Ctrl+Shift+M)"
    })
    .id("rail-toggle-theme-tip")
    .placement(TooltipPlacement::Right)
    .delay(300)
    .close_delay(80)
    .should_skip_animation(skip_anim)
    .child(
        Button::new("rail-toggle-theme")
            .variant(Variant::Ghost)
            .size(Size::Md)
            .is_icon_only(true)
            .aria_label("Toggle Theme")
            .on_press(cx.listener(|ws, _, window, cx| {
                ws.toggle_theme_mode(window, cx);
            }))
            .child(
                Icon::new(if dark { IconName::Sun } else { IconName::Moon })
                    .size(px(16.0))
                    .color(colors.muted),
            ),
    )
    .into_any_element()
}

fn render_rail_toggle(
    id: &'static str,
    tooltip: &'static str,
    icon: IconName,
    selected: bool,
    skip_anim: bool,
    cx: &Context<DataForgeWorkspace>,
    on_press: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let colors = cx.colors().clone();
    let fg = if selected {
        colors.accent.color
    } else {
        colors.muted
    };

    Tooltip::new(tooltip)
        .id(format!("{id}-tip"))
        .placement(TooltipPlacement::Right)
        .delay(300)
        .close_delay(80)
        .should_skip_animation(skip_anim)
        .child(
            ToggleButton::new(id)
                .variant(ToggleVariant::Ghost)
                .size(Size::Md)
                .is_icon_only(true)
                .is_selected(selected)
                .aria_label(tooltip)
                .on_press(on_press)
                .child(Icon::new(icon).size(px(16.0)).color(fg)),
        )
        .into_any_element()
}
