//! Central application controller coordinating state models, layout, theme,
//! keyboard/mouse dispatch, and Direct2D frame rendering.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use dataforge_gfx::Result;
use dataforge_gfx::context::{D2dContext, RenderSurface};
use dataforge_gfx::editor_renderer::{
    AutocompletePopup, CursorMode, EditorCursor, EditorLine, EditorSelection, SqlEditorRenderer,
};
use dataforge_gfx::glyphs::EngineKind;
use dataforge_gfx::grid_renderer::{DataGridRenderer, GridColumn, GridRow, GridSelection};
use dataforge_gfx::palette::{ThemeMode, ThemePalette};
use dataforge_state::connection_window::{
    AttemptToken, ConnectionWindowPhase, ConnectionWindowPhaseMachine, WorkspaceId,
};
use dataforge_state::data_grid::{DataGridLayoutScope, DataGridModel, RowID};
use dataforge_state::library::ConnectionsLibraryModel;
use dataforge_state::tabs::{
    CloseActionResult, RestoredTabMerge, TabConnectionId, TabId, TabItem, TabKind, TabStripModel,
};
use dataforge_state::theme::{ThemeDefinition, ThemeSlot};
use dataforge_windows_platform::{
    AppSettings, EnvironmentKind, SavedTabSnapshot, ScheduledTaskRecord, ThemePreference,
    WORKSPACE_SESSION_SCHEMA_VERSION, WindowsTheme, WorkspaceSessionSnapshot,
    detect_theme_or_default, load_workspace_session, save_workspace_session, storage_paths,
    workspace_session_path,
};
use tablepro_core::column::ResultColumn;
use tablepro_core::meta::{ObjectInfo, PagePosition, SchemaInfo, TableRef};
use tablepro_core::value::Value;

use crate::accessibility::{FocusRegion, KeyCommand};
use crate::controller_types::{QueryHistoryEntry, SavedQueryEntry, SplitPaneMode, VimMode};
use crate::error::AppError;
use crate::i18n::{Locale, Strings};
use crate::services::ServiceHub;
use crate::shell_layout::{ShellHitTarget, ShellLayout, ShellLayoutConfig};
use crate::updater::UpdaterManager;
use crate::views::ai_drawer::{AiDrawerState, render_ai_drawer};
use crate::views::content::{
    StructureColumnRow, render_connecting_view, render_data_grid, render_sql_editor,
    render_structure_view, render_unavailable_view,
};
use crate::views::inspector::{InspectorField, render_inspector};
use crate::views::library::{ConnectionFormDraft, engine_from_driver_id, render_library_view};
use crate::views::rail::{RailConnectionItem, render_rail};
use crate::views::settings::{import_vscode_theme_bytes, render_settings_view};
use crate::views::sidebar::{SidebarRow, SidebarSectionKind, render_sidebar};
use crate::views::status_bar::{StatusBarSnapshot, TableViewMode, render_status_bar};
use crate::views::tabs::render_tab_strip;
use crate::views::toolbar::{ToolbarStatusPill, render_toolbar};

/// Top-level screen mode shown in the content area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenMode {
    #[default]
    Workspace,
    ConnectionsLibrary,
    Settings,
}

/// Complete headless-testable and GPU-renderable application controller.
pub struct AppController {
    pub settings: AppSettings,
    pub locale: Locale,
    pub strings: Strings,
    pub system_theme: WindowsTheme,
    pub content_theme: ThemeDefinition,
    pub last_theme_import: Option<String>,
    pub screen_mode: ScreenMode,
    pub layout_config: ShellLayoutConfig,
    pub focus_region: FocusRegion,
    pub connections: ConnectionWindowPhaseMachine,
    pub active_workspace: WorkspaceId,
    pub rail_items: Vec<RailConnectionItem>,
    pub tabs: TabStripModel,
    pub library: ConnectionsLibraryModel,
    pub connection_form: ConnectionFormDraft,
    pub sidebar_filter: String,
    pub sidebar_favorites_only: bool,
    pub sidebar_rows: Vec<SidebarRow>,
    pub toolbar_status: ToolbarStatusPill,
    pub status_bar: StatusBarSnapshot,
    pub inspector_fields: Vec<InspectorField>,
    pub ai_drawer: AiDrawerState,
    pub grid_columns: Vec<GridColumn>,
    pub grid_rows: Vec<GridRow>,
    pub grid_selection: GridSelection,
    pub structure_columns: Vec<StructureColumnRow>,
    pub editor_lines: Vec<EditorLine>,
    pub editor_cursors: Vec<EditorCursor>,
    pub editor_selections: Vec<EditorSelection>,
    pub editor_cursor_mode: CursorMode,
    pub editor_autocomplete: Option<AutocompletePopup>,
    pub open_transactions: BTreeMap<WorkspaceId, bool>,
    pub safe_mode_enabled: bool,
    pub services: Option<Arc<ServiceHub>>,
    pub headless: bool,
    pub active_attempt: Option<AttemptToken>,
    pub active_table: Option<TableRef>,
    pub grid_model: DataGridModel,
    pub grid_raw_columns: Vec<ResultColumn>,
    pub grid_raw_rows: Vec<Vec<Value>>,
    pub grid_row_keys: BTreeMap<RowID, Vec<Value>>,
    pub grid_page_prev: Option<PagePosition>,
    pub grid_page_next: Option<PagePosition>,
    pub cached_schemas: Vec<SchemaInfo>,
    pub cached_objects: Vec<ObjectInfo>,
    pub favorite_objects: BTreeSet<String>,
    pub query_history: Vec<QueryHistoryEntry>,
    pub saved_queries: Vec<SavedQueryEntry>,
    pub query_notices: Vec<String>,
    pub vim_mode: Option<VimMode>,
    pub split_pane: SplitPaneMode,
    pub split_editor_lines: Vec<EditorLine>,
    pub last_test_status: Option<String>,
    pub last_export_status: Option<String>,
    pub scheduled_tasks: Vec<ScheduledTaskRecord>,
    pub last_explain_nodes: Vec<dataforge_services::ExplainNode>,
    pub grid_pinned_columns: BTreeSet<String>,
    pub restoring_connection_id: Option<String>,
    pub last_width_dip: f32,
    pub last_height_dip: f32,
    pub updater: UpdaterManager,
    grid_renderer: DataGridRenderer,
    editor_renderer: SqlEditorRenderer,
}

impl AppController {
    /// Creates a new [`AppController`] initialized with `settings` and `system_theme`.
    #[must_use]
    pub fn new(settings: AppSettings, system_theme: WindowsTheme, locale: Locale) -> Self {
        let mut connections = ConnectionWindowPhaseMachine::new();
        let active_workspace = WorkspaceId(1);
        connections.add_workspace(active_workspace);
        connections.select_workspace(active_workspace);

        let strings = Strings::for_locale(locale);
        let mut tabs = TabStripModel::new();
        let conn_id = TabConnectionId("default".to_owned());
        tabs.open_query_tab(conn_id, Some(&format!("{} 1", strings.new_query_tab)));

        let content_theme = if system_theme.is_dark() {
            ThemeDefinition::builtin_dark()
        } else {
            ThemeDefinition::builtin_light()
        };

        let mut controller = Self {
            locale,
            strings,
            system_theme,
            content_theme,
            last_theme_import: None,
            screen_mode: ScreenMode::Workspace,
            layout_config: ShellLayoutConfig {
                tab_count: tabs.tabs().len(),
                ..ShellLayoutConfig::default()
            },
            focus_region: FocusRegion::ContentArea,
            connections,
            active_workspace,
            rail_items: Vec::new(),
            tabs,
            library: ConnectionsLibraryModel::new(),
            connection_form: ConnectionFormDraft::default(),
            sidebar_filter: String::new(),
            sidebar_favorites_only: false,
            sidebar_rows: Vec::new(),
            toolbar_status: ToolbarStatusPill::default(),
            status_bar: StatusBarSnapshot::default(),
            inspector_fields: Vec::new(),
            ai_drawer: AiDrawerState::default(),
            grid_columns: Vec::new(),
            grid_rows: Vec::new(),
            grid_selection: GridSelection::default(),
            structure_columns: Vec::new(),
            editor_lines: vec![EditorLine::new("", 1)],
            editor_cursors: vec![EditorCursor {
                line: 0,
                col: 0,
                is_primary: true,
            }],
            editor_selections: Vec::new(),
            editor_cursor_mode: CursorMode::Bar,
            editor_autocomplete: None,
            open_transactions: BTreeMap::new(),
            safe_mode_enabled: false,
            services: ServiceHub::new(Some(2)).ok().map(Arc::new),
            headless: true,
            active_attempt: None,
            active_table: None,
            grid_model: DataGridModel::new(DataGridLayoutScope::Ephemeral, 100),
            grid_raw_columns: Vec::new(),
            grid_raw_rows: Vec::new(),
            grid_row_keys: BTreeMap::new(),
            grid_page_prev: None,
            grid_page_next: None,
            cached_schemas: Vec::new(),
            cached_objects: Vec::new(),
            favorite_objects: BTreeSet::new(),
            query_history: Vec::new(),
            saved_queries: Vec::new(),
            query_notices: Vec::new(),
            vim_mode: None,
            split_pane: SplitPaneMode::Single,
            split_editor_lines: Vec::new(),
            last_test_status: None,
            last_export_status: None,
            scheduled_tasks: Vec::new(),
            last_explain_nodes: Vec::new(),
            grid_pinned_columns: BTreeSet::new(),
            restoring_connection_id: None,
            last_width_dip: 1280.0,
            last_height_dip: 800.0,
            updater: UpdaterManager::new(),
            grid_renderer: DataGridRenderer::new(settings.value_font.family.clone()),
            editor_renderer: SqlEditorRenderer::new(settings.editor_font.family.clone()),
            settings,
        };
        controller.sync_layout_counts();
        controller
    }

    /// Creates a controller using live Windows system theme and saved/environment locale detection.
    #[must_use]
    pub fn with_detected_theme(settings: AppSettings) -> Self {
        let system_theme = detect_theme_or_default();
        let locale = crate::i18n::init_locale(settings.locale.as_deref());
        Self::new(settings, system_theme, locale)
    }

    /// Switches the active UI locale, persists the BCP-47 tag in settings, and refreshes localized models.
    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
        self.strings = Strings::for_locale(locale);
        self.settings.locale = Some(locale.code().to_owned());
        self.rebuild_sidebar_rows();
    }

    /// Resolves the effective [`ThemeMode`] from user preference and live OS theme.
    #[must_use]
    pub const fn effective_theme_mode(&self) -> ThemeMode {
        match self.settings.theme {
            ThemePreference::Light => ThemeMode::Light,
            ThemePreference::Dark => ThemeMode::Dark,
            ThemePreference::HighContrast => ThemeMode::HighContrastDark,
            ThemePreference::System => match self.system_theme {
                WindowsTheme::Light => ThemeMode::Light,
                WindowsTheme::Dark => ThemeMode::Dark,
                WindowsTheme::HighContrast => ThemeMode::HighContrastDark,
            },
        }
    }

    /// Returns `true` when the Win32 DWM titlebar should use dark mode.
    #[must_use]
    pub const fn uses_dark_titlebar(&self) -> bool {
        matches!(self.effective_theme_mode(), ThemeMode::Dark)
    }

    /// Resolves the active [`ThemePalette`] combining chrome tokens and content overrides.
    #[must_use]
    pub fn resolve_palette(&self) -> ThemePalette {
        let mut palette = ThemePalette::for_mode(self.effective_theme_mode());
        if let Some(bg) = self.content_theme.colors.get(&ThemeSlot::EditorBackground) {
            palette.content.bg = dataforge_gfx::palette::Color::from_rgba_hex(
                ((u32::from(bg.r)) << 24)
                    | ((u32::from(bg.g)) << 16)
                    | ((u32::from(bg.b)) << 8)
                    | u32::from(bg.a),
            );
        }
        palette
    }

    /// Updates the live system theme (called on `WM_SETTINGCHANGE` / `WM_THEMECHANGED`).
    pub fn on_system_theme_changed(&mut self, system_theme: WindowsTheme) {
        self.system_theme = system_theme;
        if self.content_theme.is_builtin {
            self.content_theme = if self.uses_dark_titlebar() {
                ThemeDefinition::builtin_dark()
            } else {
                ThemeDefinition::builtin_light()
            };
        }
    }

    /// Imports a VS Code or DataForge theme JSON payload.
    pub fn import_vscode_theme(&mut self, bytes: &[u8]) -> std::result::Result<(), String> {
        let (def, outcome) = import_vscode_theme_bytes(bytes)?;
        self.content_theme = def;
        self.last_theme_import = Some(
            self.strings
                .theme_slots_count_tpl
                .replace("{name}", &outcome.theme_name)
                .replace("{count}", &outcome.slot_count.to_string()),
        );
        Ok(())
    }

    /// Handles targets forwarded from CLI or Named Pipe IPC (`.sqlite`, `.duckdb`, URLs).
    pub fn open_forwarded_targets(&mut self, targets: &[String]) {
        for target in targets {
            let trimmed = target.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(conn) = self.parse_and_apply_connection_url(trimmed) {
                let engine = engine_from_driver_id(&conn.driver_id);
                let label = conn.name.clone();
                self.library.upsert_connection(conn);
                self.add_rail_connection(trimmed, &label, engine, true);
                if matches!(engine, EngineKind::Sqlite | EngineKind::Duckdb) {
                    let path = std::path::Path::new(&self.connection_form.database).to_path_buf();
                    if path.exists() {
                        let _ = self.connect_local_file_blocking(&path);
                    }
                }
            } else {
                let path = std::path::Path::new(trimmed);
                let engine = if trimmed.to_ascii_lowercase().ends_with(".duckdb") {
                    EngineKind::Duckdb
                } else {
                    EngineKind::Sqlite
                };
                if path.exists() {
                    let _ = self.connect_local_file_blocking(path);
                } else {
                    self.add_rail_connection(trimmed, trimmed, engine, true);
                }
            }
            self.screen_mode = ScreenMode::Workspace;
        }
        self.sync_layout_counts();
    }

    /// Adds or activates an open connection in the Connection Rail.
    pub fn add_rail_connection(
        &mut self,
        id: &str,
        label: &str,
        engine: EngineKind,
        connected: bool,
    ) {
        for item in &mut self.rail_items {
            item.active = false;
        }
        self.rail_items.push(RailConnectionItem {
            id: id.to_owned(),
            label: label.to_owned(),
            engine,
            active: true,
            connected,
        });
        self.toolbar_status.engine_version = engine.display_name().to_owned();
        self.toolbar_status.database = label.to_owned();
        self.sync_layout_counts();
    }

    /// Computes the current [`ShellLayout`] for `width x height` DIPs.
    #[must_use]
    pub fn compute_layout(&mut self, width_dip: f32, height_dip: f32) -> ShellLayout {
        self.last_width_dip = width_dip;
        self.last_height_dip = height_dip;
        self.sync_layout_counts();
        ShellLayout::compute(width_dip, height_dip, self.layout_config)
    }

    /// Returns `true` if closing the window requires a Safe Mode or open-transaction prompt.
    #[must_use]
    pub fn needs_close_guard(&self) -> bool {
        let has_open_tx = self.open_transactions.values().any(|open| *open);
        let has_unsaved = self.grid_model.edits.has_pending_edits()
            || self.tabs.tabs().iter().any(|t| t.has_unsaved_edits);
        has_open_tx || (self.safe_mode_enabled && has_unsaved)
    }

    /// Executes `Ctrl+W` (`CloseTabOrWindow`): closes the active tab first, or requests window close.
    pub fn close_frontmost_tab_or_window(&mut self) -> CloseActionResult {
        let res = self.tabs.perform_close();
        self.sync_layout_counts();
        res
    }

    /// Executes `Ctrl+Shift+T` (`ReopenClosedTab`).
    pub fn reopen_closed_tab(&mut self) -> Option<TabId> {
        let reopened = self.tabs.reopen_closed_tab();
        self.sync_layout_counts();
        reopened
    }

    /// Dispatches a keyboard command; returns `true` if the window should close.
    pub fn handle_key_command(&mut self, cmd: KeyCommand) -> bool {
        match cmd {
            KeyCommand::FocusNextRegion => {
                self.focus_region = self.focus_region.step(
                    false,
                    self.layout_config.sidebar_visible,
                    self.layout_config.inspector_visible,
                    self.layout_config.ai_drawer_visible,
                );
            }
            KeyCommand::FocusPrevRegion => {
                self.focus_region = self.focus_region.step(
                    true,
                    self.layout_config.sidebar_visible,
                    self.layout_config.inspector_visible,
                    self.layout_config.ai_drawer_visible,
                );
            }
            KeyCommand::NextTab => self.cycle_tab(1),
            KeyCommand::PrevTab => self.cycle_tab(-1),
            KeyCommand::CloseTabOrWindow => {
                return matches!(
                    self.close_frontmost_tab_or_window(),
                    CloseActionResult::CloseWindow
                );
            }
            KeyCommand::ReopenClosedTab => {
                let _ = self.reopen_closed_tab();
            }
            KeyCommand::NewQueryTab => {
                let n = self.tabs.tabs().len() + 1;
                let conn_id = TabConnectionId("default".to_owned());
                let title = format!("{} {n}", self.strings.new_query_tab);
                self.tabs.open_query_tab(conn_id, Some(&title));
                self.screen_mode = ScreenMode::Workspace;
                self.sync_layout_counts();
            }
            KeyCommand::ToggleSidebar => {
                self.layout_config.sidebar_visible = !self.layout_config.sidebar_visible;
            }
            KeyCommand::ToggleInspector => {
                self.layout_config.inspector_visible = !self.layout_config.inspector_visible;
                self.toolbar_status.inspector_open = self.layout_config.inspector_visible;
            }
            KeyCommand::ToggleAiDrawer => {
                self.layout_config.ai_drawer_visible = !self.layout_config.ai_drawer_visible;
                self.toolbar_status.ai_open = self.layout_config.ai_drawer_visible;
            }
            KeyCommand::FocusFilter => {
                self.layout_config.sidebar_visible = true;
                self.focus_region = FocusRegion::ObjectSidebar;
            }
            KeyCommand::OpenSettings => {
                self.screen_mode = ScreenMode::Settings;
            }
            KeyCommand::OpenConnectionsLibrary => {
                self.screen_mode = ScreenMode::ConnectionsLibrary;
            }
            KeyCommand::ExecuteQuery => {
                let _ = self.execute_active_sql_blocking();
            }
        }
        false
    }

    /// Handles a left-click at `(px, py)` DIPs and returns the hit target.
    pub fn handle_click(&mut self, px: f32, py: f32) -> ShellHitTarget {
        let layout = self.compute_layout(self.last_width_dip, self.last_height_dip);
        let target = layout.hit_test(px, py);
        match target {
            ShellHitTarget::RailLibraryButton => {
                self.screen_mode = if self.screen_mode == ScreenMode::ConnectionsLibrary {
                    ScreenMode::Workspace
                } else {
                    ScreenMode::ConnectionsLibrary
                };
            }
            ShellHitTarget::RailCollapseButton => {
                self.layout_config.rail_expanded = !self.layout_config.rail_expanded;
            }
            ShellHitTarget::RailSettingsButton | ShellHitTarget::StatusSettingsButton => {
                self.screen_mode = if self.screen_mode == ScreenMode::Settings {
                    ScreenMode::Workspace
                } else {
                    ScreenMode::Settings
                };
            }
            ShellHitTarget::RailConnection(idx) => {
                for (i, item) in self.rail_items.iter_mut().enumerate() {
                    item.active = i == idx;
                }
                self.screen_mode = ScreenMode::Workspace;
            }
            ShellHitTarget::SidebarFavoritesToggle => {
                self.sidebar_favorites_only = !self.sidebar_favorites_only;
            }
            ShellHitTarget::ToolbarInspectorButton => {
                let _ = self.handle_key_command(KeyCommand::ToggleInspector);
            }
            ShellHitTarget::ToolbarAiButton => {
                let _ = self.handle_key_command(KeyCommand::ToggleAiDrawer);
            }
            ShellHitTarget::Tab(idx) => {
                if let Some(tab) = self.tabs.tabs().get(idx) {
                    let id = tab.id;
                    self.tabs.select(id);
                    self.screen_mode = ScreenMode::Workspace;
                }
            }
            ShellHitTarget::TabClose(idx) => {
                if let Some(tab) = self.tabs.tabs().get(idx) {
                    let id = tab.id;
                    let _ = self.tabs.close_tab_by_user(id);
                    self.sync_layout_counts();
                }
            }
            ShellHitTarget::TabNewButton => {
                let _ = self.handle_key_command(KeyCommand::NewQueryTab);
            }
            ShellHitTarget::StatusDataToggle => {
                self.status_bar.view_mode = TableViewMode::Data;
            }
            ShellHitTarget::StatusStructureToggle => {
                self.status_bar.view_mode = TableViewMode::Structure;
                if let Some(table) = self.active_table.clone() {
                    let _ = self.load_table_structure_blocking(&table);
                }
            }
            ShellHitTarget::StatusPrevPage if self.status_bar.current_page > 1 => {
                if self.active_table.is_some() {
                    let _ = self.prev_grid_page_blocking();
                } else {
                    self.status_bar.current_page -= 1;
                }
            }
            ShellHitTarget::StatusNextPage
                if self.status_bar.current_page < self.status_bar.total_pages =>
            {
                if self.active_table.is_some() {
                    let _ = self.next_grid_page_blocking();
                } else {
                    self.status_bar.current_page += 1;
                }
            }
            _ => {}
        }
        target
    }

    /// Renders the complete DataForge shell into `surface`.
    pub fn render_frame(&mut self, ctx: &D2dContext, surface: &mut RenderSurface) -> Result<()> {
        let width = surface.width_dip();
        let height = surface.height_dip();
        let layout = self.compute_layout(width, height);
        let palette = self.resolve_palette();

        surface.begin_draw();
        surface.clear(palette.content.bg);

        render_rail(
            ctx,
            surface,
            &layout,
            &palette,
            &self.rail_items,
            self.layout_config.rail_expanded,
            self.screen_mode == ScreenMode::ConnectionsLibrary,
        )?;
        render_sidebar(
            surface,
            &layout,
            &palette,
            &self.sidebar_filter,
            self.strings.filter_placeholder,
            self.sidebar_favorites_only,
            &self.sidebar_rows,
        )?;
        let has_pending_edits = self.grid_model.edits.has_pending_edits()
            || self.tabs.tabs().iter().any(|t| t.has_unsaved_edits);
        render_toolbar(
            surface,
            &layout,
            &palette,
            &self.toolbar_status,
            has_pending_edits,
        )?;
        render_tab_strip(
            surface,
            &layout,
            &palette,
            self.tabs.tabs(),
            self.tabs.active_tab(),
        )?;
        self.render_content_area(surface, &layout, &palette)?;
        render_inspector(
            surface,
            &layout,
            &palette,
            self.strings.row_inspector_title,
            self.strings.no_selection,
            &self.inspector_fields,
        )?;
        render_ai_drawer(
            surface,
            &layout,
            &palette,
            self.strings.ai_assistant_title,
            &self.ai_drawer,
        )?;
        render_status_bar(surface, &layout, &palette, &self.status_bar)?;

        surface.end_draw()
    }

    fn render_content_area(
        &mut self,
        surface: &mut RenderSurface,
        layout: &ShellLayout,
        palette: &ThemePalette,
    ) -> Result<()> {
        match self.screen_mode {
            ScreenMode::ConnectionsLibrary => {
                return render_library_view(
                    surface,
                    layout,
                    palette,
                    &self.library,
                    &self.connection_form,
                );
            }
            ScreenMode::Settings => {
                return render_settings_view(
                    surface,
                    layout,
                    palette,
                    &self.settings,
                    &self.content_theme.name,
                    self.last_theme_import.as_deref(),
                );
            }
            ScreenMode::Workspace => {}
        }

        if let Some(phase) = self.connections.phase(self.active_workspace) {
            match phase {
                ConnectionWindowPhase::Unavailable(unavail) => {
                    return render_unavailable_view(
                        surface,
                        layout,
                        palette,
                        unavail,
                        self.strings.retry_connection,
                    );
                }
                ConnectionWindowPhase::Connecting { .. }
                | ConnectionWindowPhase::Reconnecting { .. } => {
                    return render_connecting_view(
                        surface,
                        layout,
                        palette,
                        &self.toolbar_status.database,
                    );
                }
                ConnectionWindowPhase::Disconnected | ConnectionWindowPhase::Connected => {}
            }
        }

        let active_kind = self
            .tabs
            .active_tab()
            .and_then(|id| self.tabs.tabs().iter().find(|t| t.id == id))
            .map(|t| t.kind.clone());
        match active_kind.as_ref() {
            Some(TabKind::TableData { table_name }) => {
                if self.status_bar.view_mode == TableViewMode::Structure {
                    render_structure_view(
                        surface,
                        layout,
                        palette,
                        table_name,
                        &self.structure_columns,
                    )
                } else {
                    render_data_grid(
                        surface,
                        layout,
                        palette,
                        &self.grid_renderer,
                        &self.grid_columns,
                        &self.grid_rows,
                        &self.grid_selection,
                        0,
                    )
                }
            }
            Some(TabKind::Structure { table_name }) => render_structure_view(
                surface,
                layout,
                palette,
                table_name,
                &self.structure_columns,
            ),
            Some(TabKind::QueryEditor { .. }) | None => render_sql_editor(
                surface,
                layout,
                palette,
                &mut self.editor_renderer,
                &self.editor_lines,
                &self.editor_cursors,
                &self.editor_selections,
                self.editor_cursor_mode,
                self.editor_autocomplete.as_ref(),
            ),
        }
    }

    fn cycle_tab(&mut self, delta: isize) {
        let tabs = self.tabs.tabs();
        if tabs.is_empty() {
            return;
        }
        let current = self
            .tabs
            .active_tab()
            .and_then(|id| tabs.iter().position(|t| t.id == id))
            .unwrap_or(0);
        let len = tabs.len() as isize;
        let next = (current as isize + delta).rem_euclid(len) as usize;
        if let Some(tab) = tabs.get(next) {
            let id = tab.id;
            self.tabs.select(id);
        }
    }

    pub(crate) fn sync_layout_counts(&mut self) {
        self.layout_config.open_connection_count = self.rail_items.len();
        self.layout_config.tab_count = self.tabs.tabs().len();
        self.layout_config.sidebar_item_count = self.sidebar_rows.len();
    }

    /// Marks an active workspace as connecting with a new `AttemptToken`.
    pub fn begin_workspace_connect(&mut self, id: WorkspaceId) -> Option<AttemptToken> {
        self.connections.begin_connect(id)
    }

    /// Returns the initialized [`ServiceHub`] or [`AppError::InvalidState`].
    pub fn require_services(&self) -> std::result::Result<&Arc<ServiceHub>, AppError> {
        self.services
            .as_ref()
            .ok_or_else(|| AppError::InvalidState("service hub is not initialized".to_owned()))
    }

    /// Triggers the one-time startup auto-update check in non-headless GUI sessions.
    pub fn trigger_auto_update_check(&mut self) -> bool {
        if self.headless || !self.settings.auto_check_updates {
            return false;
        }
        self.updater.trigger_auto_check_once()
    }

    /// Starts an asynchronous update check against GitHub Releases.
    pub fn check_for_updates(&mut self) {
        self.updater.check_for_updates();
    }

    /// Starts downloading the currently available update package.
    pub fn download_update(&mut self) {
        self.updater.download_update();
    }

    /// Applies the staged update package and restarts the application.
    pub fn apply_update_and_restart(&mut self) -> std::result::Result<(), String> {
        self.updater.apply_and_restart()
    }

    /// Returns the active saved library or rail connection ID, if any.
    #[must_use]
    pub(crate) fn active_connection_id(&self) -> Option<&str> {
        if let Some(restoring) = self.restoring_connection_id.as_deref() {
            return Some(restoring);
        }
        if !self.connection_form.name.is_empty()
            && self.rail_items.iter().any(|r| r.active)
            && let Some(conn) = self
                .library
                .storage()
                .connections
                .values()
                .find(|c| c.name == self.connection_form.name)
        {
            return Some(conn.id.0.as_str());
        }
        self.rail_items
            .iter()
            .find(|r| r.active)
            .map(|r| r.id.as_str())
    }

    /// Returns the deployment [`EnvironmentKind`] tag for `conn_id`, defaulting to `Local`.
    #[must_use]
    pub fn connection_environment(&self, conn_id: &str) -> EnvironmentKind {
        self.settings
            .environment_tags
            .get(conn_id)
            .copied()
            .unwrap_or_default()
    }

    /// Assigns the deployment [`EnvironmentKind`] tag for `conn_id` and persists [`AppSettings`].
    pub fn set_connection_environment(&mut self, conn_id: &str, env: EnvironmentKind) {
        self.settings
            .environment_tags
            .insert(conn_id.to_owned(), env);
        if !self.headless
            && let Ok(paths) = storage_paths()
        {
            let _ = self.settings.save_atomic(&AppSettings::file_path(&paths));
        }
    }

    /// Builds a [`WorkspaceSessionSnapshot`] reflecting the current tabs, selection, and layout.
    #[must_use]
    pub fn build_default_session_snapshot(
        &self,
        sidebar_open: bool,
        inspector_open: bool,
        bottom_drawer_open: bool,
    ) -> WorkspaceSessionSnapshot {
        let active_id = self.tabs.active_tab();
        let active_tab_index = active_id
            .and_then(|id| self.tabs.tabs().iter().position(|t| t.id == id))
            .unwrap_or(0);
        let active_conn = self.active_connection_id().map(str::to_owned);
        let active_database = (!self.toolbar_status.database.is_empty())
            .then(|| self.toolbar_status.database.clone());
        let active_editor_sql = self.editor_sql_text();

        let tabs = self
            .tabs
            .tabs()
            .iter()
            .map(|tab| {
                snapshot_tab_from_item(
                    tab,
                    active_id == Some(tab.id),
                    &active_editor_sql,
                    active_conn.as_deref(),
                    active_database.as_deref(),
                )
            })
            .collect();

        let saved_at_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));

        WorkspaceSessionSnapshot {
            version: WORKSPACE_SESSION_SCHEMA_VERSION,
            active_connection_id: active_conn,
            active_database,
            selected_table: self
                .active_table
                .as_ref()
                .map(|t| (t.schema.clone(), t.name.clone())),
            active_tab_index,
            tabs,
            sidebar_open,
            sidebar_width: Some(self.layout_config.sidebar_width),
            inspector_open,
            bottom_drawer_open,
            saved_at_unix,
        }
    }

    /// Persists `snapshot` to the default workspace session path (no-op when headless).
    pub fn save_workspace_session_snapshot(&self, snapshot: &WorkspaceSessionSnapshot) {
        if self.headless {
            return;
        }
        let _ = save_workspace_session(None, snapshot);
    }

    /// Persists `snapshot` atomically to `path`.
    pub fn save_workspace_session_from_path(
        &self,
        path: &std::path::Path,
        snapshot: &WorkspaceSessionSnapshot,
    ) {
        let _ = save_workspace_session(Some(path), snapshot);
    }

    /// Loads and restores a [`WorkspaceSessionSnapshot`] from `path`, optionally reconnecting.
    pub fn restore_workspace_session_from_path(
        &mut self,
        path: &std::path::Path,
        trigger_reconnect: bool,
    ) -> Option<WorkspaceSessionSnapshot> {
        let snapshot = load_workspace_session(Some(path)).ok().flatten()?;
        self.apply_restored_session_snapshot(&snapshot, trigger_reconnect);
        Some(snapshot)
    }

    /// Restores the saved workspace session at startup when `restore_last_session` is enabled.
    pub fn restore_workspace_session_on_startup(&mut self) -> Option<WorkspaceSessionSnapshot> {
        if self.headless || !self.settings.restore_last_session {
            return None;
        }
        let paths = storage_paths().ok()?;
        let session_path = workspace_session_path(&paths);
        self.restore_workspace_session_from_path(&session_path, true)
    }

    fn apply_restored_session_snapshot(
        &mut self,
        snapshot: &WorkspaceSessionSnapshot,
        trigger_reconnect: bool,
    ) {
        if !snapshot.tabs.is_empty() {
            let items: Vec<TabItem> = snapshot
                .tabs
                .iter()
                .enumerate()
                .map(|(idx, saved)| restored_tab_item(saved, idx))
                .collect();
            self.tabs = TabStripModel::new();
            RestoredTabMerge::merge(&mut self.tabs, items);
            if let Some(target_id) = self
                .tabs
                .tabs()
                .get(snapshot.active_tab_index)
                .or_else(|| self.tabs.tabs().first())
                .map(|t| t.id)
            {
                self.tabs.select(target_id);
            }
        }

        if let Some(active_tab_sql) = self
            .tabs
            .active_tab()
            .and_then(|id| self.tabs.tabs().iter().find(|t| t.id == id))
            .and_then(|t| match &t.kind {
                TabKind::QueryEditor { sql } => Some(sql.clone()),
                TabKind::TableData { .. } | TabKind::Structure { .. } => None,
            })
        {
            self.set_editor_sql(&active_tab_sql);
        }

        self.active_table = snapshot
            .selected_table
            .as_ref()
            .map(|(schema, name)| TableRef::new(schema.clone(), name.clone()));
        self.layout_config.sidebar_visible = snapshot.sidebar_open;
        if let Some(width) = snapshot.sidebar_width {
            self.layout_config.sidebar_width = width;
        }
        self.layout_config.inspector_visible = snapshot.inspector_open;
        self.sync_layout_counts();

        if trigger_reconnect
            && !self.headless
            && let Some(conn_id) = snapshot.active_connection_id.as_deref()
        {
            self.restoring_connection_id = Some(conn_id.to_owned());
            if self.reconnect_saved_connection_async(conn_id).is_err() {
                self.restoring_connection_id = None;
            }
        }
    }
}

fn snapshot_tab_from_item(
    tab: &TabItem,
    is_active: bool,
    active_editor_sql: &str,
    active_conn: Option<&str>,
    active_database: Option<&str>,
) -> SavedTabSnapshot {
    let (kind, table, sql_draft) = match &tab.kind {
        TabKind::QueryEditor { sql } => {
            let draft = if is_active {
                active_editor_sql.to_owned()
            } else {
                sql.clone()
            };
            ("query".to_owned(), None, draft)
        }
        TabKind::TableData { table_name } => {
            ("table".to_owned(), Some(table_name.clone()), String::new())
        }
        TabKind::Structure { table_name } => (
            "structure".to_owned(),
            Some(table_name.clone()),
            String::new(),
        ),
    };
    let connection_id = if tab.connection_id.0 == "default" {
        active_conn.map(str::to_owned)
    } else {
        Some(tab.connection_id.0.clone())
    };
    SavedTabSnapshot {
        tab_id: tab.id.0.to_string(),
        title: tab.title.clone(),
        kind,
        connection_id,
        database: active_database.map(str::to_owned),
        schema: None,
        table,
        sql_draft,
        filter_clause: tab.has_active_filters.then(String::new),
        sort_clause: tab.has_active_sort.then(String::new),
        is_pinned: tab.pinned,
    }
}

fn restored_tab_item(saved: &SavedTabSnapshot, index: usize) -> TabItem {
    let table_name = saved.table.clone().unwrap_or_else(|| saved.title.clone());
    let kind = match saved.kind.as_str() {
        "table" => TabKind::TableData { table_name },
        "structure" => TabKind::Structure { table_name },
        _ => TabKind::QueryEditor {
            sql: saved.sql_draft.clone(),
        },
    };
    let conn = saved
        .connection_id
        .clone()
        .unwrap_or_else(|| "default".to_owned());
    let numeric_id = saved
        .tab_id
        .parse::<u64>()
        .unwrap_or_else(|_| u64::try_from(index + 1).unwrap_or(1));
    TabItem {
        id: TabId(numeric_id),
        connection_id: TabConnectionId(conn),
        title: saved.title.clone(),
        kind,
        pinned: saved.is_pinned,
        has_unsaved_edits: false,
        has_active_filters: saved.filter_clause.is_some(),
        has_active_sort: saved.sort_clause.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_tag_roundtrip_and_default() -> std::result::Result<(), Box<dyn std::error::Error>>
    {
        let mut controller = AppController::with_detected_theme(AppSettings::default());
        assert_eq!(
            controller.connection_environment("conn-1"),
            EnvironmentKind::Local
        );
        controller.set_connection_environment("conn-1", EnvironmentKind::Production);
        assert_eq!(
            controller.connection_environment("conn-1"),
            EnvironmentKind::Production
        );

        let temp_dir = tempfile::tempdir()?;
        let settings_path = temp_dir.path().join("settings.json");
        controller.settings.save_atomic(&settings_path)?;
        let reloaded = AppSettings::load(&settings_path)?;
        let restored = AppController::with_detected_theme(reloaded);
        assert_eq!(
            restored.connection_environment("conn-1"),
            EnvironmentKind::Production
        );
        Ok(())
    }

    #[test]
    fn workspace_session_snapshot_save_and_restore_roundtrip()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempfile::tempdir()?;
        let session_path = temp_dir.path().join("workspace_session.json");

        let mut writer = AppController::with_detected_theme(AppSettings::default());
        writer.set_editor_sql("SELECT id, email FROM users;");
        writer.active_table = Some(TableRef::new(Some("public".to_owned()), "users".to_owned()));
        writer
            .tabs
            .open_table_tab(TabConnectionId("c1".to_owned()), "users");
        writer.tabs.select(TabId(1));

        let snapshot = writer.build_default_session_snapshot(false, true, true);
        writer.save_workspace_session_from_path(&session_path, &snapshot);

        let mut reader = AppController::with_detected_theme(AppSettings::default());
        let restored = reader
            .restore_workspace_session_from_path(&session_path, false)
            .ok_or("expected restored workspace session")?;
        assert_eq!(restored.tabs.len(), 2);
        assert!(!reader.layout_config.sidebar_visible);
        assert!(reader.layout_config.inspector_visible);
        assert_eq!(
            reader.active_table,
            Some(TableRef::new(Some("public".to_owned()), "users".to_owned()))
        );
        assert_eq!(reader.editor_sql_text(), "SELECT id, email FROM users;");
        Ok(())
    }
}

/// Helper to build a sample [`SidebarSectionKind`] header row.
#[must_use]
pub fn make_sidebar_section(kind: SidebarSectionKind, title: &str, count: usize) -> SidebarRow {
    SidebarRow::SectionHeader {
        kind,
        title: title.to_owned(),
        expanded: true,
        count,
    }
}
