//! Custom HeroGPUI client-side title bar with macOS-style traffic lights and Win32 drag hit-testing.

use herogpui::components::{
    Button, Chip, ChipLabel, ChipVariant, Color, Icon, IconName, Kbd, KbdAbbr, KbdContent, KbdKey,
    KbdVariant, Orientation, Separator, Size, Spinner, Tooltip, TooltipPlacement, Variant,
};
use herogpui::core::with_alpha;
use herogpui::gpui::{
    AnyElement, Context, FontWeight, ImageSource, InteractiveElement, IntoElement, MouseButton,
    ParentElement, StatefulInteractiveElement, Styled, Window, WindowControlArea, div, img, px,
    rgb, rgba, svg,
};
use herogpui::theme::ActiveTheme;

use crate::app::{AppController, ScreenMode};
use tableminus_gfx::glyphs::EngineKind;
use tableminus_state::connection_window::ConnectionWindowPhase;
use tableminus_windows_platform::WindowControlsStyle;
use tablepro_core::SafeModeLevel;

use super::TableMinusWorkspace;
use super::engine_icons::engine_icon;
use super::theme::{
    CONTROL_RADIUS_PX, HAIRLINE_BORDER_PX, hairline_border_color, is_reduced_motion,
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
        .flex_shrink(1.0);

    if !is_windows_controls {
        leading = leading.child(render_traffic_lights(skip_anim, cx));
    }

    leading = leading
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .min_w(px(0.0))
                .flex_shrink(1.0)
                .pl(px(if is_windows_controls { 0.0 } else { 4.0 }))
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
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.foreground)
                        .child(ws.controller.strings.app_title),
                ),
        )
        .child(render_status_pill(&ws.controller, cx))
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

fn render_status_pill(controller: &AppController, cx: &Context<TableMinusWorkspace>) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &controller.strings;
    match controller.screen_mode {
        ScreenMode::ConnectionsLibrary => {
            return render_screen_pill(IconName::Server, s.connections_library, cx);
        }
        ScreenMode::Settings => return render_screen_pill(IconName::Settings, s.settings, cx),
        ScreenMode::Workspace => {}
    }

    match controller.connections.phase(controller.active_workspace) {
        Some(ConnectionWindowPhase::Connected) => render_connected_pill(controller, cx),
        Some(ConnectionWindowPhase::Connecting { .. }) => {
            render_spinner_chip("titlebar-conn-spinner", s.connecting)
        }
        Some(ConnectionWindowPhase::Reconnecting { .. }) => {
            render_spinner_chip("titlebar-reconn-spinner", s.reconnecting)
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
            .child(ChipLabel::new().child(s.unavailable))
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
            .child(ChipLabel::new().child(s.disconnected))
            .into_any_element(),
    }
}

fn render_screen_pill(
    icon: IconName,
    label: &'static str,
    cx: &Context<TableMinusWorkspace>,
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
    cx: &Context<TableMinusWorkspace>,
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
        .flex_shrink(1.0)
        .overflow_hidden()
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
            let is_not_connected = crate::i18n::Locale::ALL.iter().any(|loc| {
                status.engine_version == crate::i18n::Strings::for_locale(*loc).not_connected
            });
            if !status.engine_version.is_empty() && !is_not_connected {
                status.engine_version.clone()
            } else {
                controller.strings.connected.to_owned()
            }
        }
    }
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
                                .text_size(px(11.5))
                                .text_color(colors.muted)
                                .child(strings.command_placeholder),
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
