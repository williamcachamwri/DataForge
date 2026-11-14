//! Keyboard-driven HeroGPUI `CommandPalette` overlay (`Ctrl+K`) backed by real
//! workspace actions, open tabs, saved connections, catalog objects, and queries.

use herogpui::components::{CommandItem, CommandPalette, IconName};
use herogpui::gpui::{AnyElement, Context, IntoElement, SharedString, Window, px};

use crate::accessibility::KeyCommand;
use crate::app::{AppController, ScreenMode};
use crate::views::status_bar::TableViewMode;
use dataforge_state::connection_window::ConnectionWindowPhase;
use dataforge_state::tabs::{TabConnectionId, TabId, TabKind};
use tablepro_core::meta::{ObjectKind, TableRef};

use super::DataForgeWorkspace;
use super::shell_state::{ToastSeverity, WorkspaceSubView};

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
    let items = build_command_items(&ws.controller);
    let palette = CommandPalette::new("dataforge-command-palette", items)
        .is_open(ws.command_palette_ui.is_open())
        .placeholder("Search actions, connections, tables, tabs, or queries...")
        .empty_text("No matching commands or objects")
        .label("DataForge Command Palette")
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
    let mut items = Vec::new();
    append_action_items(controller, &mut items);
    append_open_tab_items(controller, &mut items);
    append_connection_items(controller, &mut items);
    append_catalog_items(controller, &mut items);
    append_saved_query_items(controller, &mut items);
    append_history_items(controller, &mut items);
    items
}

fn append_action_items(controller: &AppController, items: &mut Vec<CommandItem>) {
    let is_connected = matches!(
        controller.connections.phase(controller.active_workspace),
        Some(ConnectionWindowPhase::Connected)
    );
    let has_tabs = !controller.tabs.tabs().is_empty();
    let has_export = !controller.grid_rows.is_empty();
    let edits = &controller.grid_model.edits;
    let pending = edits.modified_cells.len() + edits.inserted_rows.len() + edits.deleted_rows.len();

    append_query_action_items(is_connected, has_tabs, has_export, pending, items);
    append_view_action_items(items);
}

fn append_query_action_items(
    is_connected: bool,
    has_tabs: bool,
    has_export: bool,
    pending: usize,
    items: &mut Vec<CommandItem>,
) {
    items.extend([
        CommandItem::new("act:run", "Run SQL Query")
            .group("Actions")
            .icon(IconName::Play)
            .shortcut(["Ctrl", "Enter"])
            .keywords(["execute", "query", "sql"])
            .is_disabled(!is_connected),
        CommandItem::new("act:new-tab", "New Query Tab")
            .group("Actions")
            .icon(IconName::FilePlus)
            .shortcut(["Ctrl", "T"])
            .keywords(["editor", "sql", "tab"]),
        CommandItem::new("act:save-sql", "Save Active SQL Query")
            .group("Actions")
            .icon(IconName::Save)
            .shortcut(["Ctrl", "S"])
            .keywords(["save", "snippet", "query"]),
        CommandItem::new("act:close-tab", "Close Active Tab")
            .group("Actions")
            .icon(IconName::X)
            .shortcut(["Ctrl", "W"])
            .is_disabled(!has_tabs),
        CommandItem::new("act:reopen-tab", "Reopen Closed Tab")
            .group("Actions")
            .icon(IconName::Undo2)
            .shortcut(["Ctrl", "Shift", "T"])
            .keywords(["restore", "undo"]),
        CommandItem::new("act:export", "Export Table or Query Results")
            .group("Actions")
            .icon(IconName::Download)
            .keywords(["csv", "json", "sql", "dump"])
            .is_disabled(!has_export),
        CommandItem::new("act:refresh-schema", "Refresh Database Schemas & Objects")
            .group("Actions")
            .icon(IconName::RefreshCw)
            .keywords(["reload", "catalog", "tables"])
            .is_disabled(!is_connected),
        CommandItem::new("act:commit", "Commit Uncommitted Grid Edits")
            .group("Actions")
            .icon(IconName::Check)
            .keywords(["apply", "save", "transaction"])
            .is_disabled(!is_connected || pending == 0),
        CommandItem::new("act:rollback", "Rollback Uncommitted Grid Edits")
            .group("Actions")
            .icon(IconName::RotateCcw)
            .keywords(["discard", "revert"])
            .is_disabled(pending == 0),
    ]);
}

fn append_view_action_items(items: &mut Vec<CommandItem>) {
    items.extend([
        CommandItem::new("act:toggle-sidebar", "Toggle Schema Sidebar")
            .group("View")
            .icon(IconName::PanelLeft)
            .shortcut(["Ctrl", "B"])
            .keywords(["browser", "objects"]),
        CommandItem::new("act:toggle-inspector", "Toggle Row Inspector")
            .group("View")
            .icon(IconName::Columns2)
            .shortcut(["Ctrl", "I"])
            .keywords(["details", "cell"]),
        CommandItem::new("act:toggle-ai", "Toggle AI Assistant Drawer")
            .group("View")
            .icon(IconName::Sparkles)
            .shortcut(["Ctrl", "Shift", "A"])
            .keywords(["copilot", "llm", "sql"]),
        CommandItem::new("act:toggle-vim", "Toggle SQL Vim Keybindings")
            .group("View")
            .icon(IconName::Terminal)
            .keywords(["modal", "editor", "vim"]),
        CommandItem::new("act:toggle-theme", "Switch Light / Dark Theme")
            .group("View")
            .icon(IconName::SunMoon)
            .shortcut(["Ctrl", "Shift", "M"])
            .keywords(["appearance", "color", "dark", "light"]),
        CommandItem::new("act:connections", "Open Connections Library")
            .group("Navigation")
            .icon(IconName::Server)
            .shortcut(["Ctrl", "Shift", "C"])
            .keywords(["connect", "servers", "import"]),
        CommandItem::new("act:settings", "Open Application Settings")
            .group("Navigation")
            .icon(IconName::Settings)
            .shortcut(["Ctrl", ","])
            .keywords(["preferences", "fonts", "mcp", "theme"]),
    ]);
}

fn append_open_tab_items(controller: &AppController, items: &mut Vec<CommandItem>) {
    let active = controller.tabs.active_tab();
    for tab in controller.tabs.tabs() {
        let icon = match tab.kind {
            TabKind::QueryEditor { .. } => IconName::FileCode,
            TabKind::TableData { .. } => IconName::Table,
            TabKind::Structure { .. } => IconName::Layers,
        };
        let desc = if active == Some(tab.id) {
            "Active Tab"
        } else {
            match tab.kind {
                TabKind::QueryEditor { .. } => "Query Editor",
                TabKind::TableData { .. } => "Table Data",
                TabKind::Structure { .. } => "Table Structure",
            }
        };
        items.push(
            CommandItem::new(format!("tab:{}", tab.id.0), tab.title.clone())
                .group("Open Tabs")
                .description(desc)
                .icon(icon)
                .keywords([desc]),
        );
    }
}

fn append_connection_items(controller: &AppController, items: &mut Vec<CommandItem>) {
    for conn in controller.library.storage().connections.values() {
        let desc = format!("{} · {} / {}", conn.driver_id, conn.host, conn.database);
        items.push(
            CommandItem::new(format!("conn:{}", conn.id.0), conn.name.clone())
                .group("Saved Connections")
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

fn append_catalog_items(controller: &AppController, items: &mut Vec<CommandItem>) {
    for (idx, obj) in controller.cached_objects.iter().enumerate() {
        let qualified = match &obj.schema {
            Some(schema) => format!("{schema}.{}", obj.name),
            None => obj.name.clone(),
        };
        let (icon, kind_label) = match obj.kind {
            ObjectKind::Table | ObjectKind::PartitionedTable | ObjectKind::ForeignTable => {
                (IconName::Table, "Table")
            }
            ObjectKind::View | ObjectKind::MaterializedView => (IconName::Layers, "View"),
            _ => (IconName::Code, "Object"),
        };
        items.push(
            CommandItem::new(format!("obj:{idx}"), qualified)
                .group("Schema Objects")
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

fn append_saved_query_items(controller: &AppController, items: &mut Vec<CommandItem>) {
    for (idx, saved) in controller.saved_queries.iter().enumerate() {
        let snippet = compact_sql_preview(&saved.sql);
        items.push(
            CommandItem::new(format!("saved:{idx}"), saved.name.clone())
                .group("Saved Queries")
                .description(snippet)
                .icon(IconName::Bookmark)
                .keywords([saved.folder.as_deref().unwrap_or(""), saved.sql.as_str()]),
        );
    }
}

fn append_history_items(controller: &AppController, items: &mut Vec<CommandItem>) {
    let total = controller.query_history.len();
    for (offset, entry) in controller.query_history.iter().rev().take(15).enumerate() {
        let idx = total.saturating_sub(1 + offset);
        let title = compact_sql_preview(&entry.sql);
        let status = if entry.succeeded { "OK" } else { "Error" };
        let desc = format!("{} · {} ms · {status}", entry.database, entry.elapsed_ms);
        items.push(
            CommandItem::new(format!("hist:{idx}"), title)
                .group("Recent Queries")
                .description(desc)
                .icon(IconName::History)
                .keywords([entry.sql.as_str(), entry.database.as_str()]),
        );
    }
}

fn compact_sql_preview(sql: &str) -> String {
    let single_line: String = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.len() > 72 {
        let truncated: String = single_line.chars().take(69).collect();
        format!("{truncated}...")
    } else if single_line.is_empty() {
        "(Empty SQL)".to_owned()
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
                "Connection failed",
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
            ws.workspace_pane.sub_view = WorkspaceSubView::DataGrid;
            ws.controller.status_bar.view_mode = TableViewMode::Data;
            if let Err(err) = ws.controller.open_table_tab_blocking(table) {
                ws.notify_toast(
                    ToastSeverity::Error,
                    "Failed to open table",
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
        open_sql_in_new_tab(ws, "History Query", &entry.sql);
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
                        "Query failed",
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
            ws.controller.save_current_query("Query", None);
            ws.notify_toast(ToastSeverity::Success, "Query saved", None, cx);
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
        _ => {}
    }
}

fn select_tab_by_id(ws: &mut DataForgeWorkspace, id: TabId) {
    ws.controller.tabs.select(id);
    ws.controller.screen_mode = ScreenMode::Workspace;
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
    ws.controller
        .tabs
        .open_query_tab(TabConnectionId("default".to_owned()), Some(title));
    ws.controller.set_editor_sql(sql);
    ws.workspace_pane.sub_view = WorkspaceSubView::SqlEditor;
    ws.controller.sync_layout_counts();
}
