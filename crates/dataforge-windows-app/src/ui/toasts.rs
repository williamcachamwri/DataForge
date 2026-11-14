//! Non-blocking Sonner-style HeroGPUI `ToastViewport` overlay and typed action-toast helpers.

use std::time::Duration;

use herogpui::components::{
    Alert, Color, DEFAULT_TOAST_TIMEOUT, Toast, ToastHotkey, ToastPlacement, ToastStore,
    ToastViewport, clear_toasts, dismiss_toast, toast_store,
};
use herogpui::gpui::{
    AnyElement, App, Bounds, Context, Element, ElementId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, SharedString, WeakEntity, Window, deferred, px,
};
use herogpui::theme::ThemeProvider;

use crate::app::ScreenMode;
use crate::i18n::Strings;
use crate::services::resolve_active_strings;

use super::DataForgeWorkspace;
use super::shell_state::ToastSeverity;
use super::theme::{OVERLAY_EXIT_MS, effective_duration_ms, toast_opaque_theme};

/// Deferred paint priority above HeroGPUI's default overlays (`0`) so floating
/// toast notifications always layer above workspace controls (`Select` triggers
/// and popovers, grid footer, toolbars, modals, and command palette).
const TOAST_DEFERRED_PRIORITY: usize = 10;

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

    let viewport = ToastViewport::new()
        .id("dataforge-toast-viewport")
        .placement(ToastPlacement::BottomEnd)
        .gap(px(8.0))
        .inset(px(16.0))
        .width(px(380.0))
        .scale_factor(0.05)
        .max_visible_toasts(4)
        .exit_duration(exit_duration)
        .hotkey(ToastHotkey::ALT_T);

    deferred(OpaqueToastThemeScope::new(viewport))
        .with_priority(TOAST_DEFERRED_PRIORITY)
        .into_any_element()
}

/// Runs `f` while `ThemeProvider`'s active theme uses opaque overlay tokens,
/// then restores the previous theme in place without triggering window refreshes.
fn with_opaque_toast_theme<R>(cx: &mut App, f: impl FnOnce(&mut App) -> R) -> R {
    let previous = cx
        .try_global::<ThemeProvider>()
        .map(|provider| provider.theme().clone());
    if let Some(ref base) = previous {
        let opaque = toast_opaque_theme(base);
        cx.global_mut::<ThemeProvider>().insert(opaque);
    }
    let result = f(cx);
    if let Some(base) = previous {
        cx.global_mut::<ThemeProvider>().insert(base);
    }
    result
}

/// Element wrapper that isolates [`ToastViewport`] inside an opaque overlay theme
/// snapshot across layout, prepaint, and paint while preserving workspace translucency.
struct OpaqueToastThemeScope {
    child: AnyElement,
}

impl OpaqueToastThemeScope {
    fn new(child: impl IntoElement) -> Self {
        Self {
            child: child.into_any_element(),
        }
    }
}

impl IntoElement for OpaqueToastThemeScope {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for OpaqueToastThemeScope {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let layout_id = with_opaque_toast_theme(cx, |cx| self.child.request_layout(window, cx));
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        with_opaque_toast_theme(cx, |cx| {
            let _ = self.child.prepaint(window, cx);
        });
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        with_opaque_toast_theme(cx, |cx| self.child.paint(window, cx));
    }
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
