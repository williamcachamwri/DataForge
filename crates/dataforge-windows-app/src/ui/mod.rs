//! HeroGPUI application root, theme/icon bridges, and Wave 2 workspace contracts.

pub mod engine_icons;
pub mod shell_state;
pub mod theme;

use std::collections::BTreeMap;
use std::sync::Arc;

use dataforge_state::tabs::TabId;
use dataforge_windows_platform::detect_theme_or_default;
use herogpui::application;
use herogpui::components::{HeroGpuiAssets, TitleBar, ToastViewport, WindowBorder, app_focus_root};
use herogpui::gpui::{
    AnyElement, App, AppContext as _, AsyncApp, Bounds, Context, FocusHandle, Focusable,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString, Styled, Subscription,
    Task, WeakEntity, Window, WindowBounds, div, px, size,
};
use herogpui::theme::ActiveTheme;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::HWND;

use crate::app::{AppController, ScreenMode};
use crate::branding::apply_window_icons;
use crate::ipc::IpcInbox;
use crate::services::ServiceEvent;
use crate::window::apply_dwm_dark_mode;

pub use engine_icons::{
    ENGINE_SVG_SPECS, EngineSvgSpec, build_engine_paths, engine_icon, engine_svg_spec,
};
pub use shell_state::{
    AiPanelState, ConnectionScreenFocus, ConnectionsViewState, InlineCellEditState,
    QueryTabResultState, ResultPaneSubView, SettingsSection, SettingsViewState, ToastSeverity,
    WorkspacePaneState, WorkspaceSubView, push_error_toast, push_success_toast, push_toast,
};
pub use theme::{
    ContentThemePalette, HIGH_CONTRAST_THEME_ID, apply_chrome_theme_and_motion,
    chrome_theme_for_mode, high_contrast_chrome_theme, high_contrast_theme_from_palette,
    state_rgba_to_gpui, state_rgba_to_hsla,
};

/// Root GPUI workspace entity owning live [`AppController`] state, per-tab query
/// accumulators, screen sub-view state, reactive IPC/service loops, and native
/// Win32 window branding/guards.
pub struct DataForgeWorkspace {
    pub controller: AppController,
    pub ipc_inbox: Arc<IpcInbox>,
    pub focus_handle: FocusHandle,
    pub content_palette: ContentThemePalette,
    pub tab_results: BTreeMap<TabId, QueryTabResultState>,
    pub connections_view: ConnectionsViewState,
    pub settings_view: SettingsViewState,
    pub workspace_pane: WorkspacePaneState,
    pub ai_panel: AiPanelState,
    pub pending_quit_after_confirm: bool,
    _tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl DataForgeWorkspace {
    /// Constructs the GPUI root workspace and wires Win32 branding, appearance sync,
    /// close guard, and reactive IPC/service wakeup listeners.
    pub fn new(
        mut controller: AppController,
        ipc_inbox: Arc<IpcInbox>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        controller.headless = false;
        let content_palette = ContentThemePalette::from_definition(&controller.content_theme);
        apply_chrome_theme_and_motion(controller.effective_theme_mode(), cx);
        install_win32_window_branding(window, controller.uses_dark_titlebar());

        let focus_handle = cx.focus_handle();
        let (tasks, subscriptions) = spawn_reactive_bridges(&controller, &ipc_inbox, window, cx);
        install_close_guard(window, cx);

        Self {
            controller,
            ipc_inbox,
            focus_handle,
            content_palette,
            tab_results: BTreeMap::new(),
            connections_view: ConnectionsViewState::default(),
            settings_view: SettingsViewState::default(),
            workspace_pane: WorkspacePaneState::default(),
            ai_panel: AiPanelState::default(),
            pending_quit_after_confirm: false,
            _tasks: tasks,
            _subscriptions: subscriptions,
        }
    }

    /// Re-synchronizes HeroGPUI Chrome [`herogpui::theme::Theme`], [`ContentThemePalette`],
    /// and reduced motion after a theme preference, custom theme import, or OS change.
    pub fn refresh_theme(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.content_palette = ContentThemePalette::from_definition(&self.controller.content_theme);
        apply_chrome_theme_and_motion(self.controller.effective_theme_mode(), cx);
        install_win32_window_branding(window, self.controller.uses_dark_titlebar());
        cx.notify();
    }

    /// Drains any pending forwarded IPC targets into the controller.
    pub fn process_pending_ipc(&mut self, cx: &mut Context<Self>) {
        let targets = self.ipc_inbox.drain_targets();
        if !targets.is_empty() {
            self.controller.open_forwarded_targets(&targets);
            cx.notify();
        }
    }

    /// Drains async service events into `AppController` and per-tab query accumulators.
    pub fn process_service_events(&mut self, cx: &mut Context<Self>) {
        let events = {
            let Some(services) = &self.controller.services else {
                return;
            };
            services.drain_events()
        };
        if events.is_empty() {
            return;
        }
        let active_tab = self.controller.tabs.active_tab();
        for event in &events {
            if let Some(tab_id) = active_tab {
                mirror_query_event_to_tab(&mut self.tab_results, tab_id, event);
            }
        }
        self.controller.drain_service_events();
        cx.notify();
    }

    /// Returns the immutable [`QueryTabResultState`] for the currently active tab.
    #[must_use]
    pub fn active_tab_result(&self) -> Option<&QueryTabResultState> {
        let tab_id = self.controller.tabs.active_tab()?;
        self.tab_results.get(&tab_id)
    }

    /// Returns a mutable [`QueryTabResultState`] for `tab_id`, creating one if missing.
    pub fn ensure_tab_result(&mut self, tab_id: TabId) -> &mut QueryTabResultState {
        self.tab_results
            .entry(tab_id)
            .or_insert_with(|| QueryTabResultState::new(tab_id))
    }

    /// Dispatches a HeroGPUI toast notification.
    pub fn notify_toast(
        &self,
        severity: ToastSeverity,
        title: impl Into<SharedString>,
        description: Option<SharedString>,
        cx: &mut App,
    ) -> u64 {
        push_toast(severity, title, description, cx)
    }
}

impl Focusable for DataForgeWorkspace {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DataForgeWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.colors().clone();
        let screen_label = match self.controller.screen_mode {
            ScreenMode::Workspace => "Workspace",
            ScreenMode::ConnectionsLibrary => "Connections",
            ScreenMode::Settings => "Settings",
        };
        let status_summary = self.controller.toolbar_status.format_pill_text();

        let body: AnyElement = div()
            .flex_1()
            .size_full()
            .flex()
            .flex_col()
            .bg(self.content_palette.window_background)
            .text_color(colors.foreground)
            .p(px(16.0))
            .gap(px(8.0))
            .child(
                div()
                    .text_sm()
                    .font_weight(herogpui::gpui::FontWeight::SEMIBOLD)
                    .child(format!(
                        "{} — {screen_label}",
                        self.controller.strings.app_title
                    )),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted)
                    .child(status_summary),
            )
            .into_any_element();

        let shell = WindowBorder::new().child(
            div()
                .flex()
                .flex_col()
                .size_full()
                .bg(colors.background)
                .text_color(colors.foreground)
                .child(TitleBar::new("dataforge-titlebar").title(self.controller.strings.app_title))
                .child(body)
                .child(ToastViewport::new().id("dataforge-toast-viewport")),
        );

        app_focus_root(
            div()
                .size_full()
                .track_focus(&self.focus_handle)
                .child(shell),
            window,
            cx,
        )
    }
}

/// Launches the HeroGPUI desktop shell with [`HeroGpuiAssets`] registered,
/// reduced motion initialized, and [`DataForgeWorkspace`] mounted as root view.
pub fn launch_gpui_app(controller: AppController, ipc_inbox: Arc<IpcInbox>) {
    application()
        .with_assets(HeroGpuiAssets)
        .run(move |cx: &mut App| {
            herogpui::init(cx);
            apply_chrome_theme_and_motion(controller.effective_theme_mode(), cx);

            let width =
                f32::from(u16::try_from(controller.settings.window.width.max(800)).unwrap_or(1280));
            let height =
                f32::from(u16::try_from(controller.settings.window.height.max(540)).unwrap_or(800));
            let bounds = Bounds::centered(None, size(px(width), px(height)), cx);

            let mut options = TitleBar::window_options();
            options.window_bounds = Some(WindowBounds::Windowed(bounds));
            if let Some(titlebar) = options.titlebar.as_mut() {
                titlebar.title = Some(controller.strings.app_title.into());
            }

            let title = controller.strings.app_title;
            if let Err(err) = cx.open_window(options, move |window, cx| {
                window.set_window_title(title);
                cx.new(|cx| DataForgeWorkspace::new(controller, ipc_inbox, window, cx))
            }) {
                tracing::error!(error = %err, "failed to open DataForge GPUI window");
            }
        });
}

fn install_win32_window_branding(window: &Window, dark_titlebar: bool) {
    if let Ok(handle) = HasWindowHandle::window_handle(window)
        && let RawWindowHandle::Win32(win32) = handle.as_raw()
    {
        let hwnd = HWND(win32.hwnd.get() as *mut std::ffi::c_void);
        apply_window_icons(hwnd);
        apply_dwm_dark_mode(hwnd, dark_titlebar);
    }
}

fn install_close_guard(window: &Window, cx: &mut Context<DataForgeWorkspace>) {
    let weak: WeakEntity<DataForgeWorkspace> = cx.entity().downgrade();
    window.on_window_should_close(cx, move |_window, cx| {
        let Some(entity) = weak.upgrade() else {
            return true;
        };
        let needs_guard = entity.read(cx).controller.needs_close_guard();
        if !needs_guard {
            return true;
        }
        entity.update(cx, |ws, cx| {
            if ws.pending_quit_after_confirm {
                return;
            }
            ws.workspace_pane.close_guard_open = true;
            cx.notify();
        });
        false
    });
}

fn spawn_reactive_bridges(
    controller: &AppController,
    ipc_inbox: &Arc<IpcInbox>,
    window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> (Vec<Task<()>>, Vec<Subscription>) {
    let mut tasks = Vec::new();
    let ipc_rx = ipc_inbox.subscribe();
    tasks.push(cx.spawn(
        async move |this: WeakEntity<DataForgeWorkspace>, cx: &mut AsyncApp| {
            while ipc_rx.recv().await.is_ok() {
                let Ok(()) = this.update(cx, |ws, cx| ws.process_pending_ipc(cx)) else {
                    break;
                };
            }
        },
    ));

    if let Some(services) = &controller.services {
        let svc_rx = services.subscribe();
        tasks.push(cx.spawn(
            async move |this: WeakEntity<DataForgeWorkspace>, cx: &mut AsyncApp| {
                while svc_rx.recv().await.is_ok() {
                    let Ok(()) = this.update(cx, |ws, cx| ws.process_service_events(cx)) else {
                        break;
                    };
                }
            },
        ));
    }

    let weak_appearance = cx.entity().downgrade();
    let appearance_sub = window.observe_window_appearance(move |window, cx| {
        if let Some(entity) = weak_appearance.upgrade() {
            let detected = detect_theme_or_default();
            entity.update(cx, |ws, cx| {
                ws.controller.on_system_theme_changed(detected);
                ws.refresh_theme(window, cx);
            });
        }
    });
    (tasks, vec![appearance_sub])
}

fn mirror_query_event_to_tab(
    tab_results: &mut BTreeMap<TabId, QueryTabResultState>,
    tab_id: TabId,
    event: &ServiceEvent,
) {
    let entry = tab_results
        .entry(tab_id)
        .or_insert_with(|| QueryTabResultState::new(tab_id));
    match event {
        ServiceEvent::QueryStreamEvent { event, .. } => {
            entry.running = true;
            entry.push_event(event.clone());
        }
        ServiceEvent::QueryFinished {
            summary,
            elapsed_ms,
            ..
        } => match summary {
            Ok(summary) => entry.finish(summary.clone(), *elapsed_ms),
            Err(err) => {
                entry.elapsed_ms = Some(*elapsed_ms);
                entry.fail(err.clone());
            }
        },
        _ => {}
    }
}
