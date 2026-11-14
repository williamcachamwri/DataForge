//! Non-blocking Sonner-style HeroGPUI `ToastViewport` overlay and typed action-toast helpers.

use std::time::Duration;

use herogpui::components::{
    Alert, Color, DEFAULT_TOAST_TIMEOUT, Toast, ToastHotkey, ToastPlacement, ToastStore,
    ToastViewport, clear_toasts, dismiss_toast, toast_store,
};
use herogpui::gpui::{
    AnyElement, App, Context, ElementId, IntoElement, SharedString, WeakEntity, Window, px,
};

use crate::app::ScreenMode;
use crate::i18n::Strings;
use crate::services::resolve_active_strings;

use super::DataForgeWorkspace;
use super::shell_state::ToastSeverity;
use super::theme::{OVERLAY_EXIT_MS, effective_duration_ms};

/// Renders the non-blocking bottom-right HeroGPUI [`ToastViewport`] and subscribes
/// [`DataForgeWorkspace`] to live [`ToastStore`] changes so queued, updated, and
/// auto-dismissed toasts repaint immediately.
#[must_use]
pub fn render_toast_viewport(
    _ws: &DataForgeWorkspace,
    window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let weak = cx.entity().downgrade();
    let store = toast_store(cx);
    let store_for_sub = store.clone();
    let _store_sub = window.use_keyed_state(
        ElementId::Name("dataforge-toast-store-subscription".into()),
        cx,
        move |_, cx| {
            ToastStore::subscribe(&store_for_sub, cx, move |app| {
                if let Some(view) = weak.upgrade() {
                    view.update(app, |_, cx| cx.notify());
                }
            })
        },
    );

    let exit_duration = Duration::from_millis(effective_duration_ms(OVERLAY_EXIT_MS, cx));

    ToastViewport::new()
        .id("dataforge-toast-viewport")
        .placement(ToastPlacement::BottomEnd)
        .gap(px(8.0))
        .inset(px(16.0))
        .width(px(380.0))
        .scale_factor(0.05)
        .max_visible_toasts(4)
        .exit_duration(exit_duration)
        .hotkey(ToastHotkey::ALT_T)
        .into_any_element()
}

/// Dispatches a dismissible HeroGPUI [`Toast`] with an optional trailing action link.
pub fn push_action_toast(
    severity: ToastSeverity,
    title: impl Into<SharedString>,
    description: Option<SharedString>,
    action_label: impl Into<SharedString>,
    on_action: impl Fn(&mut App) + 'static,
    cx: &mut App,
) -> u64 {
    let color = severity_to_color(severity);
    let mut toast = Toast::new(title)
        .variant(color)
        .closable(true)
        .timeout(DEFAULT_TOAST_TIMEOUT)
        .action(action_label, on_action);
    if let Some(desc) = description {
        toast = toast.description(desc);
    }
    toast.push(None, cx)
}

fn active_workspace_strings(workspace: &WeakEntity<DataForgeWorkspace>, cx: &App) -> Strings {
    workspace
        .upgrade()
        .map_or_else(resolve_active_strings, |view| {
            view.read(cx).controller.strings.clone()
        })
}

/// Pushes an error toast with a localized retry action link bound to `workspace`.
pub fn push_retry_connection_toast(
    workspace: WeakEntity<DataForgeWorkspace>,
    title: impl Into<SharedString>,
    error_detail: impl Into<SharedString>,
    cx: &mut App,
) -> u64 {
    let strings = active_workspace_strings(&workspace, cx);
    push_action_toast(
        ToastSeverity::Error,
        title,
        Some(error_detail.into()),
        strings.retry_connection,
        move |app| {
            if let Some(view) = workspace.upgrade() {
                view.update(app, |ws, cx| {
                    if let Err(err) = ws.controller.retry_unavailable_connection() {
                        let retry_failed = ws.controller.strings.retry_failed;
                        ws.notify_toast(
                            ToastSeverity::Error,
                            retry_failed,
                            Some(err.to_string().into()),
                            cx,
                        );
                    }
                    cx.notify();
                });
            }
        },
        cx,
    )
}

/// Pushes a notification toast with a localized `"Open Settings"` action link.
pub fn push_open_settings_toast(
    workspace: WeakEntity<DataForgeWorkspace>,
    severity: ToastSeverity,
    title: impl Into<SharedString>,
    description: Option<SharedString>,
    cx: &mut App,
) -> u64 {
    let strings = active_workspace_strings(&workspace, cx);
    push_action_toast(
        severity,
        title,
        description,
        strings.open_settings,
        move |app| {
            if let Some(view) = workspace.upgrade() {
                view.update(app, |ws, cx| {
                    ws.switch_screen(ScreenMode::Settings, cx);
                });
            }
        },
        cx,
    )
}

/// Dismisses a single toast by id from the shared HeroGPUI [`ToastStore`].
pub fn dismiss_notification(id: u64, cx: &mut App) {
    dismiss_toast(id, cx);
}

/// Clears all active notifications from the shared HeroGPUI [`ToastStore`].
pub fn clear_all_notifications(cx: &mut App) {
    clear_toasts(cx);
}

/// Renders a HeroGPUI [`Alert`] element matching [`ToastSeverity`] semantics.
#[must_use]
pub fn render_status_alert(
    severity: ToastSeverity,
    title: impl Into<SharedString>,
    description: Option<SharedString>,
) -> AnyElement {
    let mut alert = Alert::new(title).status(severity_to_color(severity));
    if let Some(desc) = description {
        alert = alert.description(desc);
    }
    alert.into_any_element()
}

const fn severity_to_color(severity: ToastSeverity) -> Color {
    match severity {
        ToastSeverity::Info => Color::Accent,
        ToastSeverity::Success => Color::Success,
        ToastSeverity::Warning => Color::Warning,
        ToastSeverity::Error => Color::Danger,
    }
}
