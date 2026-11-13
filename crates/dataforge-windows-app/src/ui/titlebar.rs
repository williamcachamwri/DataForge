//! Custom HeroGPUI client-side title bar with Win32 Snap Layout hit-testing.

use herogpui::components::{
    Button, Chip, ChipLabel, ChipVariant, Color, Icon, IconName, Kbd, KbdAbbr, KbdContent, KbdKey,
    KbdVariant, Orientation, Separator, Size, Spinner, TitleBar, TitleBarControls, ToggleButton,
    ToggleVariant, Tooltip, TooltipPlacement, Variant, WindowAction,
};
use herogpui::core::with_alpha;
use herogpui::gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, Styled,
    Window, WindowControlArea, div, px,
};
use herogpui::theme::ActiveTheme;

use crate::accessibility::KeyCommand;
use crate::app::{AppController, ScreenMode};
use dataforge_gfx::glyphs::EngineKind;
use dataforge_state::connection_window::ConnectionWindowPhase;
use tablepro_core::SafeModeLevel;

use super::DataForgeWorkspace;
use super::engine_icons::engine_icon;
use super::theme::{
    CONTROL_RADIUS_PX, HAIRLINE_BORDER_PX, MICRO_GAP_PX, hairline_border_color, is_reduced_motion,
};

/// Renders the custom `DataForge` title bar wired with branding, live connection status,
/// quick command search trigger, and chrome panel toggles.
#[must_use]
pub fn render_titlebar(
    ws: &mut DataForgeWorkspace,
    window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let entity = cx.entity().downgrade();
    let skip_anim = is_reduced_motion(cx);
    let dark = ws.controller.uses_dark_titlebar();

    let mut bar = TitleBar::new("dataforge-titlebar")
        .title(ws.controller.strings.app_title)
        .height(px(36.0))
        .controls(TitleBarControls::Auto)
        .on_window_action(move |action, win, app| {
            if matches!(action, WindowAction::Close)
                && let Some(view) = entity.upgrade()
            {
                let needs_guard = view.read(app).controller.needs_close_guard();
                if needs_guard {
                    view.update(app, |ws, cx| {
                        ws.workspace_pane.close_guard_open = true;
                        cx.notify();
                    });
                    return;
                }
            }
            action.perform(win);
        })
        .sx({
            let hairline = hairline_border_color(cx);
            move |s| s.border_color(hairline)
        })
        .child(render_status_pill(&ws.controller, cx))
        .child(
            Separator::new()
                .id("titlebar-sep-left")
                .orientation(Orientation::Vertical)
                .sx(|s| s.h(px(16.0))),
        )
        .child(render_quick_search_trigger(skip_anim, cx))
        .child(render_quick_toggles(&ws.controller, dark, skip_anim, cx));

    // Keep an explicit draggable capture zone inside content flow when width expands.
    let _ = window;
    bar = bar.child(
        div()
            .id("titlebar-drag-filler")
            .min_w(px(12.0))
            .h_full()
            .window_control_area(WindowControlArea::Drag),
    );
    bar.into_any_element()
}

fn active_engine_kind(controller: &AppController) -> Option<EngineKind> {
    let phase = controller.connections.phase(controller.active_workspace);
    if !matches!(phase, Some(ConnectionWindowPhase::Connected)) {
        return None;
    }
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

fn render_status_pill(controller: &AppController, cx: &Context<DataForgeWorkspace>) -> AnyElement {
    let colors = cx.colors().clone();
    match controller.screen_mode {
        ScreenMode::ConnectionsLibrary => {
            return render_screen_pill(IconName::Server, "Connections", cx);
        }
        ScreenMode::Settings => return render_screen_pill(IconName::Settings, "Settings", cx),
        ScreenMode::Workspace => {}
    }

    match controller.connections.phase(controller.active_workspace) {
        Some(ConnectionWindowPhase::Connected) => render_connected_pill(controller, cx),
        Some(ConnectionWindowPhase::Connecting { .. }) => {
            render_spinner_chip("titlebar-conn-spinner", "Connecting...")
        }
        Some(ConnectionWindowPhase::Reconnecting { .. }) => {
            render_spinner_chip("titlebar-reconn-spinner", "Reconnecting...")
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
                div()
                    .w(px(6.0))
                    .h(px(6.0))
                    .rounded(px(99.0))
                    .flex_shrink_0()
                    .bg(colors.muted),
            )
            .child(ChipLabel::new().child("Disconnected"))
            .into_any_element(),
    }
}

fn render_screen_pill(
    icon: IconName,
    label: &'static str,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    Chip::new()
        .variant(ChipVariant::Secondary)
        .color(Color::Default)
        .size(Size::Sm)
        .child(Icon::new(icon).size(px(12.0)).color(colors.muted))
        .child(ChipLabel::new().child(label))
        .into_any_element()
}

fn render_spinner_chip(id: &'static str, label: &'static str) -> AnyElement {
    Chip::new()
        .variant(ChipVariant::Soft)
        .color(Color::Accent)
        .size(Size::Sm)
        .child(Spinner::new(id).size_px(px(12.0)))
        .child(ChipLabel::new().child(label))
        .into_any_element()
}

fn render_connected_pill(
    controller: &AppController,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let engine = active_engine_kind(controller);
    let target_label = format_connected_label(controller);
    let safe_mode = controller.active_safe_mode();
    let mut pill = div()
        .id("titlebar-conn-pill")
        .flex()
        .items_center()
        .gap(px(6.0))
        .max_w(px(280.0))
        .min_w(px(0.0))
        .px(px(8.0))
        .py(px(3.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(with_alpha(colors.surface_secondary, 0.85))
        .border(px(HAIRLINE_BORDER_PX))
        .border_color(colors.border)
        .child(
            div()
                .w(px(6.0))
                .h(px(6.0))
                .rounded(px(99.0))
                .flex_shrink_0()
                .bg(colors.success.color),
        );
    if let Some(kind) = engine {
        pill = pill.child(engine_icon(kind, px(13.0), colors.foreground));
    }
    pill = pill.child(
        div()
            .flex_1()
            .min_w(px(0.0))
            .truncate()
            .text_size(px(11.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(colors.foreground)
            .child(target_label),
    );
    if safe_mode != SafeModeLevel::Off {
        pill = pill.child(
            Icon::new(IconName::ShieldCheck)
                .size(px(12.0))
                .color(colors.warning.color),
        );
    }
    pill.into_any_element()
}

fn format_connected_label(controller: &AppController) -> String {
    let status = &controller.toolbar_status;
    let host = status.host.trim();
    let db = status.database.trim();
    match (!host.is_empty() && host != "—", !db.is_empty() && db != "—") {
        (true, true) => format!("{host} / {db}"),
        (true, false) => host.to_owned(),
        (false, true) => db.to_owned(),
        (false, false) => {
            if !status.engine_version.is_empty() && status.engine_version != "Not connected" {
                status.engine_version.clone()
            } else {
                "Connected".to_owned()
            }
        }
    }
}

fn render_quick_search_trigger(skip_anim: bool, cx: &Context<DataForgeWorkspace>) -> AnyElement {
    let colors = cx.colors().clone();
    Tooltip::new("Command Palette (Ctrl+K)")
        .id("titlebar-cmd-tip")
        .placement(TooltipPlacement::Bottom)
        .delay(350)
        .close_delay(80)
        .should_skip_animation(skip_anim)
        .child(
            Button::new("titlebar-cmd-btn")
                .variant(Variant::Outline)
                .size(Size::Sm)
                .height(px(24.0))
                .padding_x(px(8.0))
                .radius(px(CONTROL_RADIUS_PX))
                .aria_label("Open Command Palette")
                .on_press(cx.listener(|ws, _, window, cx| {
                    ws.command_palette_ui.toggle(window, cx);
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            Icon::new(IconName::Search)
                                .size(px(12.0))
                                .color(colors.muted),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(colors.muted)
                                .child("Command..."),
                        )
                        .child(
                            Kbd::new()
                                .variant(KbdVariant::Light)
                                .sx(|s| s.h(px(18.0)).px(px(4.0)).text_size(px(10.0)))
                                .child(KbdAbbr::new(KbdKey::Ctrl))
                                .child(KbdContent::new().child("K")),
                        ),
                ),
        )
        .into_any_element()
}

fn render_quick_toggles(
    controller: &AppController,
    dark: bool,
    skip_anim: bool,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let sidebar_active =
        controller.screen_mode == ScreenMode::Workspace && controller.layout_config.sidebar_visible;
    let inspector_active = controller.layout_config.inspector_visible;
    let ai_active = controller.layout_config.ai_drawer_visible;
    let settings_active = controller.screen_mode == ScreenMode::Settings;

    div()
        .id("titlebar-quick-toggles")
        .flex()
        .items_center()
        .gap(px(MICRO_GAP_PX))
        .child(render_toggle_icon(
            "titlebar-toggle-sidebar",
            "Toggle Sidebar (Ctrl+B)",
            IconName::PanelLeft,
            sidebar_active,
            skip_anim,
            cx,
            cx.listener(|ws, _, window, cx| {
                ws.controller.screen_mode = ScreenMode::Workspace;
                ws.execute_key_command(KeyCommand::ToggleSidebar, window, cx);
            }),
        ))
        .child(render_toggle_icon(
            "titlebar-toggle-inspector",
            "Toggle Inspector (Ctrl+I)",
            IconName::Columns2,
            inspector_active,
            skip_anim,
            cx,
            cx.listener(|ws, _, window, cx| {
                ws.execute_key_command(KeyCommand::ToggleInspector, window, cx);
            }),
        ))
        .child(render_toggle_icon(
            "titlebar-toggle-ai",
            "Toggle AI Assistant (Ctrl+Shift+A)",
            IconName::Sparkles,
            ai_active,
            skip_anim,
            cx,
            cx.listener(|ws, _, window, cx| {
                ws.execute_key_command(KeyCommand::ToggleAiDrawer, window, cx);
            }),
        ))
        .child(render_theme_toggle(dark, skip_anim, cx))
        .child(render_toggle_icon(
            "titlebar-toggle-settings",
            "Settings (Ctrl+,)",
            IconName::Settings,
            settings_active,
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

fn render_theme_toggle(
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
    .id("titlebar-theme-tip")
    .placement(TooltipPlacement::Bottom)
    .delay(350)
    .close_delay(80)
    .should_skip_animation(skip_anim)
    .child(
        Button::new("titlebar-toggle-theme")
            .variant(Variant::Ghost)
            .size(Size::Sm)
            .is_icon_only(true)
            .aria_label("Toggle Theme")
            .on_press(cx.listener(|ws, _, window, cx| {
                ws.toggle_theme_mode(window, cx);
            }))
            .child(
                Icon::new(if dark { IconName::Sun } else { IconName::Moon })
                    .size(px(14.0))
                    .color(colors.muted),
            ),
    )
    .into_any_element()
}

fn render_toggle_icon(
    id: &'static str,
    tooltip: &'static str,
    icon: IconName,
    selected: bool,
    skip_anim: bool,
    cx: &Context<DataForgeWorkspace>,
    on_press: impl Fn(&herogpui::gpui::ClickEvent, &mut Window, &mut herogpui::gpui::App) + 'static,
) -> AnyElement {
    let colors = cx.colors().clone();
    let fg = if selected {
        colors.accent.color
    } else {
        colors.muted
    };
    Tooltip::new(tooltip)
        .id(format!("{id}-tip"))
        .placement(TooltipPlacement::Bottom)
        .delay(350)
        .close_delay(80)
        .should_skip_animation(skip_anim)
        .child(
            ToggleButton::new(id)
                .variant(ToggleVariant::Ghost)
                .size(Size::Sm)
                .is_icon_only(true)
                .is_selected(selected)
                .aria_label(tooltip)
                .on_press(on_press)
                .child(Icon::new(icon).size(px(14.0)).color(fg)),
        )
        .into_any_element()
}
