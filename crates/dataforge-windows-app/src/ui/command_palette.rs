//! Keyboard-driven HeroGPUI `CommandPalette` overlay (`Ctrl+K`) backed by real
//! workspace actions, open tabs, saved connections, catalog objects, and queries.

use herogpui::components::{CommandItem, CommandPalette, IconName};
use herogpui::gpui::{AnyElement, Context, IntoElement, SharedString, Window, px};

use crate::accessibility::KeyCommand;
use crate::app::{AppController, ScreenMode};
use crate::i18n::Strings;
use crate::views::status_bar::TableViewMode;
use dataforge_state::connection_window::ConnectionWindowPhase;
use dataforge_state::tabs::{TabConnectionId, TabId, TabKind};
use tablepro_core::meta::{ObjectKind, TableRef};

use super::DataForgeWorkspace;
use super::shell_state::{ResultPaneSubView, ToastSeverity, WorkspaceSubView, WorkspaceToolView};

/// Persistent state for the workspace command palette (`Ctrl+K`).
#[derive(Debug, Default)]
pub struct CommandPaletteState {
    pub open: bool,
    pub query: String,
    pub selected_index: usize,
}

impl CommandPaletteState {
    /// Initializes command palette state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) -> Self {
        Self::default()
    }

    /// Opens the command palette and resets selection.
    pub fn open_palette(&mut self, _window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) {
        self.open = true;
        self.selected_index = 0;
    }

    /// Toggles command palette visibility.
    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<DataForgeWorkspace>) {
        if self.open {
            self.close();
        } else {
            self.open_palette(window, cx);
        }
    }

    /// Closes the command palette.
    pub fn close(&mut self) {
        self.open = false;
    }

    /// Returns whether the command palette overlay is currently open.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }
}

/// Renders the HeroGPUI `CommandPalette` overlay, staying mounted across exit transitions
/// so `150ms` enter / `100ms` exit animations and focus restoration run cleanly.
#[must_use]
pub fn render_command_palette(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> Option<AnyElement> {
    let s = &ws.controller.strings;
    let items = build_command_items(&ws.controller);
    let palette = CommandPalette::new("dataforge-command-palette", items)
        .is_open(ws.command_palette_ui.is_open())
        .placeholder(s.cmd_palette_placeholder)
        .empty_text(s.cmd_palette_empty)
        .label(s.cmd_palette_label)
        .width(px(600.0))
        .max_list_height(px(400.0))
        .close_on_select(true)
        .on_open_change(cx.listener(|ws, open: &bool, _, cx| {
            ws.command_palette_ui.open = *open;
            if *open {
                ws.command_palette_ui.selected_index = 0;
            }
            cx.notify();
        }))
        .on_select(cx.listener(|ws, key: &SharedString, window, cx| {
            execute_palette_item(ws, key.as_ref(), window, cx);
        }));

    Some(palette.into_any_element())
}

fn build_command_items(controller: &AppController) -> Vec<CommandItem> {
    let s = &controller.strings;
    let mut items = Vec::new();
    append_action_items(controller, s, &mut items);
    append_open_tab_items(controller, s, &mut items);
    append_connection_items(controller, s, &mut items);
    append_catalog_items(controller, s, &mut items);
    append_saved_query_items(controller, s, &mut items);
    append_history_items(controller, s, &mut items);
    items
}

fn append_action_items(controller: &AppController, s: &Strings, items: &mut Vec<CommandItem>) {
    let is_connected = matches!(
        controller.connections.phase(controller.active_workspace),
        Some(ConnectionWindowPhase::Connected)
    );
    let has_tabs = !controller.tabs.tabs().is_empty();
    let has_export = !controller.grid_rows.is_empty();
    let edits = &controller.grid_model.edits;
    let pending = edits.modified_cells.len() + edits.inserted_rows.len() + edits.deleted_rows.len();

    append_query_action_items(s, is_connected, has_tabs, has_export, pending, items);
    append_workbench_tool_items(s, is_connected, items);
    append_view_action_items(s, items);
}

fn append_workbench_tool_items(s: &Strings, is_connected: bool, items: &mut Vec<CommandItem>) {
    items.extend([
        CommandItem::new("act:format-sql", s.editor_format_sql)
            .group(s.cmd_group_actions)
            .icon(IconName::Code)
            .keywords(["format", "sql", "pretty", "indent"]),
        CommandItem::new("act:visual-explain", s.explain_visual_tab)
            .group(s.cmd_group_actions)
            .icon(IconName::Activity)
            .keywords(["explain", "plan", "visual", "analyze", "cost"])
            .is_disabled(!is_connected),
        CommandItem::new("act:result-charts", s.charts_tab)
            .group(s.cmd_group_view)
            .icon(IconName::ChartBar)
            .keywords(["chart", "charts", "bar", "line", "donut", "plot"]),
        CommandItem::new("act:er-diagram", s.er_diagram)
            .group(s.cmd_group_navigation)
            .icon(IconName::Share2)
            .keywords(["er", "diagram", "schema", "relations", "foreign key"]),
        CommandItem::new("act:schema-designer", s.schema_designer)
            .group(s.cmd_group_actions)
            .icon(IconName::Layers)
            .keywords(["schema", "designer", "ddl", "create", "alter"]),
        CommandItem::new(
            "act:import-wizard",
            format!("{} & {}", s.import_wizard, s.import_sql_script),
        )
        .group(s.cmd_group_actions)
        .icon(IconName::Upload)
        .keywords(["import", "csv", "json", "sql", "script"]),
        CommandItem::new("act:metadata-search", s.metadata_search)
            .group(s.cmd_group_navigation)
            .icon(IconName::Search)
            .keywords(["metadata", "search", "tables", "columns", "find"]),
        CommandItem::new("act:query-builder", s.query_builder)
            .group(s.cmd_group_actions)
            .icon(IconName::SlidersHorizontal)
            .keywords(["visual", "query", "builder", "select", "join"]),
        CommandItem::new("act:compare", s.compare_title)
            .group(s.cmd_group_actions)
            .icon(IconName::ArrowLeftRight)
            .keywords(["compare", "diff", "schema", "data", "sync"]),
        CommandItem::new(
            "act:admin",
            format!("{} & {}", s.admin_title, s.admin_scheduled_tasks),
        )
        .group(s.cmd_group_navigation)
        .icon(IconName::ShieldCheck)
        .keywords(["admin", "server", "sessions", "users", "scheduled", "tasks"]),
    ]);
}

fn append_query_action_items(
    s: &Strings,
    is_connected: bool,
    has_tabs: bool,
    has_export: bool,
    pending: usize,
    items: &mut Vec<CommandItem>,
) {
    items.extend([
        CommandItem::new("act:run", s.cmd_run_sql)
            .group(s.cmd_group_actions)
            .icon(IconName::Play)
            .shortcut(["Ctrl", "Enter"])
            .keywords(["execute", "query", "sql"])
            .is_disabled(!is_connected),
        CommandItem::new("act:new-tab", s.cmd_new_query_tab)
            .group(s.cmd_group_actions)
            .icon(IconName::FilePlus)
            .shortcut(["Ctrl", "T"])
            .keywords(["editor", "sql", "tab"]),
        CommandItem::new("act:save-sql", s.cmd_save_sql)
            .group(s.cmd_group_actions)
            .icon(IconName::Save)
            .shortcut(["Ctrl", "S"])
            .keywords(["save", "snippet", "query"]),
        CommandItem::new("act:close-tab", s.cmd_close_tab)
            .group(s.cmd_group_actions)
            .icon(IconName::X)
            .shortcut(["Ctrl", "W"])
            .is_disabled(!has_tabs),
        CommandItem::new("act:reopen-tab", s.cmd_reopen_tab)
            .group(s.cmd_group_actions)
            .icon(IconName::Undo2)
            .shortcut(["Ctrl", "Shift", "T"])
            .keywords(["restore", "undo"]),
        CommandItem::new("act:export", s.cmd_export)
            .group(s.cmd_group_actions)
            .icon(IconName::Download)
            .keywords(["csv", "json", "sql", "dump"])
            .is_disabled(!has_export),
        CommandItem::new("act:refresh-schema", s.cmd_refresh_schema)
            .group(s.cmd_group_actions)
            .icon(IconName::RefreshCw)
            .keywords(["reload", "catalog", "tables"])
            .is_disabled(!is_connected),
        CommandItem::new("act:commit", s.cmd_commit_edits)
            .group(s.cmd_group_actions)
            .icon(IconName::Check)
            .keywords(["apply", "save", "transaction"])
            .is_disabled(!is_connected || pending == 0),
        CommandItem::new("act:rollback", s.cmd_rollback_edits)
            .group(s.cmd_group_actions)
            .icon(IconName::RotateCcw)
            .keywords(["discard", "revert"])
            .is_disabled(pending == 0),
    ]);
}

fn append_view_action_items(s: &Strings, items: &mut Vec<CommandItem>) {
    items.extend([
        CommandItem::new("act:toggle-sidebar", s.cmd_toggle_sidebar)
            .group(s.cmd_group_view)
            .icon(IconName::PanelLeft)
            .shortcut(["Ctrl", "B"])
            .keywords(["browser", "objects"]),
        CommandItem::new("act:toggle-inspector", s.cmd_toggle_inspector)
            .group(s.cmd_group_view)
            .icon(IconName::Columns2)
            .shortcut(["Ctrl", "I"])
            .keywords(["details", "cell"]),
        CommandItem::new("act:toggle-ai", s.cmd_toggle_ai)
            .group(s.cmd_group_view)
            .icon(IconName::Sparkles)
            .shortcut(["Ctrl", "Shift", "A"])
            .keywords(["copilot", "llm", "sql"]),
        CommandItem::new("act:toggle-vim", s.cmd_toggle_vim)
            .group(s.cmd_group_view)
            .icon(IconName::Terminal)
            .keywords(["modal", "editor", "vim"]),
        CommandItem::new("act:toggle-theme", s.cmd_toggle_theme)
            .group(s.cmd_group_view)
            .icon(IconName::SunMoon)
            .shortcut(["Ctrl", "Shift", "M"])
            .keywords(["appearance", "color", "dark", "light"]),
        CommandItem::new("act:connections", s.cmd_open_connections)
            .group(s.cmd_group_navigation)
            .icon(IconName::Server)
            .shortcut(["Ctrl", "Shift", "C"])
            .keywords(["connect", "servers", "import"]),
        CommandItem::new("act:settings", s.cmd_open_settings)
            .group(s.cmd_group_navigation)
            .icon(IconName::Settings)
            .shortcut(["Ctrl", ","])
            .keywords(["preferences", "fonts", "mcp", "theme"]),
    ]);
}

fn append_open_tab_items(controller: &AppController, s: &Strings, items: &mut Vec<CommandItem>) {
    let active = controller.tabs.active_tab();
    for tab in controller.tabs.tabs() {
        let icon = match tab.kind {
            TabKind::QueryEditor { .. } => IconName::FileCode,
            TabKind::TableData { .. } => IconName::Table,
            TabKind::Structure { .. } => IconName::Layers,
        };
        let desc = if active == Some(tab.id) {
            s.cmd_desc_active_tab
        } else {
            match tab.kind {
                TabKind::QueryEditor { .. } => s.cmd_desc_query_editor,
                TabKind::TableData { .. } => s.cmd_desc_table_data,
                TabKind::Structure { .. } => s.cmd_desc_table_structure,
            }
        };
        items.push(
            CommandItem::new(format!("tab:{}", tab.id.0), tab.title.clone())
                .group(s.cmd_group_open_tabs)
                .description(desc)
                .icon(icon)
                .keywords([desc]),
        );
    }
}

fn append_connection_items(controller: &AppController, s: &Strings, items: &mut Vec<CommandItem>) {
    for conn in controller.library.storage().connections.values() {
        let desc = format!("{} · {} / {}", conn.driver_id, conn.host, conn.database);
        items.push(
            CommandItem::new(format!("conn:{}", conn.id.0), conn.name.clone())
                .group(s.cmd_group_saved_connections)
                .description(desc)
                .icon(IconName::Database)
                .keywords([
                    conn.driver_id.as_str(),
                    conn.host.as_str(),
                    conn.database.as_str(),
                ]),
        );
    }
}

fn append_catalog_items(controller: &AppController, s: &Strings, items: &mut Vec<CommandItem>) {
    for (idx, obj) in controller.cached_objects.iter().enumerate() {
        let qualified = match &obj.schema {
            Some(schema) => format!("{schema}.{}", obj.name),
            None => obj.name.clone(),
        };
        let (icon, kind_label) = match obj.kind {
            ObjectKind::Table | ObjectKind::PartitionedTable | ObjectKind::ForeignTable => {
                (IconName::Table, s.obj_kind_table)
            }
            ObjectKind::View | ObjectKind::MaterializedView => (IconName::Layers, s.obj_kind_view),
            _ => (IconName::Code, s.obj_kind_object),
        };
        items.push(
            CommandItem::new(format!("obj:{idx}"), qualified)
                .group(s.cmd_group_schema_objects)
                .description(kind_label)
                .icon(icon)
                .keywords([
                    obj.schema.as_deref().unwrap_or(""),
                    obj.name.as_str(),
                    kind_label,
                ]),
        );
    }
}

fn append_saved_query_items(controller: &AppController, s: &Strings, items: &mut Vec<CommandItem>) {
    for (idx, saved) in controller.saved_queries.iter().enumerate() {
        let snippet = compact_sql_preview(&saved.sql, s.empty_sql_preview);
        items.push(
            CommandItem::new(format!("saved:{idx}"), saved.name.clone())
                .group(s.cmd_group_saved_queries)
                .description(snippet)
                .icon(IconName::Bookmark)
                .keywords([saved.folder.as_deref().unwrap_or(""), saved.sql.as_str()]),
        );
    }
}

fn append_history_items(controller: &AppController, s: &Strings, items: &mut Vec<CommandItem>) {
    let total = controller.query_history.len();
    for (offset, entry) in controller.query_history.iter().rev().take(15).enumerate() {
        let idx = total.saturating_sub(1 + offset);
        let title = compact_sql_preview(&entry.sql, s.empty_sql_preview);
        let status = if entry.succeeded {
            s.status_ok
        } else {
            s.status_error
        };
        let desc = format!("{} · {} ms · {status}", entry.database, entry.elapsed_ms);
        items.push(
            CommandItem::new(format!("hist:{idx}"), title)
                .group(s.cmd_group_recent_queries)
                .description(desc)
                .icon(IconName::History)
                .keywords([entry.sql.as_str(), entry.database.as_str()]),
        );
    }
}

fn compact_sql_preview(sql: &str, empty_preview: &str) -> String {
    let single_line: String = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.len() > 72 {
        let truncated: String = single_line.chars().take(69).collect();
        format!("{truncated}...")
    } else if single_line.is_empty() {
        empty_preview.to_owned()
    } else {
        single_line
    }
}

fn execute_palette_item(
    ws: &mut DataForgeWorkspace,
    key: &str,
    window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) {
    ws.command_palette_ui.close();

    if let Some(act) = key.strip_prefix("act:") {
        execute_named_action(ws, act, window, cx);
    } else if let Some(raw_id) = key.strip_prefix("tab:") {
        if let Ok(id) = raw_id.parse::<u64>() {
            select_tab_by_id(ws, TabId(id));
        }
    } else if let Some(conn_id) = key.strip_prefix("conn:") {
        ws.controller.screen_mode = ScreenMode::Workspace;
        if let Err(err) = ws.controller.connect_library_item_blocking(conn_id) {
            ws.notify_toast(
                ToastSeverity::Error,
                ws.controller.strings.connection_failed,
                Some(err.to_string().into()),
                cx,
            );
        }
    } else if let Some(raw_idx) = key.strip_prefix("obj:") {
        if let Ok(idx) = raw_idx.parse::<usize>()
            && let Some(obj) = ws.controller.cached_objects.get(idx).cloned()
        {
            let table = TableRef::new(obj.schema, obj.name);
            ws.controller.screen_mode = ScreenMode::Workspace;
            ws.workspace_pane.close_tool();
            ws.workspace_pane.sub_view = WorkspaceSubView::DataGrid;
            ws.controller.status_bar.view_mode = TableViewMode::Data;
            if let Err(err) = ws.controller.open_table_tab_blocking(table) {
                ws.notify_toast(
                    ToastSeverity::Error,
                    ws.controller.strings.failed_to_open_table,
                    Some(err.to_string().into()),
                    cx,
                );
            }
        }
    } else if let Some(raw_idx) = key.strip_prefix("saved:") {
        if let Ok(idx) = raw_idx.parse::<usize>()
            && let Some(saved) = ws.controller.saved_queries.get(idx).cloned()
        {
            open_sql_in_new_tab(ws, &saved.name, &saved.sql);
        }
    } else if let Some(raw_idx) = key.strip_prefix("hist:")
        && let Ok(idx) = raw_idx.parse::<usize>()
        && let Some(entry) = ws.controller.query_history.get(idx).cloned()
    {
        let title = ws.controller.strings.history_query_tab;
        open_sql_in_new_tab(ws, title, &entry.sql);
    }

    cx.notify();
}

fn execute_named_action(
    ws: &mut DataForgeWorkspace,
    action: &str,
    window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) {
    match action {
        "run" => {
            let active_tab = ws.controller.tabs.active_tab();
            if let Some(tab_id) = active_tab {
                ws.ensure_tab_result(tab_id).reset_for_run();
            }
            match ws.controller.execute_active_sql_blocking() {
                Ok(summary) => {
                    if let Some(tab_id) = active_tab {
                        let elapsed_ms =
                            u64::try_from(summary.elapsed.as_millis()).unwrap_or(u64::MAX);
                        ws.ensure_tab_result(tab_id).finish(summary, elapsed_ms);
                    }
                }
                Err(err) => {
                    if let Some(tab_id) = active_tab {
                        ws.ensure_tab_result(tab_id).fail(err.to_string());
                    }
                    ws.notify_toast(
                        ToastSeverity::Error,
                        ws.controller.strings.query_failed,
                        Some(err.to_string().into()),
                        cx,
                    );
                }
            }
        }
        "new-tab" => {
            ws.execute_key_command(KeyCommand::NewQueryTab, window, cx);
            ws.workspace_pane.sub_view = WorkspaceSubView::SqlEditor;
        }
        "save-sql" => {
            let default_name = ws.controller.strings.new_query_tab;
            let saved_toast = ws.controller.strings.query_saved_toast;
            ws.controller.save_current_query(default_name, None);
            ws.notify_toast(ToastSeverity::Success, saved_toast, None, cx);
        }
        "close-tab" => ws.execute_key_command(KeyCommand::CloseTabOrWindow, window, cx),
        "reopen-tab" => ws.execute_key_command(KeyCommand::ReopenClosedTab, window, cx),
        "export" => ws.modals_ui.open_export(window, cx),
        "refresh-schema" => {
            super::sidebar::refresh_active_schema(ws, cx);
        }
        "commit" => ws.workspace_pane.confirm_commit_open = true,
        "rollback" => {
            ws.controller.rollback_grid_edits();
            ws.workspace_pane.inline_edit = None;
        }
        "toggle-sidebar" => ws.execute_key_command(KeyCommand::ToggleSidebar, window, cx),
        "toggle-inspector" => ws.execute_key_command(KeyCommand::ToggleInspector, window, cx),
        "toggle-ai" => ws.execute_key_command(KeyCommand::ToggleAiDrawer, window, cx),
        "toggle-vim" => ws.controller.toggle_vim_mode(),
        "toggle-theme" => ws.toggle_theme_mode(window, cx),
        "connections" => ws.switch_screen(ScreenMode::ConnectionsLibrary, cx),
        "settings" => ws.switch_screen(ScreenMode::Settings, cx),
        other => execute_workbench_action(ws, other, window, cx),
    }
}

fn execute_workbench_action(
    ws: &mut DataForgeWorkspace,
    action: &str,
    window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) {
    match action {
        "er-diagram" => open_workbench_tool(ws, WorkspaceToolView::ErDiagram),
        "schema-designer" => open_workbench_tool(ws, WorkspaceToolView::SchemaDesigner),
        "import-wizard" => open_workbench_tool(ws, WorkspaceToolView::ImportWizard),
        "query-builder" => open_workbench_tool(ws, WorkspaceToolView::QueryBuilder),
        "compare" => open_workbench_tool(ws, WorkspaceToolView::Compare),
        "admin" => open_workbench_tool(ws, WorkspaceToolView::Admin),
        "metadata-search" => ws.metadata_search_ui.open_modal(window, cx),
        "format-sql" => {
            let live_sql = ws.sql_editor_ui.current_text(cx);
            if !live_sql.is_empty() {
                ws.controller.set_editor_sql(&live_sql);
            }
            ws.controller.format_editor_sql();
            ws.controller.screen_mode = ScreenMode::Workspace;
            ws.workspace_pane.close_tool();
            ws.workspace_pane.sub_view = WorkspaceSubView::SqlEditor;
        }
        "visual-explain" => run_visual_explain_from_palette(ws, cx),
        "result-charts" => {
            ws.controller.screen_mode = ScreenMode::Workspace;
            ws.workspace_pane.close_tool();
            ws.workspace_pane.sub_view = WorkspaceSubView::SqlEditor;
            if let Some(tab_id) = ws.controller.tabs.active_tab() {
                ws.ensure_tab_result(tab_id).sub_view = ResultPaneSubView::Charts;
            }
        }
        _ => {}
    }
}

fn open_workbench_tool(ws: &mut DataForgeWorkspace, tool: WorkspaceToolView) {
    ws.controller.screen_mode = ScreenMode::Workspace;
    ws.workspace_pane.open_tool(tool);
}

fn run_visual_explain_from_palette(
    ws: &mut DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) {
    let live_sql = ws.sql_editor_ui.current_text(cx);
    if !live_sql.trim().is_empty() {
        ws.controller.set_editor_sql(&live_sql);
    }
    let sql = ws.controller.editor_sql_text();
    match ws.controller.run_visual_explain_blocking(&sql, false) {
        Ok(_) => {
            ws.controller.screen_mode = ScreenMode::Workspace;
            ws.workspace_pane.close_tool();
            ws.workspace_pane.sub_view = WorkspaceSubView::SqlEditor;
            if let Some(tab_id) = ws.controller.tabs.active_tab() {
                ws.ensure_tab_result(tab_id).sub_view = ResultPaneSubView::Explain;
            }
        }
        Err(err) => {
            ws.notify_toast(
                ToastSeverity::Error,
                ws.controller.strings.explain_plan,
                Some(err.to_string().into()),
                cx,
            );
        }
    }
}

fn select_tab_by_id(ws: &mut DataForgeWorkspace, id: TabId) {
    ws.controller.tabs.select(id);
    ws.controller.screen_mode = ScreenMode::Workspace;
    ws.workspace_pane.close_tool();
    if let Some(tab) = ws
        .controller
        .tabs
        .tabs()
        .iter()
        .find(|t| t.id == id)
        .cloned()
    {
        ws.workspace_pane.sub_view = match tab.kind {
            TabKind::QueryEditor { sql } => {
                if !sql.is_empty() {
                    ws.controller.set_editor_sql(&sql);
                }
                WorkspaceSubView::SqlEditor
            }
            TabKind::TableData { .. } => {
                ws.controller.status_bar.view_mode = TableViewMode::Data;
                WorkspaceSubView::DataGrid
            }
            TabKind::Structure { .. } => {
                ws.controller.status_bar.view_mode = TableViewMode::Structure;
                WorkspaceSubView::Structure
            }
        };
    }
}

fn open_sql_in_new_tab(ws: &mut DataForgeWorkspace, title: &str, sql: &str) {
    ws.controller.screen_mode = ScreenMode::Workspace;
    ws.workspace_pane.close_tool();
    ws.controller
        .tabs
        .open_query_tab(TabConnectionId("default".to_owned()), Some(title));
    ws.controller.set_editor_sql(sql);
    ws.workspace_pane.sub_view = WorkspaceSubView::SqlEditor;
    ws.controller.sync_layout_counts();
}
