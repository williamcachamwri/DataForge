//! Custom HeroGPUI client-side title bar with macOS-style traffic lights and Win32 drag hit-testing.

use herogpui::components::{
    Button, Icon, IconName, Kbd, KbdAbbr, KbdContent, KbdKey, KbdVariant, Orientation, Popover,
    PopoverPlacement, Separator, Size, Tooltip, TooltipPlacement, Variant,
};
use herogpui::core::with_alpha;
use herogpui::gpui::{
    AnyElement, Context, FontWeight, ImageSource, InteractiveElement, IntoElement, MouseButton,
    ParentElement, StatefulInteractiveElement, Styled, Window, WindowControlArea, div, img, px,
    rgb, rgba, svg,
};
use herogpui::theme::ActiveTheme;

use crate::accessibility::KeyCommand;
use crate::app::ScreenMode;
use tableminus_state::connection_window::ConnectionWindowPhase;
use tableminus_windows_platform::WindowControlsStyle;

use super::TableMinusWorkspace;
use super::shell_state::{SettingsSection, WorkspaceToolView};
use super::theme::{
    CONTROL_RADIUS_PX, HAIRLINE_BORDER_PX, TEXT_BADGE_PX, TEXT_BODY_PX, TEXT_CONTROL_PX,
    TEXT_META_PX, animate_popover_enter, elevated_overlay_bg, hairline_border_color,
    is_reduced_motion,
};

/// Renders the custom `TableMinus` title bar wired with macOS-style traffic lights
/// or native Windows 11 caption buttons (per `settings.window_controls`),
/// branding, live connection status, quick command search trigger, and an isolated
/// empty drag area so clicks on controls are never swallowed by `WM_NCHITTEST`.
#[must_use]
pub fn render_titlebar(
    ws: &mut TableMinusWorkspace,
    window: &mut Window,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let hairline = hairline_border_color(cx);
    let skip_anim = is_reduced_motion(cx);
    let window_controls = ws.controller.settings.window_controls;
    let is_windows_controls = window_controls == WindowControlsStyle::Windows;
    let is_maximized = window.is_maximized();

    let mut leading = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .min_w(px(0.0))
        .flex_shrink(1.0)
        .overflow_hidden();

    if !is_windows_controls {
        leading = leading.child(render_traffic_lights(skip_anim, cx));
    }

    leading = leading
        .child(render_hamburger_menu(ws, skip_anim, cx))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .min_w(px(0.0))
                .flex_shrink(1.0)
                .pl(px(if is_windows_controls { 0.0 } else { 2.0 }))
                .child(
                    img(ImageSource::Image(crate::branding::tableminus_logo_image()))
                        .w(px(18.0))
                        .h(px(18.0))
                        .flex_shrink_0(),
                )
                .child(
                    div()
                        .min_w(px(0.0))
                        .flex_shrink(1.0)
                        .truncate()
                        .text_size(px(TEXT_BODY_PX))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.foreground)
                        .child(ws.controller.strings.app_title),
                ),
        );

    let show_welcome = ws.controller.screen_mode == ScreenMode::Workspace && ws.welcome_ui.active;
    if !show_welcome {
        leading = leading
            .child(
                Separator::new()
                    .id("titlebar-sep-left")
                    .orientation(Orientation::Vertical)
                    .sx(|s| s.h(px(16.0)).flex_shrink_0()),
            )
            .child(render_quick_search_trigger(
                &ws.controller.strings,
                skip_anim,
                cx,
            ));
    }

    let mut bar = div()
        .id("tableminus-titlebar")
        .flex()
        .flex_row()
        .items_center()
        .w_full()
        .h(px(36.0))
        .min_w(px(0.0))
        .flex_shrink_0()
        .overflow_hidden()
        .pl(px(12.0))
        .pr(px(if is_windows_controls { 0.0 } else { 12.0 }))
        .bg(colors.surface.background)
        .border_b(px(HAIRLINE_BORDER_PX))
        .border_color(hairline)
        .child(leading)
        .child(
            div()
                .id("titlebar-drag-filler")
                .flex_1()
                .min_w(px(24.0))
                .h_full()
                .window_control_area(WindowControlArea::Drag)
                .on_mouse_down(MouseButton::Left, |event, window, _cx| {
                    if event.click_count >= 2 {
                        window.zoom_window();
                    } else if !cfg!(target_os = "windows") {
                        window.start_window_move();
                    }
                }),
        );

    if is_windows_controls {
        bar = bar.child(render_windows_caption_buttons(is_maximized, cx));
    }

    bar.into_any_element()
}

fn request_close_window(
    ws: &mut TableMinusWorkspace,
    window: &mut Window,
    cx: &mut Context<TableMinusWorkspace>,
) {
    if ws.controller.needs_close_guard() {
        ws.workspace_pane.close_guard_open = true;
        cx.notify();
    } else if ws.controller.screen_mode == ScreenMode::Workspace && !ws.welcome_ui.active {
        super::welcome_view::close_active_connection_to_welcome(ws, cx);
    } else {
        window.remove_window();
    }
}

struct TrafficLightStyle {
    id: &'static str,
    base: u32,
    hover: u32,
    active: u32,
    icon: IconName,
    glyph_color: u32,
}

fn traffic_light_dot(
    style: TrafficLightStyle,
    skip_anim: bool,
    on_click: impl Fn(&herogpui::gpui::ClickEvent, &mut Window, &mut herogpui::gpui::App) + 'static,
) -> AnyElement {
    let glyph_opacity = if skip_anim { 1.0 } else { 0.95 };
    div()
        .id(style.id)
        .w(px(12.0))
        .h(px(12.0))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .bg(rgb(style.base))
        .border(px(0.5))
        .border_color(rgba(0x0000_0026))
        .cursor_pointer()
        .hover(move |s| s.bg(rgb(style.hover)))
        .active(move |s| s.bg(rgb(style.active)))
        .on_click(on_click)
        .child(
            div()
                .size(px(8.0))
                .flex()
                .items_center()
                .justify_center()
                .opacity(0.0)
                .group_hover("traffic-lights", move |s| s.opacity(glyph_opacity))
                .child(
                    Icon::new(style.icon)
                        .size(px(8.0))
                        .stroke_width(3.0)
                        .color(rgba(style.glyph_color)),
                ),
        )
        .into_any_element()
}

fn render_traffic_lights(skip_anim: bool, cx: &Context<TableMinusWorkspace>) -> AnyElement {
    let close_btn = traffic_light_dot(
        TrafficLightStyle {
            id: "traffic-light-close",
            base: 0x00FF_5F57,
            hover: 0x00E0_443E,
            active: 0x00C9_3832,
            icon: IconName::X,
            glyph_color: 0x4D00_00D9,
        },
        skip_anim,
        cx.listener(|ws, _, window, cx| request_close_window(ws, window, cx)),
    );

    let minimize_btn = traffic_light_dot(
        TrafficLightStyle {
            id: "traffic-light-minimize",
            base: 0x00FE_BC2E,
            hover: 0x00D8_9E24,
            active: 0x00BF_8B1D,
            icon: IconName::Minus,
            glyph_color: 0x5C3A_00D9,
        },
        skip_anim,
        |_, window, _| window.minimize_window(),
    );

    let zoom_btn = traffic_light_dot(
        TrafficLightStyle {
            id: "traffic-light-zoom",
            base: 0x0028_C840,
            hover: 0x001A_AB29,
            active: 0x0014_9421,
            icon: IconName::Plus,
            glyph_color: 0x004D_0AD9,
        },
        skip_anim,
        |_, window, _| window.zoom_window(),
    );

    div()
        .id("titlebar-traffic-lights")
        .group("traffic-lights")
        .flex()
        .items_center()
        .gap(px(8.0))
        .flex_shrink_0()
        .child(close_btn)
        .child(minimize_btn)
        .child(zoom_btn)
        .into_any_element()
}

fn render_windows_caption_buttons(
    is_maximized: bool,
    cx: &Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let hover_bg = with_alpha(colors.foreground, 0.08);
    let active_bg = with_alpha(colors.foreground, 0.14);
    let close_hover_bg = rgb(0x00C4_2B1C);
    let close_active_bg = rgb(0x00B2_2719);
    let close_hover_fg = rgb(0x00FF_FFFF);

    let caption_btn = |id: &'static str, icon: IconName, icon_size: f32| {
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .w(px(46.0))
            .h_full()
            .flex_shrink_0()
            .hover(move |s| s.bg(hover_bg))
            .active(move |s| s.bg(active_bg))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                Icon::new(icon)
                    .size(px(icon_size))
                    .stroke_width(1.1)
                    .absolute_stroke_width(true)
                    .color(colors.foreground),
            )
    };

    let minimize_btn = caption_btn("win-caption-minimize", IconName::Minus, 16.0)
        .on_click(|_, window, _| window.minimize_window());

    let (zoom_icon, zoom_size) = if is_maximized {
        (IconName::Copy, 12.0)
    } else {
        (IconName::Square, 12.0)
    };
    let zoom_btn =
        caption_btn("win-caption-zoom", zoom_icon, zoom_size).on_click(|_, window, _| {
            window.zoom_window();
        });

    let close_btn = div()
        .id("win-caption-close")
        .group("win-caption-close")
        .flex()
        .items_center()
        .justify_center()
        .w(px(46.0))
        .h_full()
        .flex_shrink_0()
        .hover(move |s| s.bg(close_hover_bg))
        .active(move |s| s.bg(close_active_bg))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|ws, _, window, cx| request_close_window(ws, window, cx)))
        .child(
            svg()
                .path(IconName::X.path_with_stroke_width(1.1 * 24.0 / 18.0))
                .size(px(18.0))
                .flex_none()
                .text_color(colors.foreground)
                .group_hover("win-caption-close", move |s| s.text_color(close_hover_fg)),
        );

    div()
        .id("titlebar-win-controls")
        .flex()
        .flex_row()
        .items_center()
        .h_full()
        .flex_shrink_0()
        .child(minimize_btn)
        .child(zoom_btn)
        .child(close_btn)
        .into_any_element()
}

fn render_quick_search_trigger(
    strings: &crate::i18n::Strings,
    skip_anim: bool,
    cx: &Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    Tooltip::new(strings.cmd_palette_tooltip)
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
                .aria_label(strings.open_cmd_palette_aria)
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
                                .whitespace_nowrap()
                                .text_size(px(TEXT_CONTROL_PX))
                                .text_color(colors.muted)
                                .child(strings.command_placeholder),
                        )
                        .child(
                            Kbd::new()
                                .variant(KbdVariant::Light)
                                .sx(|s| s.h(px(18.0)).px(px(4.0)).text_size(px(TEXT_BADGE_PX)))
                                .child(KbdAbbr::new(KbdKey::Ctrl))
                                .child(KbdContent::new().child("K")),
                        ),
                ),
        )
        .into_any_element()
}

fn render_hamburger_menu(
    ws: &mut TableMinusWorkspace,
    skip_anim: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &ws.controller.strings;
    let is_connected = matches!(
        ws.controller
            .connections
            .phase(ws.controller.active_workspace),
        Some(ConnectionWindowPhase::Connected)
    );
    let show_welcome = ws.controller.screen_mode == ScreenMode::Workspace && ws.welcome_ui.active;
    let has_export_rows = !ws.controller.grid_rows.is_empty()
        || ws
            .active_tab_result()
            .is_some_and(|r| !r.accumulator.result_sets.is_empty());

    let trigger = Tooltip::new("Menu")
        .id("titlebar-hamburger-tip")
        .placement(TooltipPlacement::Bottom)
        .delay(350)
        .close_delay(80)
        .should_skip_animation(skip_anim)
        .child(
            Button::new("titlebar-hamburger-btn")
                .variant(if ws.titlebar_menu_open {
                    Variant::Secondary
                } else {
                    Variant::Ghost
                })
                .size(Size::Sm)
                .is_icon_only(true)
                .height(px(26.0))
                .width(px(26.0))
                .radius(px(CONTROL_RADIUS_PX))
                .aria_label("Application Menu")
                .child(
                    Icon::new(IconName::Menu)
                        .size(px(15.0))
                        .color(colors.foreground),
                ),
        );

    let menu_body = div()
        .id("titlebar-hamburger-menu-body")
        .w_full()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .bg(elevated_overlay_bg(cx))
        .child(render_menu_query_section(s, has_export_rows, cx))
        .child(
            Separator::new()
                .id("menu-sep-view")
                .orientation(Orientation::Horizontal)
                .sx(|s| s.my(px(4.0))),
        )
        .child(render_menu_view_section(s, show_welcome, cx))
        .child(
            Separator::new()
                .id("menu-sep-app")
                .orientation(Orientation::Horizontal)
                .sx(|s| s.my(px(4.0))),
        )
        .child(render_menu_app_section(s, is_connected, cx))
        .child(
            Separator::new()
                .id("menu-sep-quit")
                .orientation(Orientation::Horizontal)
                .sx(|s| s.my(px(4.0))),
        )
        .child(render_menu_item(
            "menu-quit",
            IconName::Power,
            "Quit",
            Some("Alt+F4"),
            false,
            cx,
            cx.listener(|ws, _, window, cx| {
                ws.titlebar_menu_open = false;
                request_close_window(ws, window, cx);
            }),
        ));

    Popover::new(trigger)
        .id("titlebar-hamburger-popover")
        .placement(PopoverPlacement::BottomStart)
        .panel_width(px(272.0))
        .padding(px(8.0))
        .is_open(ws.titlebar_menu_open)
        .on_open_change(cx.listener(|ws, open: &bool, _w, cx| {
            ws.titlebar_menu_open = *open;
            cx.notify();
        }))
        .child(animate_popover_enter(
            menu_body,
            "titlebar-hamburger-anim",
            cx,
        ))
        .into_any_element()
}

fn render_menu_query_section(
    s: &crate::i18n::Strings,
    has_export_rows: bool,
    cx: &Context<TableMinusWorkspace>,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(render_menu_section_label("Query & File", cx))
        .child(render_menu_item(
            "menu-new-query",
            IconName::FilePlus,
            s.new_query_tab_tooltip,
            Some("Ctrl+T"),
            false,
            cx,
            cx.listener(|ws, _, window, cx| {
                ws.titlebar_menu_open = false;
                ws.welcome_ui.close();
                ws.execute_key_command(KeyCommand::NewQueryTab, window, cx);
            }),
        ))
        .child(render_menu_item(
            "menu-save-query",
            IconName::Save,
            s.save_query,
            Some("Ctrl+S"),
            false,
            cx,
            cx.listener(|ws, _, _, cx| {
                ws.titlebar_menu_open = false;
                super::toolbar::save_active_tab_query(ws, cx);
            }),
        ))
        .child(render_menu_item(
            "menu-export",
            IconName::Download,
            s.export,
            Some("Ctrl+Shift+E"),
            !has_export_rows,
            cx,
            cx.listener(|ws, _, window, cx| {
                ws.titlebar_menu_open = false;
                ws.modals_ui.open_export(window, cx);
                cx.notify();
            }),
        ))
        .child(render_menu_item(
            "menu-import",
            IconName::Upload,
            s.import_wizard,
            None,
            false,
            cx,
            cx.listener(|ws, _, _, cx| {
                ws.titlebar_menu_open = false;
                ws.welcome_ui.close();
                ws.workspace_pane.open_tool(WorkspaceToolView::ImportWizard);
                ws.switch_screen(ScreenMode::Workspace, cx);
            }),
        ))
        .into_any_element()
}

fn render_menu_view_section(
    s: &crate::i18n::Strings,
    show_welcome: bool,
    cx: &Context<TableMinusWorkspace>,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(render_menu_section_label("View", cx))
        .child(render_menu_item(
            "menu-toggle-rail",
            IconName::PanelLeft,
            "Toggle Left Rail",
            None,
            false,
            cx,
            cx.listener(|ws, _, _, cx| {
                ws.titlebar_menu_open = false;
                ws.toggle_rail(cx);
            }),
        ))
        .child(render_menu_item(
            "menu-toggle-sidebar",
            IconName::PanelLeft,
            "Toggle Objects Sidebar",
            Some("Ctrl+B"),
            false,
            cx,
            cx.listener(|ws, _, window, cx| {
                ws.titlebar_menu_open = false;
                ws.execute_key_command(KeyCommand::ToggleSidebar, window, cx);
            }),
        ))
        .child(render_menu_item(
            "menu-toggle-inspector",
            IconName::Columns2,
            s.cmd_toggle_inspector,
            Some("Ctrl+I"),
            false,
            cx,
            cx.listener(|ws, _, window, cx| {
                ws.titlebar_menu_open = false;
                ws.execute_key_command(KeyCommand::ToggleInspector, window, cx);
            }),
        ))
        .child(render_menu_item(
            "menu-toggle-ai",
            IconName::Sparkles,
            s.cmd_toggle_ai,
            Some("Ctrl+Shift+A"),
            false,
            cx,
            cx.listener(|ws, _, window, cx| {
                ws.titlebar_menu_open = false;
                ws.execute_key_command(KeyCommand::ToggleAiDrawer, window, cx);
            }),
        ))
        .child(render_menu_item(
            "menu-toggle-console",
            IconName::Terminal,
            "Toggle SQL Console",
            None,
            show_welcome,
            cx,
            cx.listener(|ws, _, _, cx| {
                ws.titlebar_menu_open = false;
                ws.console_log.toggle();
                cx.notify();
            }),
        ))
        .child(render_menu_item(
            "menu-cmd-palette",
            IconName::Search,
            s.cmd_palette_tooltip,
            Some("Ctrl+K"),
            show_welcome,
            cx,
            cx.listener(|ws, _, window, cx| {
                ws.titlebar_menu_open = false;
                if !(ws.controller.screen_mode == ScreenMode::Workspace && ws.welcome_ui.active) {
                    ws.command_palette_ui.toggle(window, cx);
                }
                cx.notify();
            }),
        ))
        .into_any_element()
}

fn render_menu_app_section(
    s: &crate::i18n::Strings,
    is_connected: bool,
    cx: &Context<TableMinusWorkspace>,
) -> AnyElement {
    let mut section = div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(render_menu_section_label(s.app_title, cx))
        .child(render_menu_item(
            "menu-connections",
            IconName::Server,
            s.connections_library,
            Some("Ctrl+Shift+O"),
            false,
            cx,
            cx.listener(|ws, _, _, cx| {
                ws.titlebar_menu_open = false;
                ws.switch_screen(ScreenMode::ConnectionsLibrary, cx);
            }),
        ));

    if is_connected {
        section = section.child(render_menu_item(
            "menu-disconnect-welcome",
            IconName::LogOut,
            "Close Connection",
            None,
            false,
            cx,
            cx.listener(|ws, _, _, cx| {
                ws.titlebar_menu_open = false;
                super::welcome_view::close_active_connection_to_welcome(ws, cx);
            }),
        ));
    }

    section
        .child(render_menu_item(
            "menu-settings",
            IconName::Settings,
            s.settings,
            Some("Ctrl+,"),
            false,
            cx,
            cx.listener(|ws, _, _, cx| {
                ws.titlebar_menu_open = false;
                ws.switch_screen(ScreenMode::Settings, cx);
            }),
        ))
        .child(render_menu_item(
            "menu-shortcuts",
            IconName::Keyboard,
            "Keymap & Shortcuts",
            None,
            false,
            cx,
            cx.listener(|ws, _, _, cx| {
                ws.titlebar_menu_open = false;
                ws.settings_view.section = SettingsSection::Shortcuts;
                ws.switch_screen(ScreenMode::Settings, cx);
            }),
        ))
        .child(render_menu_item(
            "menu-toggle-theme",
            IconName::SunMoon,
            s.cmd_toggle_theme,
            Some("Ctrl+Shift+M"),
            false,
            cx,
            cx.listener(|ws, _, window, cx| {
                ws.titlebar_menu_open = false;
                ws.toggle_theme_mode(window, cx);
            }),
        ))
        .child(render_menu_item(
            "menu-about",
            IconName::Info,
            s.cat_about,
            None,
            false,
            cx,
            cx.listener(|ws, _, _, cx| {
                ws.titlebar_menu_open = false;
                ws.settings_view.section = SettingsSection::Appearance;
                ws.switch_screen(ScreenMode::Settings, cx);
            }),
        ))
        .into_any_element()
}

fn render_menu_section_label(label: &'static str, cx: &Context<TableMinusWorkspace>) -> AnyElement {
    div()
        .px(px(8.0))
        .py(px(2.0))
        .text_size(px(TEXT_META_PX))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.colors().muted)
        .child(label)
        .into_any_element()
}

fn render_menu_item(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    shortcut: Option<&'static str>,
    disabled: bool,
    cx: &Context<TableMinusWorkspace>,
    on_press: impl Fn(&herogpui::gpui::ClickEvent, &mut Window, &mut herogpui::gpui::App) + 'static,
) -> AnyElement {
    let colors = cx.colors().clone();
    let fg = if disabled {
        colors.muted
    } else {
        colors.foreground
    };

    let mut row = div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .min_w(px(0.0))
                .child(Icon::new(icon).size(px(14.0)).color(fg))
                .child(
                    div()
                        .truncate()
                        .text_size(px(TEXT_BODY_PX))
                        .text_color(fg)
                        .child(label),
                ),
        );

    if let Some(keys) = shortcut {
        row = row.child(
            div()
                .flex_shrink_0()
                .text_size(px(TEXT_BADGE_PX))
                .text_color(colors.muted)
                .child(keys),
        );
    }

    Button::new(id)
        .variant(Variant::Ghost)
        .size(Size::Sm)
        .height(px(28.0))
        .full_width(true)
        .is_disabled(disabled)
        .on_press(on_press)
        .child(row)
        .into_any_element()
}
