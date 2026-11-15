//! Inline connection-unavailable, connecting, reconnecting, and auth-failed banners.

use tableminus_state::connection_window::{ConnectionUnavailableView, ConnectionWindowPhase};
use herogpui::components::{
    Alert, Button, Chip, ChipLabel, ChipVariant, Color, Icon, IconName, IconSize, Size, Spinner,
    SpinnerSize, Variant,
};
use herogpui::gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, Window, div, px,
};
use herogpui::theme::ActiveTheme;

use super::TableMinusWorkspace;
use super::engine_icons::engine_icon;
use super::shell_state::ToastSeverity;
use super::theme::{
    COMPACT_GAP_PX, HAIRLINE_BORDER_PX, MICRO_GAP_PX, hairline_border_color, is_reduced_motion,
    panel_surface_bg,
};
use crate::app::ScreenMode;

/// Renders an inline banner when the active workspace phase is unavailable,
/// connecting, or reconnecting. Returns `None` when connected or idle.
#[must_use]
pub fn render_unavailable_banner(
    ws: &mut TableMinusWorkspace,
    _window: &mut Window,
    cx: &mut Context<TableMinusWorkspace>,
) -> Option<AnyElement> {
    let phase = ws
        .controller
        .connections
        .phase(ws.controller.active_workspace)?
        .clone();

    let banner = match phase {
        ConnectionWindowPhase::Connecting { .. } => render_connecting_banner(ws, false, cx),
        ConnectionWindowPhase::Reconnecting { .. } => render_connecting_banner(ws, true, cx),
        ConnectionWindowPhase::Unavailable(view) => render_error_banner(ws, &view, cx),
        ConnectionWindowPhase::Disconnected | ConnectionWindowPhase::Connected => return None,
    };

    Some(banner)
}

/// Reusable compact card for disconnected, unsupported, or unavailable surfaces.
#[must_use]
pub fn render_unavailable_card(
    title: &str,
    detail: &str,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let mut alert = Alert::new(title.to_owned()).status(Color::Default);
    if !detail.trim().is_empty() {
        alert = alert.description(detail.to_owned());
    }

    div()
        .id("workspace-unavailable-card")
        .w_full()
        .px(px(12.0))
        .py(px(8.0))
        .bg(panel_surface_bg(cx))
        .border_b(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .child(alert)
        .into_any_element()
}

fn render_connecting_banner(
    ws: &mut TableMinusWorkspace,
    reconnecting: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &ws.controller.strings;
    let cancel_label = s.cancel_connection;
    let cancelled_title = s.connection_cancelled_title;
    let cancelled_desc = s.connection_cancelled_desc;
    let engine = ws.controller.connection_form.engine;
    let (title, status, spinner_color) = if reconnecting {
        (
            s.reconnecting_to_db.to_owned(),
            Color::Warning,
            Color::Warning,
        )
    } else {
        (s.connecting_to_db.to_owned(), Color::Accent, Color::Accent)
    };
    let target = active_target_summary(ws);

    let indicator = if is_reduced_motion(cx) {
        Icon::new(IconName::LoaderCircle)
            .size(IconSize::Md)
            .color(colors.accent.color)
            .into_any_element()
    } else {
        Spinner::new("workspace-connect-spinner")
            .size(SpinnerSize::Sm)
            .color(spinner_color)
            .into_any_element()
    };

    let cancel_btn = Button::new("unavailable-cancel-connect")
        .label(cancel_label)
        .variant(Variant::Secondary)
        .size(Size::Sm)
        .content(move |_| {
            div()
                .flex()
                .items_center()
                .gap(px(MICRO_GAP_PX))
                .child(Icon::new(IconName::X).size(IconSize::Xs))
                .child(cancel_label)
                .into_any_element()
        })
        .on_press(cx.listener(move |ws, _, _, cx| {
            ws.controller.cancel_active_connect();
            ws.notify_toast(
                ToastSeverity::Info,
                cancelled_title,
                Some(cancelled_desc.into()),
                cx,
            );
            cx.notify();
        }));

    let target_chip = Chip::new()
        .variant(ChipVariant::Secondary)
        .size(Size::Sm)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(MICRO_GAP_PX))
                .child(engine_icon(engine, px(13.0), colors.foreground))
                .child(ChipLabel::new().child(engine.display_name())),
        );

    let alert = Alert::new(title)
        .status(status)
        .description(target)
        .indicator(indicator)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(COMPACT_GAP_PX))
                .child(target_chip)
                .child(cancel_btn),
        );

    banner_shell("workspace-connecting-banner", alert, cx)
}

fn render_error_banner(
    ws: &mut TableMinusWorkspace,
    view: &ConnectionUnavailableView,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &ws.controller.strings;
    let auth_failed_badge = s.auth_failed_badge;
    let retry_label = s.retry_connection;
    let retry_failed_title = s.retry_failed;
    let edit_connection_label = s.edit_connection;
    let summary = if view.summary.trim().is_empty() || view.summary == "Connection unavailable" {
        s.unavailable_long.to_owned()
    } else {
        view.summary.clone()
    };
    let target = active_target_summary(ws);
    let detail_text = match view.detail.as_deref().map(str::trim) {
        Some(detail) if !detail.is_empty() => format!("{detail} ({target})"),
        _ => target,
    };

    let mut actions = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(COMPACT_GAP_PX));

    if view.is_auth_failure {
        actions = actions.child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .color(Color::Warning)
                .size(Size::Sm)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(MICRO_GAP_PX))
                        .child(
                            Icon::new(IconName::Key)
                                .size(IconSize::Xs)
                                .color(colors.warning.color),
                        )
                        .child(ChipLabel::new().child(auth_failed_badge)),
                ),
        );
    }

    if view.can_retry {
        actions = actions.child(
            Button::new("unavailable-retry-connect")
                .label(retry_label)
                .variant(Variant::Primary)
                .size(Size::Sm)
                .on_press(cx.listener(move |ws, _, _, cx| {
                    if let Err(err) = ws.controller.retry_unavailable_connection() {
                        ws.notify_toast(
                            ToastSeverity::Error,
                            retry_failed_title,
                            Some(err.to_string().into()),
                            cx,
                        );
                    }
                    cx.notify();
                })),
        );
    }

    actions = actions.child(
        Button::new("unavailable-edit-connection")
            .label(edit_connection_label)
            .variant(Variant::Secondary)
            .size(Size::Sm)
            .on_press(cx.listener(|ws, _, _window, cx| {
                ws.controller.edit_unavailable_connection();
                ws.library_ui
                    .sync_from_controller(&ws.controller.connection_form, cx);
                ws.switch_screen(ScreenMode::ConnectionsLibrary, cx);
                cx.notify();
            })),
    );

    let icon = if view.is_auth_failure {
        IconName::Lock
    } else {
        IconName::TriangleAlert
    };

    let alert = Alert::new(summary)
        .status(Color::Danger)
        .indicator(
            Icon::new(icon)
                .size(IconSize::Md)
                .color(colors.danger.color),
        )
        .description(detail_text)
        .child(actions);

    banner_shell("workspace-unavailable-banner", alert, cx)
}

fn banner_shell(id: &'static str, alert: Alert, cx: &Context<TableMinusWorkspace>) -> AnyElement {
    div()
        .id(id)
        .w_full()
        .px(px(12.0))
        .py(px(8.0))
        .bg(panel_surface_bg(cx))
        .border_b(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .child(alert)
        .into_any_element()
}

fn active_target_summary(ws: &TableMinusWorkspace) -> String {
    let form = &ws.controller.connection_form;
    let host = if !ws.controller.toolbar_status.host.trim().is_empty() {
        ws.controller.toolbar_status.host.trim()
    } else if !form.host.trim().is_empty() {
        form.host.trim()
    } else if !form.database.trim().is_empty() {
        form.database.trim()
    } else {
        ws.controller.strings.target_endpoint_fallback
    };

    let db = form.database.trim();
    if db.is_empty() || host == db {
        format!("{} · {host}", form.engine.display_name())
    } else {
        format!("{} · {host} / {db}", form.engine.display_name())
    }
}
