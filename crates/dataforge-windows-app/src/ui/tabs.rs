//! Horizontal tab strip backed by `dataforge_state::tabs::TabStripModel`
//! plus the `[SQL | Grid | Structure]` workspace sub-view switch.

use std::collections::BTreeMap;

use herogpui::components::{
    Button, ContextMenu, Icon, IconName, Input, InputState, MenuItem, Size, TabItem as HeroTabItem,
    Tabs, TabsSize, TabsVariant, Tooltip, TooltipPlacement, Variant, extend::shift_wheel_scroll_x,
};
use herogpui::core::with_alpha;
use herogpui::gpui::{
    AnyElement, AppContext as _, Context, Entity, FontWeight, InteractiveElement, IntoElement,
    ParentElement, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use herogpui::theme::ActiveTheme;

use crate::accessibility::KeyCommand;
use crate::app::ScreenMode;
use crate::views::status_bar::TableViewMode;
use dataforge_state::tabs::{TabConnectionId, TabId, TabItem, TabKind};
use tablepro_core::meta::TableRef;

use super::DataForgeWorkspace;
use super::shell_state::{ToastSeverity, WorkspaceSubView};
use super::theme::{
    CONTROL_RADIUS_PX, HAIRLINE_BORDER_PX, MICRO_GAP_PX, hairline_border_color, is_reduced_motion,
    muted_surface_bg, panel_surface_bg,
};

/// Persistent UI state for the horizontal workspace tab strip.
#[derive(Debug, Default)]
pub struct TabsStripState {
    pub scroll_handle: ScrollHandle,
    title_overrides: BTreeMap<TabId, String>,
    renaming_tab: Option<TabId>,
    rename_input: Option<Entity<InputState>>,
}

impl TabsStripState {
    /// Creates tab-strip scroll/rename state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) -> Self {
        Self {
            scroll_handle: ScrollHandle::new(),
            title_overrides: BTreeMap::new(),
            renaming_tab: None,
            rename_input: None,
        }
    }

    fn display_title<'a>(&'a self, tab: &'a TabItem) -> &'a str {
        self.title_overrides
            .get(&tab.id)
            .map_or(tab.title.as_str(), String::as_str)
    }

    fn begin_rename(
        &mut self,
        tab_id: TabId,
        current_title: &str,
        cx: &mut Context<DataForgeWorkspace>,
    ) {
        let initial = current_title.to_owned();
        let entity = cx.new(|cx| InputState::with_value(cx, initial));
        self.renaming_tab = Some(tab_id);
        self.rename_input = Some(entity);
    }

    fn commit_rename(&mut self, tab_id: TabId, next: &str) {
        let trimmed = next.trim();
        if !trimmed.is_empty() {
            self.title_overrides.insert(tab_id, trimmed.to_owned());
        }
        self.renaming_tab = None;
        self.rename_input = None;
    }

    fn cancel_rename(&mut self) {
        self.renaming_tab = None;
        self.rename_input = None;
    }
}

/// Renders the workspace tab strip, tab management controls, and `[SQL | Grid | Structure]` switcher.
#[must_use]
pub fn render_tab_strip(
    ws: &mut DataForgeWorkspace,
    window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let skip_anim = is_reduced_motion(cx);
    let active_tab = ws.controller.tabs.active_tab();
    let pending_edits = pending_grid_edits(ws);
    let tabs_snapshot: Vec<TabItem> = ws.controller.tabs.tabs().to_vec();
    let tab_count = tabs_snapshot.len();
    let renaming_tab = ws.tabs_ui.renaming_tab;
    let rename_input = ws.tabs_ui.rename_input.clone();

    let mut strip = div()
        .id("workspace-tabs-scroll")
        .flex_1()
        .min_w(px(0.0))
        .h_full()
        .flex()
        .items_center()
        .gap(px(MICRO_GAP_PX))
        .overflow_x_scroll()
        .track_scroll(&ws.tabs_ui.scroll_handle)
        .on_scroll({
            let handle = ws.tabs_ui.scroll_handle.clone();
            move |event, window, _cx| {
                shift_wheel_scroll_x(&handle, event, window);
            }
        });

    if tabs_snapshot.is_empty() {
        strip = strip.child(render_empty_tabs_notice(cx));
    } else {
        for tab in &tabs_snapshot {
            let title = ws.tabs_ui.display_title(tab).to_owned();
            let is_active = active_tab == Some(tab.id);
            let is_dirty = tab.has_unsaved_edits || (is_active && pending_edits > 0);
            let input_for_tab = if renaming_tab == Some(tab.id) {
                rename_input.clone()
            } else {
                None
            };
            strip = strip.child(render_tab_pill(
                tab,
                &title,
                is_active,
                is_dirty,
                tab_count,
                input_for_tab,
                cx,
            ));
        }
    }

    let _ = window;
    div()
        .id("workspace-tabs")
        .h(px(36.0))
        .w_full()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_between()
        .px(px(8.0))
        .gap(px(8.0))
        .bg(muted_surface_bg(cx))
        .border_b(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .h_full()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(strip)
                .child(render_tab_strip_actions(skip_anim, cx)),
        )
        .child(render_subview_switcher(ws.workspace_pane.sub_view, cx))
        .into_any_element()
}

fn render_empty_tabs_notice(cx: &Context<DataForgeWorkspace>) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .id("tabs-empty-notice")
        .px(px(8.0))
        .text_size(px(12.0))
        .text_color(colors.muted)
        .child("No open tabs")
        .into_any_element()
}

fn render_tab_pill(
    tab: &TabItem,
    title: &str,
    is_active: bool,
    is_dirty: bool,
    total_tabs: usize,
    rename_input: Option<Entity<InputState>>,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let tab_id = tab.id;
    let tab_kind = tab.kind.clone();
    let menu_tab_kind = tab.kind.clone();
    let current_title = title.to_owned();
    let conn_id = tab.connection_id.clone();

    let mut pill = build_tab_pill_shell(tab_id, &tab_kind, is_active, cx).child(
        render_tab_title_slot(tab_id, title, is_active, rename_input, cx),
    );

    if is_dirty {
        pill = pill.child(
            div()
                .id(format!("workspace-tab-dirty-{}", tab_id.0))
                .w(px(6.0))
                .h(px(6.0))
                .rounded(px(99.0))
                .flex_shrink_0()
                .bg(colors.warning.color),
        );
    }

    pill = pill.child(render_tab_close_button(tab_id, title, cx));
    wrap_tab_context_menu(
        pill.into_any_element(),
        tab_id,
        current_title,
        conn_id,
        menu_tab_kind,
        total_tabs,
        cx,
    )
}

fn build_tab_pill_shell(
    tab_id: TabId,
    tab_kind: &TabKind,
    is_active: bool,
    cx: &Context<DataForgeWorkspace>,
) -> herogpui::gpui::Stateful<herogpui::gpui::Div> {
    let colors = cx.colors().clone();
    let kind_icon = match tab_kind {
        TabKind::QueryEditor { .. } => IconName::FileCode,
        TabKind::TableData { .. } => IconName::Table,
        TabKind::Structure { .. } => IconName::Layers,
    };
    let bg = if is_active {
        panel_surface_bg(cx)
    } else {
        with_alpha(colors.surface_secondary, 0.0)
    };
    let hover_bg = with_alpha(colors.default.color, 0.45);
    let click_kind = tab_kind.clone();

    div()
        .id(format!("workspace-tab-{}", tab_id.0))
        .h(px(28.0))
        .min_w(px(84.0))
        .max_w(px(210.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(8.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(bg)
        .border(px(HAIRLINE_BORDER_PX))
        .border_color(if is_active {
            hairline_border_color(cx)
        } else {
            with_alpha(colors.border, 0.0)
        })
        .when(!is_active, |el| {
            el.hover(move |s| s.bg(hover_bg).text_color(colors.foreground))
        })
        .on_click(cx.listener(move |ws, _, _, cx| {
            select_workspace_tab(ws, tab_id, &click_kind, cx);
        }))
        .child(Icon::new(kind_icon).size(px(13.0)).color(if is_active {
            colors.accent.color
        } else {
            colors.muted
        }))
}

fn render_tab_title_slot(
    tab_id: TabId,
    title: &str,
    is_active: bool,
    rename_input: Option<Entity<InputState>>,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    if let Some(input_entity) = rename_input {
        return render_tab_rename_input(tab_id, input_entity, cx);
    }
    let colors = cx.colors().clone();
    div()
        .flex_1()
        .min_w(px(0.0))
        .truncate()
        .text_size(px(12.0))
        .font_weight(if is_active {
            FontWeight::MEDIUM
        } else {
            FontWeight::NORMAL
        })
        .text_color(if is_active {
            colors.foreground
        } else {
            colors.muted
        })
        .child(title.to_owned())
        .into_any_element()
}

fn render_tab_rename_input(
    tab_id: TabId,
    input_entity: Entity<InputState>,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    div()
        .w(px(116.0))
        .on_key_down(
            cx.listener(|ws, event: &herogpui::gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    ws.tabs_ui.cancel_rename();
                    cx.notify();
                    cx.stop_propagation();
                }
            }),
        )
        .child(
            Input::new(&input_entity)
                .auto_focus(true)
                .is_bare(true)
                .height(px(20.0))
                .text_size(px(12.0))
                .on_submit(cx.listener(move |ws, text: &str, _, cx| {
                    ws.tabs_ui.commit_rename(tab_id, text);
                    cx.notify();
                })),
        )
        .into_any_element()
}

fn render_tab_close_button(
    tab_id: TabId,
    title: &str,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    Button::new(format!("workspace-tab-close-{}", tab_id.0))
        .variant(Variant::Ghost)
        .size(Size::Sm)
        .is_icon_only(true)
        .width(px(18.0))
        .height(px(18.0))
        .aria_label(format!("Close {title}"))
        .on_press(cx.listener(move |ws, _, _, cx| {
            cx.stop_propagation();
            close_workspace_tab(ws, tab_id, cx);
        }))
        .child(Icon::new(IconName::X).size(px(11.0)).color(colors.muted))
        .into_any_element()
}

fn wrap_tab_context_menu(
    pill: AnyElement,
    tab_id: TabId,
    current_title: String,
    conn_id: TabConnectionId,
    menu_tab_kind: TabKind,
    total_tabs: usize,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let menu_items = build_tab_context_menu_items(total_tabs);
    let mut ctx_menu = ContextMenu::new(format!("tab-context-menu-{}", tab_id.0), pill, menu_items)
        .on_action(cx.listener(move |ws, key: &SharedString, _, cx| {
            handle_tab_context_action(
                ws,
                key.as_ref(),
                tab_id,
                &current_title,
                &conn_id,
                &menu_tab_kind,
                cx,
            );
        }));
    if total_tabs <= 1 {
        ctx_menu = ctx_menu.disabled_keys(["close-others"]);
    }
    ctx_menu.into_any_element()
}

fn build_tab_context_menu_items(total_tabs: usize) -> Vec<MenuItem> {
    let mut items = vec![
        MenuItem::new("save", "Save Query")
            .icon(IconName::Save.path())
            .shortcut("Ctrl+S"),
        MenuItem::new("rename", "Rename Tab").icon(IconName::Pencil.path()),
        MenuItem::new("duplicate", "Duplicate Tab").icon(IconName::Copy.path()),
        MenuItem::Separator,
        MenuItem::new("close", "Close")
            .icon(IconName::X.path())
            .shortcut("Ctrl+W"),
    ];
    let mut close_others = MenuItem::new("close-others", "Close Others");
    if total_tabs > 1 {
        close_others = close_others.danger();
    }
    items.push(close_others);
    items
}

fn render_tab_strip_actions(skip_anim: bool, cx: &Context<DataForgeWorkspace>) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .id("workspace-tab-actions")
        .flex()
        .items_center()
        .gap(px(2.0))
        .flex_shrink_0()
        .child(
            Tooltip::new("New Query Tab (Ctrl+T)")
                .id("tabs-new-query-tip")
                .placement(TooltipPlacement::Bottom)
                .delay(350)
                .close_delay(80)
                .should_skip_animation(skip_anim)
                .child(
                    Button::new("tabs-new-query-btn")
                        .variant(Variant::Ghost)
                        .size(Size::Sm)
                        .is_icon_only(true)
                        .aria_label("New Query Tab")
                        .on_press(cx.listener(|ws, _, window, cx| {
                            ws.execute_key_command(KeyCommand::NewQueryTab, window, cx);
                            ws.workspace_pane.sub_view = WorkspaceSubView::SqlEditor;
                            cx.notify();
                        }))
                        .child(
                            Icon::new(IconName::Plus)
                                .size(px(14.0))
                                .color(colors.foreground),
                        ),
                ),
        )
        .child(
            Tooltip::new("Reopen Closed Tab (Ctrl+Shift+T)")
                .id("tabs-reopen-tip")
                .placement(TooltipPlacement::Bottom)
                .delay(350)
                .close_delay(80)
                .should_skip_animation(skip_anim)
                .child(
                    Button::new("tabs-reopen-btn")
                        .variant(Variant::Ghost)
                        .size(Size::Sm)
                        .is_icon_only(true)
                        .aria_label("Reopen Closed Tab")
                        .on_press(cx.listener(|ws, _, _, cx| {
                            if ws.controller.reopen_closed_tab().is_none() {
                                ws.notify_toast(
                                    ToastSeverity::Info,
                                    "No recently closed tab",
                                    None,
                                    cx,
                                );
                            }
                            cx.notify();
                        }))
                        .child(
                            Icon::new(IconName::Undo2)
                                .size(px(13.0))
                                .color(colors.muted),
                        ),
                ),
        )
        .into_any_element()
}

fn render_subview_switcher(
    sub_view: WorkspaceSubView,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let selected_key = match sub_view {
        WorkspaceSubView::SqlEditor | WorkspaceSubView::SplitSqlAndGrid => "sql",
        WorkspaceSubView::DataGrid => "grid",
        WorkspaceSubView::Structure => "structure",
    };

    let items = vec![
        HeroTabItem::new("sql", "SQL")
            .height(px(24.0))
            .padding_x(px(10.0)),
        HeroTabItem::new("grid", "Grid")
            .height(px(24.0))
            .padding_x(px(10.0)),
        HeroTabItem::new("structure", "Structure")
            .height(px(24.0))
            .padding_x(px(10.0)),
    ];

    Tabs::new("workspace-subview-tabs", items, "sql")
        .selected_key(selected_key)
        .variant(TabsVariant::Primary)
        .size(TabsSize::Sm)
        .text_size(px(11.5))
        .on_selection_change(cx.listener(|ws, key: &SharedString, _, cx| {
            apply_subview_selection(ws, key.as_ref(), cx);
        }))
        .into_any_element()
}

fn apply_subview_selection(
    ws: &mut DataForgeWorkspace,
    key: &str,
    cx: &mut Context<DataForgeWorkspace>,
) {
    match key {
        "sql" => {
            ws.workspace_pane.sub_view = WorkspaceSubView::SqlEditor;
        }
        "grid" => {
            ws.workspace_pane.sub_view = WorkspaceSubView::DataGrid;
            ws.controller.status_bar.view_mode = TableViewMode::Data;
        }
        "structure" => {
            ws.workspace_pane.sub_view = WorkspaceSubView::Structure;
            ws.controller.status_bar.view_mode = TableViewMode::Structure;
            if let Some(table) = ws.controller.active_table.clone()
                && let Err(err) = ws.controller.load_table_structure_blocking(&table)
            {
                ws.notify_toast(
                    ToastSeverity::Error,
                    "Failed to load table structure",
                    Some(err.to_string().into()),
                    cx,
                );
            }
        }
        _ => {}
    }
    cx.notify();
}

fn select_workspace_tab(
    ws: &mut DataForgeWorkspace,
    tab_id: TabId,
    kind: &TabKind,
    cx: &mut Context<DataForgeWorkspace>,
) {
    ws.controller.tabs.select(tab_id);
    ws.controller.screen_mode = ScreenMode::Workspace;
    ws.workspace_pane.sub_view = match kind {
        TabKind::QueryEditor { sql } => {
            if !sql.is_empty() {
                ws.controller.set_editor_sql(sql);
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
    cx.notify();
}

fn close_workspace_tab(
    ws: &mut DataForgeWorkspace,
    tab_id: TabId,
    cx: &mut Context<DataForgeWorkspace>,
) {
    ws.tabs_ui.title_overrides.remove(&tab_id);
    if ws.tabs_ui.renaming_tab == Some(tab_id) {
        ws.tabs_ui.cancel_rename();
    }
    ws.controller.tabs.close_tab_by_user(tab_id);
    ws.controller.sync_layout_counts();
    cx.notify();
}

fn handle_tab_context_action(
    ws: &mut DataForgeWorkspace,
    action: &str,
    tab_id: TabId,
    title: &str,
    conn_id: &TabConnectionId,
    kind: &TabKind,
    cx: &mut Context<DataForgeWorkspace>,
) {
    match action {
        "save" => {
            ws.controller.save_current_query(title, None);
            ws.notify_toast(
                ToastSeverity::Success,
                "Query saved",
                Some(title.to_owned().into()),
                cx,
            );
        }
        "rename" => {
            ws.tabs_ui.begin_rename(tab_id, title, cx);
        }
        "duplicate" => duplicate_workspace_tab(ws, title, conn_id, kind, cx),
        "close" => {
            close_workspace_tab(ws, tab_id, cx);
            return;
        }
        "close-others" => {
            let others: Vec<TabId> = ws
                .controller
                .tabs
                .tabs()
                .iter()
                .filter(|t| t.id != tab_id)
                .map(|t| t.id)
                .collect();
            for other in others {
                ws.tabs_ui.title_overrides.remove(&other);
                ws.controller.tabs.close_tab_by_user(other);
            }
            ws.controller.tabs.select(tab_id);
            ws.controller.sync_layout_counts();
        }
        _ => {}
    }
    cx.notify();
}

fn duplicate_workspace_tab(
    ws: &mut DataForgeWorkspace,
    title: &str,
    conn_id: &TabConnectionId,
    kind: &TabKind,
    cx: &mut Context<DataForgeWorkspace>,
) {
    let copy_title = format!("{title} Copy");
    match kind {
        TabKind::QueryEditor { sql } => {
            let active_sql = if sql.is_empty() {
                ws.controller.editor_sql_text()
            } else {
                sql.clone()
            };
            ws.controller
                .tabs
                .open_query_tab(conn_id.clone(), Some(&copy_title));
            if !active_sql.is_empty() {
                ws.controller.set_editor_sql(&active_sql);
            }
            ws.workspace_pane.sub_view = WorkspaceSubView::SqlEditor;
        }
        TabKind::TableData { table_name } | TabKind::Structure { table_name } => {
            let table_ref = parse_table_ref(table_name);
            if let Err(err) = ws.controller.open_table_tab_blocking(table_ref) {
                ws.notify_toast(
                    ToastSeverity::Error,
                    "Failed to open table tab",
                    Some(err.to_string().into()),
                    cx,
                );
            }
        }
    }
    ws.controller.sync_layout_counts();
}

fn parse_table_ref(raw: &str) -> TableRef {
    match raw.split_once('.') {
        Some((schema, name)) => TableRef::new(Some(schema.to_owned()), name),
        None => TableRef::unqualified(raw),
    }
}

fn pending_grid_edits(ws: &DataForgeWorkspace) -> usize {
    let edits = &ws.controller.grid_model.edits;
    edits.modified_cells.len() + edits.inserted_rows.len() + edits.deleted_rows.len()
}
