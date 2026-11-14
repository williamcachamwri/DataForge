//! HeroGPUI application root, theme/icon/syntax bridges, and Wave 2 workspace contracts.

pub mod actions;
pub mod ai_drawer;
pub mod command_palette;
pub mod data_grid;
pub mod engine_icons;
pub mod inspector;
pub mod library;
pub mod modals;
pub mod rail;
pub mod results_pane;
pub mod settings;
pub mod shell_state;
pub mod sidebar;
pub mod sql_editor;
pub mod status_bar;
pub mod structure_view;
pub mod syntax_bridge;
pub mod tabs;
pub mod theme;
pub mod titlebar;
pub mod toasts;
pub mod toolbar;
pub mod unavailable;

use std::collections::BTreeMap;
use std::sync::Arc;

use dataforge_gfx::glyphs::EngineKind;
use dataforge_state::tabs::{TabId, TabKind};
use dataforge_state::theme::ThemeDefinition;
use dataforge_windows_platform::{
    ThemePreference, WindowBackdropPreference, detect_theme_or_default,
};
use herogpui::application;
use herogpui::components::{HeroGpuiAssets, TitleBar, WindowBorder, app_focus_root};
use herogpui::core::with_alpha;
use herogpui::gpui::{
    AnyElement, App, AppContext as _, AsyncApp, Bounds, Context, Div, FocusHandle, Focusable,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString, Styled, Subscription,
    Task, WeakEntity, Window, WindowBackgroundAppearance, WindowBounds, div, px, size,
};
use herogpui::theme::ActiveTheme;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tablepro_core::sql_syntax::SqlGrammar;
use windows::Win32::Foundation::HWND;

use crate::app::{AppController, ScreenMode};
use crate::branding::apply_window_icons;
use crate::ipc::IpcInbox;
use crate::services::ServiceEvent;
use crate::views::status_bar::TableViewMode;
use crate::window::apply_dwm_dark_mode;

pub use actions::{WORKSPACE_KEY_CONTEXT, bind_workspace_keys};
pub use engine_icons::{
    ENGINE_SVG_SPECS, EngineSvgSpec, build_engine_paths, engine_icon, engine_svg_spec,
};
pub use shell_state::{
    AiPanelState, ConnectionScreenFocus, ConnectionsViewState, InlineCellEditState,
    QueryTabResultState, ResultPaneSubView, SettingsSection, SettingsViewState, ToastSeverity,
    WorkspacePaneState, WorkspaceSubView, push_error_toast, push_success_toast, push_toast,
};
pub use syntax_bridge::{
    build_line_text_runs, grammar_for_engine, highlight_sql_line, highlight_sql_text,
    highlight_style_for_kind, styled_sql_block, utf16_spans_to_byte_highlights,
};
pub use theme::{
    ContentThemePalette, HIGH_CONTRAST_THEME_ID, apply_chrome_theme_and_motion,
    chrome_theme_for_mode, editor_gpui_font, gpui_font_from_editor, gpui_font_from_spec,
    high_contrast_chrome_theme, high_contrast_theme_from_palette, state_rgba_to_gpui,
    state_rgba_to_hsla, value_gpui_font,
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
    pub tabs_ui: tabs::TabsStripState,
    pub command_palette_ui: command_palette::CommandPaletteState,
    pub sidebar_ui: sidebar::SidebarPanelState,
    pub library_ui: library::LibraryScreenState,
    pub sql_editor_ui: sql_editor::SqlEditorViewState,
    pub data_grid_ui: data_grid::DataGridViewState,
    pub structure_ui: structure_view::StructureViewState,
    pub results_ui: results_pane::ResultsPaneState,
    pub inspector_ui: inspector::InspectorViewState,
    pub ai_drawer_ui: ai_drawer::AiDrawerViewState,
    pub settings_ui: settings::SettingsScreenState,
    pub modals_ui: modals::ModalLayerState,
    _tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl DataForgeWorkspace {
    /// Constructs the GPUI root workspace and wires Win32 branding, appearance sync,
    /// close guard, modular Wave 2 view states, and reactive IPC/service listeners.
    pub fn new(
        mut controller: AppController,
        ipc_inbox: Arc<IpcInbox>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        controller.headless = false;
        apply_saved_theme_presets(&mut controller);
        let mut content_palette = ContentThemePalette::from_definition(&controller.content_theme);
        theme::apply_backdrop_to_content_palette(
            &mut content_palette,
            controller.settings.window_backdrop,
        );
        apply_chrome_theme_and_motion(
            controller.effective_theme_mode(),
            controller.settings.window_backdrop,
            cx,
        );
        window.set_background_appearance(gpui_window_backdrop(controller.settings.window_backdrop));
        install_win32_window_branding(window, controller.uses_dark_titlebar());

        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
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
            tabs_ui: tabs::TabsStripState::new(window, cx),
            command_palette_ui: command_palette::CommandPaletteState::new(window, cx),
            sidebar_ui: sidebar::SidebarPanelState::new(window, cx),
            library_ui: library::LibraryScreenState::new(window, cx),
            sql_editor_ui: sql_editor::SqlEditorViewState::new(window, cx),
            data_grid_ui: data_grid::DataGridViewState::new(window, cx),
            structure_ui: structure_view::StructureViewState::new(window, cx),
            results_ui: results_pane::ResultsPaneState::new(window, cx),
            inspector_ui: inspector::InspectorViewState::new(window, cx),
            ai_drawer_ui: ai_drawer::AiDrawerViewState::new(window, cx),
            settings_ui: settings::SettingsScreenState::new(window, cx),
            modals_ui: modals::ModalLayerState::new(window, cx),
            _tasks: tasks,
            _subscriptions: subscriptions,
        }
    }

    /// Re-synchronizes HeroGPUI Chrome [`herogpui::theme::Theme`], [`ContentThemePalette`],
    /// and reduced motion after a theme preference, custom theme import, or OS change.
    pub fn refresh_theme(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.content_palette = ContentThemePalette::from_definition(&self.controller.content_theme);
        theme::apply_backdrop_to_content_palette(
            &mut self.content_palette,
            self.controller.settings.window_backdrop,
        );
        apply_chrome_theme_and_motion(
            self.controller.effective_theme_mode(),
            self.controller.settings.window_backdrop,
            cx,
        );
        window.set_background_appearance(gpui_window_backdrop(
            self.controller.settings.window_backdrop,
        ));
        install_win32_window_branding(window, self.controller.uses_dark_titlebar());
        cx.notify();
    }

    /// Toggles between Light and Dark themes (unless user explicitly locked High Contrast),
    /// refreshing both Content and Chrome palettes cleanly.
    pub fn toggle_theme_mode(&mut self, window: &Window, cx: &mut Context<Self>) {
        let next_pref = if self.controller.uses_dark_titlebar() {
            ThemePreference::Light
        } else {
            ThemePreference::Dark
        };
        self.controller.settings.theme = next_pref;
        let next_appearance = match next_pref {
            ThemePreference::Light => dataforge_state::theme::ThemeAppearance::Light,
            ThemePreference::Dark => dataforge_state::theme::ThemeAppearance::Dark,
            ThemePreference::HighContrast => dataforge_state::theme::ThemeAppearance::HighContrast,
            ThemePreference::System => dataforge_state::theme::ThemeAppearance::Dark,
        };
        self.controller.content_theme = ThemeDefinition::default_for(next_appearance);
        self.refresh_theme(window, cx);
    }

    /// Switches top-level screen mode and refreshes the root workspace view.
    pub fn switch_screen(&mut self, mode: ScreenMode, cx: &mut Context<Self>) {
        self.controller.screen_mode = mode;
        cx.notify();
    }

    /// Dismisses transient overlays (command palette, modals, inline cell edit, autocomplete).
    pub fn dismiss_overlays(&mut self, cx: &mut Context<Self>) {
        if self.command_palette_ui.is_open() {
            self.command_palette_ui.close();
        } else if self.modals_ui.has_open_modal(self) {
            self.modals_ui.close_all();
            self.workspace_pane.close_guard_open = false;
            self.workspace_pane.confirm_commit_open = false;
        } else if self.workspace_pane.inline_edit.take().is_some() {
            // Cancelled inline grid cell edit.
        } else {
            self.controller.editor_autocomplete = None;
        }
        cx.notify();
    }

    /// Returns the active SQL grammar derived from the connected workspace engine.
    #[must_use]
    pub fn active_grammar(&self) -> SqlGrammar {
        let engine = EngineKind::ALL
            .iter()
            .copied()
            .find(|kind| {
                self.controller
                    .toolbar_status
                    .engine_version
                    .starts_with(kind.display_name())
            })
            .unwrap_or(EngineKind::Postgresql);
        grammar_for_engine(engine)
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
        let mut connected_event = false;
        for event in &events {
            if matches!(event, ServiceEvent::Connected { .. }) {
                connected_event = true;
            }
            if let Some(tab_id) = active_tab {
                mirror_query_event_to_tab(&mut self.tab_results, tab_id, event);
            }
        }
        self.controller.apply_service_events(events);
        if connected_event {
            self.sidebar_ui.reset_session_cache();
        }
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

    /// Synchronizes the workspace center sub-view and editor SQL text with the currently selected tab.
    pub fn sync_active_tab_sub_view(&mut self) {
        let Some(active_id) = self.controller.tabs.active_tab() else {
            return;
        };
        let Some(tab) = self
            .controller
            .tabs
            .tabs()
            .iter()
            .find(|t| t.id == active_id)
            .cloned()
        else {
            return;
        };
        match tab.kind {
            TabKind::QueryEditor { sql } => {
                if !sql.is_empty() {
                    self.controller.set_editor_sql(&sql);
                }
                self.workspace_pane.sub_view = WorkspaceSubView::SqlEditor;
            }
            TabKind::TableData { .. } => {
                self.controller.status_bar.view_mode = TableViewMode::Data;
                self.workspace_pane.sub_view = WorkspaceSubView::DataGrid;
            }
            TabKind::Structure { .. } => {
                self.controller.status_bar.view_mode = TableViewMode::Structure;
                self.workspace_pane.sub_view = WorkspaceSubView::Structure;
            }
        }
    }

    fn execute_key_command(
        &mut self,
        cmd: crate::accessibility::KeyCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.controller.handle_key_command(cmd) {
            if self.controller.needs_close_guard() && !self.pending_quit_after_confirm {
                self.workspace_pane.close_guard_open = true;
                cx.notify();
                return;
            }
            window.remove_window();
            return;
        }
        self.sync_active_tab_sub_view();
        cx.notify();
    }

    fn attach_workspace_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        let root = self.attach_tab_and_nav_actions(root, cx);
        self.attach_query_and_grid_actions(root, cx)
    }

    fn attach_tab_and_nav_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.key_context(WORKSPACE_KEY_CONTEXT)
            .on_action(cx.listener(|ws, _: &actions::NewQueryTab, window, cx| {
                ws.execute_key_command(crate::accessibility::KeyCommand::NewQueryTab, window, cx);
            }))
            .on_action(cx.listener(|ws, _: &actions::CloseActiveTab, window, cx| {
                ws.execute_key_command(
                    crate::accessibility::KeyCommand::CloseTabOrWindow,
                    window,
                    cx,
                );
            }))
            .on_action(cx.listener(|ws, _: &actions::ReopenClosedTab, window, cx| {
                ws.execute_key_command(
                    crate::accessibility::KeyCommand::ReopenClosedTab,
                    window,
                    cx,
                );
            }))
            .on_action(cx.listener(|ws, _: &actions::NextTab, window, cx| {
                ws.execute_key_command(crate::accessibility::KeyCommand::NextTab, window, cx);
            }))
            .on_action(cx.listener(|ws, _: &actions::PrevTab, window, cx| {
                ws.execute_key_command(crate::accessibility::KeyCommand::PrevTab, window, cx);
            }))
            .on_action(
                cx.listener(|ws, _: &actions::OpenCommandPalette, window, cx| {
                    ws.command_palette_ui.toggle(window, cx);
                    cx.notify();
                }),
            )
            .on_action(
                cx.listener(|ws, _: &actions::FocusSidebarFilter, window, cx| {
                    ws.execute_key_command(
                        crate::accessibility::KeyCommand::FocusFilter,
                        window,
                        cx,
                    );
                    ws.sidebar_ui.focus_filter(window, cx);
                }),
            )
            .on_action(cx.listener(|ws, _: &actions::ToggleSidebar, window, cx| {
                ws.execute_key_command(crate::accessibility::KeyCommand::ToggleSidebar, window, cx);
            }))
            .on_action(cx.listener(|ws, _: &actions::ToggleInspector, window, cx| {
                ws.execute_key_command(
                    crate::accessibility::KeyCommand::ToggleInspector,
                    window,
                    cx,
                );
            }))
            .on_action(cx.listener(|ws, _: &actions::ToggleAiDrawer, window, cx| {
                ws.execute_key_command(
                    crate::accessibility::KeyCommand::ToggleAiDrawer,
                    window,
                    cx,
                );
            }))
            .on_action(cx.listener(|ws, _: &actions::OpenConnections, window, cx| {
                ws.execute_key_command(
                    crate::accessibility::KeyCommand::OpenConnectionsLibrary,
                    window,
                    cx,
                );
            }))
            .on_action(cx.listener(|ws, _: &actions::OpenSettings, window, cx| {
                ws.execute_key_command(crate::accessibility::KeyCommand::OpenSettings, window, cx);
            }))
            .on_action(cx.listener(|ws, _: &actions::ToggleTheme, window, cx| {
                ws.toggle_theme_mode(window, cx);
            }))
            .on_action(cx.listener(|ws, _: &actions::EscapeDismiss, _window, cx| {
                ws.dismiss_overlays(cx);
            }))
    }

    fn attach_query_and_grid_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|ws, _: &actions::RunQuery, _window, cx| {
            sql_editor::trigger_run_query(ws, false, cx);
        }))
        .on_action(
            cx.listener(|ws, _: &actions::RunSelectedQuery, _window, cx| {
                sql_editor::trigger_run_query(ws, true, cx);
            }),
        )
        .on_action(cx.listener(|ws, _: &actions::CancelQuery, _window, cx| {
            sql_editor::cancel_active_query(ws, cx);
        }))
        .on_action(cx.listener(|ws, _: &actions::SaveQuery, _window, cx| {
            let sql = ws.sql_editor_ui.current_text(cx);
            if !sql.trim().is_empty() {
                ws.controller.set_editor_sql(&sql);
            }
            let query_title = ws.controller.strings.new_query_tab;
            let saved_toast = ws.controller.strings.query_saved_toast;
            ws.controller.save_current_query(query_title, None);
            ws.notify_toast(ToastSeverity::Success, saved_toast, None, cx);
            cx.notify();
        }))
        .on_action(cx.listener(|ws, _: &actions::CommitEdits, _window, cx| {
            ws.workspace_pane.confirm_commit_open = true;
            cx.notify();
        }))
        .on_action(cx.listener(|ws, _: &actions::RollbackEdits, _window, cx| {
            ws.controller.rollback_grid_edits();
            ws.workspace_pane.inline_edit = None;
            cx.notify();
        }))
        .on_action(cx.listener(|ws, _: &actions::AddRow, _window, cx| {
            let col_count = ws.controller.grid_columns.len();
            let values = vec![tablepro_core::Value::Null; col_count];
            ws.controller.insert_grid_row(values);
            cx.notify();
        }))
        .on_action(
            cx.listener(|ws, _: &actions::DeleteSelectedRows, _window, cx| {
                if let Some((row_idx, _)) = ws.controller.grid_selection.focused {
                    ws.controller.delete_grid_row(row_idx);
                    cx.notify();
                }
            }),
        )
        .on_action(cx.listener(|ws, _: &actions::ExportResults, window, cx| {
            ws.modals_ui.open_export(window, cx);
            cx.notify();
        }))
    }

    fn render_center_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let active_tab = self.controller.tabs.active_tab();
        let sub_view = self.workspace_pane.sub_view;
        let pane = match sub_view {
            WorkspaceSubView::DataGrid => data_grid::render_data_grid(self, window, cx),
            WorkspaceSubView::Structure => structure_view::render_structure_view(self, window, cx),
            WorkspaceSubView::SqlEditor | WorkspaceSubView::SplitSqlAndGrid => {
                sql_editor::render_sql_editor(self, window, cx)
            }
        };
        let anim_id = SharedString::from(format!("center-pane-{active_tab:?}-{sub_view:?}"));
        theme::animate_slide_fade_in(
            div()
                .flex_1()
                .min_w(px(0.0))
                .min_h(px(0.0))
                .size_full()
                .flex()
                .flex_col()
                .child(pane),
            anim_id,
            180,
            6.0,
            cx,
        )
    }

    fn render_workspace_screen(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut middle_row = div()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .overflow_hidden()
            .flex()
            .flex_row();
        if self.controller.layout_config.sidebar_visible {
            let sidebar_el = sidebar::render_sidebar(self, window, cx);
            middle_row = middle_row.child(theme::animate_drawer_enter(
                div().h_full().flex_shrink_0().child(sidebar_el),
                "workspace-sidebar-drawer",
                false,
                cx,
            ));
        }
        middle_row = middle_row.child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .min_h(px(0.0))
                .flex()
                .flex_col()
                .child(self.render_center_pane(window, cx)),
        );
        if self.controller.layout_config.inspector_visible {
            let inspector_el = inspector::render_inspector(self, window, cx);
            middle_row = middle_row.child(theme::animate_drawer_enter(
                div().h_full().flex_shrink_0().child(inspector_el),
                "workspace-inspector-drawer",
                true,
                cx,
            ));
        }
        if self.controller.layout_config.ai_drawer_visible {
            let ai_el = ai_drawer::render_ai_drawer(self, window, cx);
            middle_row = middle_row.child(theme::animate_drawer_enter(
                div().h_full().flex_shrink_0().child(ai_el),
                "workspace-ai-drawer",
                true,
                cx,
            ));
        }

        let mut col = div()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .child(toolbar::render_toolbar(self, window, cx))
            .child(tabs::render_tab_strip(self, window, cx));
        if let Some(banner) = unavailable::render_unavailable_banner(self, window, cx) {
            col = col.child(banner);
        }
        col.child(middle_row).into_any_element()
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
        let screen_key = match self.controller.screen_mode {
            ScreenMode::Workspace => "screen-workspace",
            ScreenMode::ConnectionsLibrary => "screen-library",
            ScreenMode::Settings => "screen-settings",
        };
        let main_area = match self.controller.screen_mode {
            ScreenMode::Workspace => self.render_workspace_screen(window, cx),
            ScreenMode::ConnectionsLibrary => library::render_library_screen(self, window, cx),
            ScreenMode::Settings => settings::render_settings_screen(self, window, cx),
        };
        let main_area = theme::animate_fade_in(
            div()
                .flex_1()
                .min_w(px(0.0))
                .min_h(px(0.0))
                .overflow_hidden()
                .flex()
                .flex_col()
                .child(main_area),
            screen_key,
            theme::SCREEN_TRANSITION_MS,
            cx,
        );

        let shell_bg = match self.controller.settings.window_backdrop {
            WindowBackdropPreference::Opaque => colors.background,
            WindowBackdropPreference::GaussianBlur => with_alpha(colors.background, 0.72),
            WindowBackdropPreference::Mica | WindowBackdropPreference::MicaAlt => {
                with_alpha(colors.background, 0.80)
            }
        };

        let mut shell_col = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .min_w(px(0.0))
            .overflow_hidden()
            .bg(shell_bg)
            .text_color(colors.foreground)
            .child(titlebar::render_titlebar(self, window, cx))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .flex()
                    .flex_row()
                    .child(rail::render_rail(self, window, cx))
                    .child(main_area),
            )
            .child(status_bar::render_status_bar(self, window, cx));

        if let Some(palette) = command_palette::render_command_palette(self, window, cx) {
            shell_col = shell_col.child(palette);
        }
        if let Some(modal) = modals::render_modals(self, window, cx) {
            shell_col = shell_col.child(modal);
        }
        shell_col = shell_col.child(toasts::render_toast_viewport(self, window, cx));

        let root = self
            .attach_workspace_actions(div().size_full().track_focus(&self.focus_handle), cx)
            .child(WindowBorder::new().child(shell_col));
        app_focus_root(root, window, cx)
    }
}

/// Launches the HeroGPUI desktop shell with [`HeroGpuiAssets`] registered,
/// reduced motion & keymap initialized, and [`DataForgeWorkspace`] mounted as root view.
pub fn launch_gpui_app(controller: AppController, ipc_inbox: Arc<IpcInbox>) {
    application()
        .with_assets(HeroGpuiAssets)
        .run(move |cx: &mut App| {
            herogpui::init(cx);
            actions::bind_workspace_keys(cx);
            apply_chrome_theme_and_motion(
                controller.effective_theme_mode(),
                controller.settings.window_backdrop,
                cx,
            );

            let width =
                f32::from(u16::try_from(controller.settings.window.width.max(900)).unwrap_or(1280));
            let height =
                f32::from(u16::try_from(controller.settings.window.height.max(600)).unwrap_or(800));
            let bounds = Bounds::centered(None, size(px(width), px(height)), cx);

            let mut options = TitleBar::window_options();
            options.window_bounds = Some(WindowBounds::Windowed(bounds));
            options.window_min_size = Some(size(px(900.0), px(600.0)));
            options.window_background = gpui_window_backdrop(controller.settings.window_backdrop);
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

const fn gpui_window_backdrop(pref: WindowBackdropPreference) -> WindowBackgroundAppearance {
    match pref {
        WindowBackdropPreference::Opaque => WindowBackgroundAppearance::Opaque,
        WindowBackdropPreference::GaussianBlur => WindowBackgroundAppearance::Blurred,
        WindowBackdropPreference::Mica => WindowBackgroundAppearance::MicaBackdrop,
        WindowBackdropPreference::MicaAlt => WindowBackgroundAppearance::MicaAltBackdrop,
    }
}

fn apply_saved_theme_presets(controller: &mut AppController) {
    if let Some(theme_id) = controller.settings.content_theme_id.as_deref()
        && let Some(base) = ThemeDefinition::from_builtin_id(theme_id)
    {
        controller.content_theme = base;
    }
    if let Some(syntax_id) = controller.settings.syntax_palette_id.as_deref()
        && let Some(syntax_src) = ThemeDefinition::from_builtin_id(syntax_id)
    {
        controller.content_theme = controller.content_theme.with_syntax_from(&syntax_src);
    }
}
