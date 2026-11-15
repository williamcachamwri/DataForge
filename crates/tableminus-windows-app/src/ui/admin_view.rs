//! Server Administration, Live Server Monitor, and Scheduled Tasks view.

use std::time::Duration;

use tableminus_services::server_monitor::{
    self, LiveSessionRow, ServerLockRow, ServerMonitorSnapshot,
};
use tableminus_services::{
    DatabaseEngine, active_sessions_query, kill_session_sql, users_roles_query,
};
use tableminus_state::connection_window::{ConnectionWindowPhase, WorkspaceId};
use tableminus_windows_platform::{ScheduledTaskRecord, ScheduledTaskRunEntry};
use herogpui::components::{
    Alert, Button, Chip, ChipLabel, ChipVariant, Icon, IconName, IconSize, Input, InputState,
    PickerItem, Select, Separator, Switch, TabItem, Tabs, TabsSize, TabsVariant,
};
use herogpui::core::{Color as HeroColor, Size as HeroSize, Variant};
use herogpui::gpui::{
    AnyElement, App, AppContext as _, AsyncApp, Context, Entity, FontWeight, InteractiveElement,
    IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, Task, WeakEntity,
    Window, div, px,
};
use herogpui::theme::ActiveTheme;

use super::TableMinusWorkspace;
use super::results_pane::format_core_value;
use super::shell_state::{push_error_toast, push_success_toast};
use super::theme::{
    COMPACT_GAP_PX, CONTROL_RADIUS_PX, PANEL_RADIUS_PX, editor_gpui_font, elevated_overlay_bg,
    hairline_border_color, muted_surface_bg, panel_surface_bg, row_zebra_bg, value_gpui_font,
};

const SECTION_USERS: usize = 0;
const SECTION_SESSIONS: usize = 1;
const SECTION_MONITOR_SESSIONS: usize = 2;
const SECTION_MONITOR_LOCKS: usize = 3;
const SECTION_TASKS: usize = 4;
const MONITOR_AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingKillSession {
    id: String,
    user: String,
    query: String,
}

/// Persistent UI state for the Server Admin, Live Server Monitor, and Scheduled Tasks view.
pub struct AdminViewState {
    pub active_section: usize,
    users_cols: Vec<String>,
    users_rows: Vec<Vec<String>>,
    users_notice: Option<String>,
    users_error: Option<String>,
    users_loaded_ws: Option<WorkspaceId>,
    sessions_cols: Vec<String>,
    sessions_rows: Vec<Vec<String>>,
    sessions_notice: Option<String>,
    sessions_error: Option<String>,
    sessions_loaded_ws: Option<WorkspaceId>,
    monitor_snapshot: Option<ServerMonitorSnapshot>,
    monitor_notice: Option<String>,
    monitor_error: Option<String>,
    monitor_loaded_ws: Option<WorkspaceId>,
    monitor_loading: bool,
    monitor_auto_refresh: bool,
    monitor_auto_refresh_gen: u64,
    monitor_auto_refresh_task: Option<Task<()>>,
    monitor_fetch_gen: u64,
    monitor_fetch_task: Option<Task<()>>,
    monitor_confirm_kill: Option<PendingKillSession>,
    monitor_killing: bool,
    monitor_kill_task: Option<Task<()>>,
    new_task_open: bool,
    task_name_input: Entity<InputState>,
    task_sql_input: Entity<InputState>,
    task_interval_input: Entity<InputState>,
    task_kind: String,
    task_connection_id: Option<String>,
    task_form_error: Option<String>,
    expanded_history_task_id: Option<String>,
}

impl std::fmt::Debug for AdminViewState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminViewState")
            .field("active_section", &self.active_section)
            .field("users_loaded_ws", &self.users_loaded_ws)
            .field("sessions_loaded_ws", &self.sessions_loaded_ws)
            .field("monitor_loaded_ws", &self.monitor_loaded_ws)
            .field("monitor_loading", &self.monitor_loading)
            .field("monitor_auto_refresh", &self.monitor_auto_refresh)
            .field("monitor_confirm_kill", &self.monitor_confirm_kill)
            .field("monitor_killing", &self.monitor_killing)
            .field("new_task_open", &self.new_task_open)
            .finish_non_exhaustive()
    }
}

impl AdminViewState {
    /// Creates initial Admin view state owned by [`TableMinusWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, cx: &mut Context<TableMinusWorkspace>) -> Self {
        let task_name_input = cx.new(|cx| InputState::new(cx));
        let task_sql_input = cx.new(|cx| InputState::new(cx));
        let task_interval_input = cx.new(|cx| {
            let mut state = InputState::new(cx);
            state.set_value("15".to_owned());
            state
        });
        Self {
            active_section: SECTION_USERS,
            users_cols: Vec::new(),
            users_rows: Vec::new(),
            users_notice: None,
            users_error: None,
            users_loaded_ws: None,
            sessions_cols: Vec::new(),
            sessions_rows: Vec::new(),
            sessions_notice: None,
            sessions_error: None,
            sessions_loaded_ws: None,
            monitor_snapshot: None,
            monitor_notice: None,
            monitor_error: None,
            monitor_loaded_ws: None,
            monitor_loading: false,
            monitor_auto_refresh: false,
            monitor_auto_refresh_gen: 0,
            monitor_auto_refresh_task: None,
            monitor_fetch_gen: 0,
            monitor_fetch_task: None,
            monitor_confirm_kill: None,
            monitor_killing: false,
            monitor_kill_task: None,
            new_task_open: false,
            task_name_input,
            task_sql_input,
            task_interval_input,
            task_kind: "sql".to_owned(),
            task_connection_id: None,
            task_form_error: None,
            expanded_history_task_id: None,
        }
    }
}

/// Renders the Server Admin & Scheduled Tasks workbench tool view.
#[must_use]
pub fn render_admin_view(
    ws: &mut TableMinusWorkspace,
    _window: &mut Window,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let is_connected = matches!(
        ws.controller
            .connections
            .phase(ws.controller.active_workspace),
        Some(ConnectionWindowPhase::Connected)
    );
    ensure_section_loaded(ws, is_connected, cx);

    let body = match ws.admin_ui.active_section {
        SECTION_USERS => render_users_section(ws, is_connected, cx),
        SECTION_SESSIONS => render_sessions_section(ws, is_connected, cx),
        SECTION_MONITOR_SESSIONS => render_monitor_sessions_section(ws, is_connected, cx),
        SECTION_MONITOR_LOCKS => render_monitor_locks_section(ws, is_connected, cx),
        _ => render_tasks_section(ws, cx),
    };

    div()
        .id("admin-view")
        .size_full()
        .flex()
        .flex_col()
        .bg(panel_surface_bg(cx))
        .child(render_admin_header(ws, cx))
        .child(render_admin_tabs(ws, cx))
        .child(Separator::new())
        .child(body)
        .into_any_element()
}

fn ensure_section_loaded(
    ws: &mut TableMinusWorkspace,
    is_connected: bool,
    cx: &mut Context<TableMinusWorkspace>,
) {
    if !is_connected {
        return;
    }
    let active_ws = ws.controller.active_workspace;
    match ws.admin_ui.active_section {
        SECTION_USERS if ws.admin_ui.users_loaded_ws != Some(active_ws) => {
            refresh_users_roles(ws);
        }
        SECTION_SESSIONS if ws.admin_ui.sessions_loaded_ws != Some(active_ws) => {
            refresh_active_sessions(ws);
        }
        SECTION_MONITOR_SESSIONS | SECTION_MONITOR_LOCKS
            if ws.admin_ui.monitor_loaded_ws != Some(active_ws) =>
        {
            ws.admin_ui.monitor_snapshot = None;
            ws.admin_ui.monitor_confirm_kill = None;
            refresh_monitor_snapshot(ws, cx);
        }
        _ => {}
    }
}

fn render_admin_header(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let engine = ws.controller.active_database_engine();

    div()
        .h(px(40.0))
        .px(px(14.0))
        .border_b_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    Icon::new(IconName::ShieldCheck)
                        .size(IconSize::Sm)
                        .color(colors.accent.color),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(s.admin_title),
                )
                .child(
                    div().flex_shrink_0().child(
                        Chip::new()
                            .variant(ChipVariant::Soft)
                            .color(HeroColor::Default)
                            .size(HeroSize::Sm)
                            .child(ChipLabel::new().child(engine.as_str().to_uppercase())),
                    ),
                ),
        )
        .child(
            Button::new("admin-view-close")
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .is_icon_only(true)
                .aria_label(s.close_label)
                .on_press(cx.listener(|ws, _, _, cx| {
                    ws.workspace_pane.close_tool();
                    cx.notify();
                }))
                .child(Icon::new(IconName::X).size(IconSize::Xs)),
        )
        .into_any_element()
}

fn render_admin_tabs(ws: &TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) -> AnyElement {
    let s = &ws.controller.strings;
    let live_session_count = ws
        .admin_ui
        .monitor_snapshot
        .as_ref()
        .map_or(0, |snap| snap.sessions.len());
    let lock_count = ws
        .admin_ui
        .monitor_snapshot
        .as_ref()
        .map_or(0, |snap| snap.locks.len());
    let monitor_loaded = ws.admin_ui.monitor_snapshot.is_some();

    let items = vec![
        TabItem::new("users", s.admin_users_roles),
        TabItem::new("sessions", s.admin_active_sessions),
        TabItem::new(
            "monitor-sessions",
            if monitor_loaded {
                format!("{} ({live_session_count})", s.monitor_live_sessions)
            } else {
                s.monitor_live_sessions.to_owned()
            },
        ),
        TabItem::new(
            "monitor-locks",
            if monitor_loaded {
                format!("{} ({lock_count})", s.monitor_locks_tab)
            } else {
                s.monitor_locks_tab.to_owned()
            },
        ),
        TabItem::new(
            "tasks",
            format!(
                "{} ({})",
                s.admin_scheduled_tasks,
                ws.controller.scheduled_tasks.len()
            ),
        ),
    ];
    let selected = match ws.admin_ui.active_section {
        SECTION_SESSIONS => "sessions",
        SECTION_MONITOR_SESSIONS => "monitor-sessions",
        SECTION_MONITOR_LOCKS => "monitor-locks",
        SECTION_TASKS => "tasks",
        _ => "users",
    };

    let tabs = Tabs::new("admin-section-tabs", items, selected)
        .selected_key(SharedString::from(selected))
        .variant(TabsVariant::Secondary)
        .size(TabsSize::Sm)
        .on_selection_change(cx.listener(|ws, key: &SharedString, _w, cx| {
            ws.admin_ui.active_section = match key.as_ref() {
                "sessions" => SECTION_SESSIONS,
                "monitor-sessions" => SECTION_MONITOR_SESSIONS,
                "monitor-locks" => SECTION_MONITOR_LOCKS,
                "tasks" => SECTION_TASKS,
                _ => SECTION_USERS,
            };
            cx.notify();
        }));

    div()
        .px(px(14.0))
        .py(px(8.0))
        .bg(muted_surface_bg(cx))
        .child(tabs)
        .into_any_element()
}

fn render_users_section(
    ws: &TableMinusWorkspace,
    is_connected: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let toolbar = render_query_section_toolbar(
        "admin-users-refresh",
        s.admin_users_roles,
        s.refresh_label,
        ws.admin_ui.users_rows.len(),
        is_connected,
        cx.listener(|ws, _, _w, cx| {
            refresh_users_roles(ws);
            cx.notify();
        }),
        cx,
    );

    let spec = AdminTableSpec {
        scroll_id: "admin-users-scroll",
        is_connected,
        notice: ws.admin_ui.users_notice.as_deref(),
        error: ws.admin_ui.users_error.as_deref(),
        cols: &ws.admin_ui.users_cols,
        rows: &ws.admin_ui.users_rows,
        kill_pid_col: None,
    };
    render_query_section_body(toolbar, &spec, ws, cx)
}

fn render_sessions_section(
    ws: &TableMinusWorkspace,
    is_connected: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let toolbar = render_query_section_toolbar(
        "admin-sessions-refresh",
        s.admin_active_sessions,
        s.refresh_label,
        ws.admin_ui.sessions_rows.len(),
        is_connected,
        cx.listener(|ws, _, _w, cx| {
            refresh_active_sessions(ws);
            cx.notify();
        }),
        cx,
    );

    let pid_col_idx = find_pid_column_index(&ws.admin_ui.sessions_cols);
    let spec = AdminTableSpec {
        scroll_id: "admin-sessions-scroll",
        is_connected,
        notice: ws.admin_ui.sessions_notice.as_deref(),
        error: ws.admin_ui.sessions_error.as_deref(),
        cols: &ws.admin_ui.sessions_cols,
        rows: &ws.admin_ui.sessions_rows,
        kill_pid_col: Some(pid_col_idx),
    };
    render_query_section_body(toolbar, &spec, ws, cx)
}

fn render_query_section_toolbar(
    btn_id: &'static str,
    title: &'static str,
    refresh_label: &'static str,
    row_count: usize,
    is_connected: bool,
    on_refresh: impl Fn(&herogpui::gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let colors = cx.colors();
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .child(
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(title),
                )
                .child(
                    Chip::new()
                        .variant(ChipVariant::Soft)
                        .color(HeroColor::Default)
                        .size(HeroSize::Sm)
                        .child(ChipLabel::new().child(format!("{row_count}"))),
                ),
        )
        .child(
            Button::new(btn_id)
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .is_disabled(!is_connected)
                .on_press(on_refresh)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .child(Icon::new(IconName::RefreshCw).size(IconSize::Xs))
                        .child(refresh_label),
                ),
        )
        .into_any_element()
}

struct AdminTableSpec<'a> {
    scroll_id: &'static str,
    is_connected: bool,
    notice: Option<&'a str>,
    error: Option<&'a str>,
    cols: &'a [String],
    rows: &'a [Vec<String>],
    kill_pid_col: Option<usize>,
}

fn render_query_section_body(
    toolbar: AnyElement,
    spec: &AdminTableSpec<'_>,
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();

    if !spec.is_connected {
        return render_centered_note(s.no_active_connection, s.no_active_connection_desc, cx);
    }

    let mut root = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .p(px(14.0))
        .gap(px(10.0))
        .child(toolbar);

    if let Some(msg) = spec.notice {
        root = root.child(
            Alert::new(s.admin_title)
                .status(HeroColor::Default)
                .description(msg.to_owned()),
        );
    }
    if let Some(err) = spec.error {
        root = root.child(
            Alert::new(s.query_error_title)
                .status(HeroColor::Danger)
                .description(err.to_owned()),
        );
    }

    if spec.cols.is_empty() {
        return root
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.0))
                    .text_color(colors.muted)
                    .child(s.results_zero_cols_rows),
            )
            .into_any_element();
    }

    let mut table = div()
        .id(spec.scroll_id)
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .rounded(px(PANEL_RADIUS_PX))
        .border_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .flex_col()
        .child(render_admin_table_header(
            spec.cols,
            spec.kill_pid_col.is_some(),
            s.admin_kill_session,
            cx,
        ));

    for (row_idx, row) in spec.rows.iter().enumerate() {
        table = table.child(render_admin_table_row(
            row_idx,
            row,
            spec.kill_pid_col,
            s.admin_kill_session,
            cx,
        ));
    }

    root.child(table).into_any_element()
}

fn render_admin_table_header(
    cols: &[String],
    has_action: bool,
    action_label: &str,
    cx: &App,
) -> AnyElement {
    let colors = cx.colors();
    let mut row = div()
        .w_full()
        .px(px(10.0))
        .py(px(7.0))
        .bg(muted_surface_bg(cx))
        .border_b_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .items_center()
        .gap(px(8.0));

    for col in cols {
        row = row.child(
            div()
                .flex_1()
                .min_w(px(70.0))
                .truncate()
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.foreground)
                .child(col.clone()),
        );
    }
    if has_action {
        row = row.child(
            div()
                .w(px(110.0))
                .flex_shrink_0()
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.foreground)
                .child(action_label.to_owned()),
        );
    }
    row.into_any_element()
}

fn render_admin_table_row(
    row_idx: usize,
    cells: &[String],
    kill_pid_col: Option<usize>,
    kill_label: &'static str,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let mut row = div()
        .w_full()
        .px(px(10.0))
        .py(px(6.0))
        .border_b_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .items_center()
        .gap(px(8.0));
    if let Some(zebra) = row_zebra_bg(row_idx, cx) {
        row = row.bg(zebra);
    }

    for cell in cells {
        row = row.child(
            div()
                .flex_1()
                .min_w(px(70.0))
                .truncate()
                .text_size(px(12.0))
                .text_color(colors.foreground)
                .child(cell.clone()),
        );
    }

    if let Some(pid_idx) = kill_pid_col {
        let pid = cells.get(pid_idx).cloned().unwrap_or_default();
        let can_kill = !pid.trim().is_empty() && pid != "NULL";
        row = row.child(
            div().w(px(110.0)).flex_shrink_0().child(
                Button::new(SharedString::from(format!("admin-kill-{row_idx}")))
                    .variant(Variant::DangerSoft)
                    .size(HeroSize::Sm)
                    .is_disabled(!can_kill)
                    .on_press(cx.listener(move |ws, _, _w, cx| {
                        kill_active_session(ws, &pid, cx);
                    }))
                    .child(kill_label),
            ),
        );
    }

    row.into_any_element()
}

fn render_tasks_section(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let new_open = ws.admin_ui.new_task_open;

    let top_bar = div()
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_size(px(12.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.foreground)
                .child(s.admin_scheduled_tasks),
        )
        .child(
            Button::new("admin-new-task-toggle")
                .variant(if new_open {
                    Variant::Secondary
                } else {
                    Variant::Primary
                })
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    ws.admin_ui.new_task_open = !ws.admin_ui.new_task_open;
                    ws.admin_ui.task_form_error = None;
                    cx.notify();
                }))
                .child(if new_open {
                    s.cancel_label
                } else {
                    s.admin_new_task
                }),
        );

    let mut root = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .p(px(14.0))
        .gap(px(10.0))
        .child(top_bar);

    if new_open {
        root = root.child(render_new_task_form(ws, cx));
    }

    let mut list = div()
        .id("admin-tasks-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(8.0));

    if ws.controller.scheduled_tasks.is_empty() {
        list = list.child(render_centered_note(
            s.admin_scheduled_tasks,
            s.admin_new_task,
            cx,
        ));
    } else {
        for task in &ws.controller.scheduled_tasks {
            list = list.child(render_scheduled_task_card(ws, task, cx));
        }
    }

    root.child(list).into_any_element()
}

fn render_new_task_form(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let kind_items = vec![
        PickerItem::new("sql", "SQL"),
        PickerItem::new("export", s.export),
        PickerItem::new("schema_compare", s.compare_schema),
    ];
    let kind_select = Select::new("admin-task-kind", kind_items)
        .label(s.admin_kind_label)
        .full_width(true)
        .value(Some(SharedString::from(ws.admin_ui.task_kind.clone())))
        .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _w, cx| {
            if let Some(k) = key.as_deref() {
                ws.admin_ui.task_kind = k.to_owned();
                cx.notify();
            }
        }));

    let conn_select = render_task_connection_picker(ws, cx);
    let mut form = div()
        .w_full()
        .p(px(12.0))
        .rounded(px(PANEL_RADIUS_PX))
        .border_1()
        .border_color(hairline_border_color(cx))
        .bg(muted_surface_bg(cx))
        .flex()
        .flex_col()
        .gap(px(COMPACT_GAP_PX));

    if let Some(err) = ws.admin_ui.task_form_error.as_ref() {
        form = form.child(
            Alert::new(s.admin_new_task)
                .status(HeroColor::Danger)
                .description(err.clone()),
        );
    }

    form.child(
        div()
            .flex()
            .items_end()
            .flex_wrap()
            .gap(px(10.0))
            .child(
                div().flex_1().min_w(px(160.0)).child(
                    Input::new(&ws.admin_ui.task_name_input)
                        .label(s.admin_task_name)
                        .placeholder(s.admin_task_name)
                        .full_width(),
                ),
            )
            .child(div().w(px(150.0)).flex_shrink_0().child(kind_select))
            .child(
                div().w(px(130.0)).flex_shrink_0().child(
                    Input::new(&ws.admin_ui.task_interval_input)
                        .label(s.admin_task_interval_min)
                        .placeholder("15")
                        .full_width(),
                ),
            ),
    )
    .child(
        div()
            .flex()
            .items_end()
            .flex_wrap()
            .gap(px(10.0))
            .child(div().w(px(200.0)).flex_shrink_0().child(conn_select))
            .child(
                div().flex_1().min_w(px(180.0)).child(
                    Input::new(&ws.admin_ui.task_sql_input)
                        .label(s.sql_tab)
                        .placeholder("VACUUM;  (or SELECT ... => path.csv)")
                        .full_width(),
                ),
            )
            .child(
                Button::new("admin-task-save-btn")
                    .variant(Variant::Primary)
                    .size(HeroSize::Sm)
                    .on_press(cx.listener(|ws, _, _w, cx| {
                        save_new_scheduled_task(ws, cx);
                    }))
                    .child(s.save),
            ),
    )
    .into_any_element()
}

fn render_task_connection_picker(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let mut items = vec![PickerItem::new("", s.active_connection_fallback)];
    for conn in ws.controller.library.storage().connections.values() {
        items.push(PickerItem::new(conn.id.0.clone(), conn.name.clone()));
    }
    let val = ws.admin_ui.task_connection_id.clone().unwrap_or_default();

    Select::new("admin-task-conn", items)
        .label(s.connections_library)
        .full_width(true)
        .value(Some(SharedString::from(val)))
        .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _w, cx| {
            ws.admin_ui.task_connection_id =
                key.as_deref().filter(|k| !k.is_empty()).map(str::to_owned);
            cx.notify();
        }))
        .into_any_element()
}

fn resolve_task_conn_label(ws: &TableMinusWorkspace, connection_id: &str) -> String {
    let fallback = ws.controller.strings.active_connection_fallback;
    ws.controller
        .library
        .storage()
        .connections
        .values()
        .find(|c| c.id.0 == connection_id)
        .map_or_else(
            || {
                if connection_id.is_empty() {
                    fallback.to_owned()
                } else {
                    connection_id.to_owned()
                }
            },
            |c| c.name.clone(),
        )
}

fn render_task_card_header(
    ws: &TableMinusWorkspace,
    task: &ScheduledTaskRecord,
    conn_label: String,
    history_open: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let task_id_toggle = task.id.clone();

    div()
        .flex()
        .items_center()
        .justify_between()
        .flex_wrap()
        .gap(px(10.0))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .items_center()
                .flex_wrap()
                .gap(px(8.0))
                .child(
                    Switch::new(SharedString::from(format!("task-switch-{}", task.id)))
                        .is_selected(task.enabled)
                        .on_change(cx.listener(move |ws, &enabled: &bool, _w, cx| {
                            ws.controller
                                .toggle_scheduled_task(&task_id_toggle, enabled);
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(task.name.clone()),
                )
                .child(
                    Chip::new()
                        .variant(ChipVariant::Soft)
                        .color(HeroColor::Accent)
                        .size(HeroSize::Sm)
                        .child(ChipLabel::new().child(task.task_kind.clone())),
                )
                .child(
                    Chip::new()
                        .variant(ChipVariant::Soft)
                        .color(HeroColor::Default)
                        .size(HeroSize::Sm)
                        .child(ChipLabel::new().child(format!("{}m", task.interval_minutes))),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(11.5))
                        .text_color(colors.muted)
                        .child(conn_label),
                ),
        )
        .child(render_task_card_actions(
            ws,
            &task.id,
            task.run_history.len(),
            history_open,
            cx,
        ))
        .into_any_element()
}

fn render_scheduled_task_card(
    ws: &TableMinusWorkspace,
    task: &ScheduledTaskRecord,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let font = editor_gpui_font(&ws.controller.settings);
    let history_open = ws.admin_ui.expanded_history_task_id.as_deref() == Some(task.id.as_str());
    let conn_label = resolve_task_conn_label(ws, &task.connection_id);

    let mut card = div()
        .w_full()
        .p(px(12.0))
        .rounded(px(PANEL_RADIUS_PX))
        .border_1()
        .border_color(hairline_border_color(cx))
        .bg(muted_surface_bg(cx))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(render_task_card_header(
            ws,
            task,
            conn_label,
            history_open,
            cx,
        ))
        .child(
            div()
                .px(px(8.0))
                .py(px(5.0))
                .rounded(px(CONTROL_RADIUS_PX))
                .bg(panel_surface_bg(cx))
                .font(font)
                .text_size(px(11.5))
                .text_color(colors.foreground)
                .truncate()
                .child(task.sql_or_target.clone()),
        );

    if history_open {
        card = card.child(render_task_run_history(ws, &task.run_history, cx));
    }
    card.into_any_element()
}

fn render_task_card_actions(
    ws: &TableMinusWorkspace,
    task_id: &str,
    history_len: usize,
    history_open: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let id_run = task_id.to_owned();
    let id_hist = task_id.to_owned();
    let id_del = task_id.to_owned();

    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .flex_wrap()
        .gap(px(6.0))
        .child(
            Button::new(SharedString::from(format!("task-run-{task_id}")))
                .variant(Variant::Primary)
                .size(HeroSize::Sm)
                .on_press(cx.listener(move |ws, _, _w, cx| {
                    run_task_now_with_toast(ws, &id_run, cx);
                }))
                .child(s.admin_run_now),
        )
        .child(
            Button::new(SharedString::from(format!("task-hist-{task_id}")))
                .variant(if history_open {
                    Variant::Primary
                } else {
                    Variant::Secondary
                })
                .size(HeroSize::Sm)
                .on_press(cx.listener(move |ws, _, _w, cx| {
                    ws.admin_ui.expanded_history_task_id = if history_open {
                        None
                    } else {
                        Some(id_hist.clone())
                    };
                    cx.notify();
                }))
                .child(format!("{} ({history_len})", s.admin_task_history)),
        )
        .child(
            Button::new(SharedString::from(format!("task-del-{task_id}")))
                .variant(Variant::DangerSoft)
                .size(HeroSize::Sm)
                .on_press(cx.listener(move |ws, _, _w, cx| {
                    ws.controller.delete_scheduled_task(&id_del);
                    if ws.admin_ui.expanded_history_task_id.as_deref() == Some(id_del.as_str()) {
                        ws.admin_ui.expanded_history_task_id = None;
                    }
                    cx.notify();
                }))
                .child(s.delete),
        )
        .into_any_element()
}

fn render_task_run_history(
    ws: &TableMinusWorkspace,
    entries: &[ScheduledTaskRunEntry],
    cx: &App,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors();
    let mut box_el = div()
        .w_full()
        .pt(px(4.0))
        .border_t_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .flex_col()
        .gap(px(4.0));

    if entries.is_empty() {
        return box_el
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(colors.muted)
                    .child("—"),
            )
            .into_any_element();
    }

    for entry in entries.iter().rev() {
        let status_label = if entry.success {
            s.status_ok
        } else {
            s.status_error
        };
        let status_color = if entry.success {
            HeroColor::Success
        } else {
            HeroColor::Danger
        };
        box_el = box_el.child(
            div()
                .w_full()
                .px(px(8.0))
                .py(px(4.0))
                .rounded(px(CONTROL_RADIUS_PX))
                .bg(panel_surface_bg(cx))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            Chip::new()
                                .variant(ChipVariant::Soft)
                                .color(status_color)
                                .size(HeroSize::Sm)
                                .child(ChipLabel::new().child(status_label)),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(colors.muted)
                                .child(format_unix_utc(entry.ran_at_unix)),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_size(px(11.5))
                                .text_color(colors.foreground)
                                .child(entry.summary.clone()),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(11.0))
                        .text_color(colors.muted)
                        .child(format!("{} ms", entry.duration_ms)),
                ),
        );
    }

    box_el.into_any_element()
}

fn render_centered_note(title: &str, subtitle: &str, cx: &App) -> AnyElement {
    let colors = cx.colors();
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(4.0))
        .p(px(24.0))
        .child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.foreground)
                .child(title.to_owned()),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(colors.muted)
                .child(subtitle.to_owned()),
        )
        .into_any_element()
}

fn refresh_users_roles(ws: &mut TableMinusWorkspace) {
    ws.admin_ui.users_loaded_ws = Some(ws.controller.active_workspace);
    ws.admin_ui.users_error = None;
    ws.admin_ui.users_notice = None;
    let engine = ws.controller.active_database_engine();

    let Some(sql) = users_roles_query(engine) else {
        ws.admin_ui.users_cols.clear();
        ws.admin_ui.users_rows.clear();
        ws.admin_ui.users_notice = Some(embedded_engine_admin_message(
            engine,
            &ws.controller.strings,
        ));
        return;
    };

    match ws.controller.execute_query_capture_blocking(sql) {
        Ok((cols, rows)) => {
            ws.admin_ui.users_cols = cols.into_iter().map(|c| c.name).collect();
            ws.admin_ui.users_rows = rows
                .into_iter()
                .map(|r| r.iter().map(format_core_value).collect())
                .collect();
        }
        Err(err) => {
            ws.admin_ui.users_error = Some(err.to_string());
        }
    }
}

fn refresh_active_sessions(ws: &mut TableMinusWorkspace) {
    ws.admin_ui.sessions_loaded_ws = Some(ws.controller.active_workspace);
    ws.admin_ui.sessions_error = None;
    ws.admin_ui.sessions_notice = None;
    let engine = ws.controller.active_database_engine();

    let Some(sql) = active_sessions_query(engine) else {
        ws.admin_ui.sessions_cols.clear();
        ws.admin_ui.sessions_rows.clear();
        ws.admin_ui.sessions_notice = Some(embedded_engine_admin_message(
            engine,
            &ws.controller.strings,
        ));
        return;
    };

    match ws.controller.execute_query_capture_blocking(sql) {
        Ok((cols, rows)) => {
            ws.admin_ui.sessions_cols = cols.into_iter().map(|c| c.name).collect();
            ws.admin_ui.sessions_rows = rows
                .into_iter()
                .map(|r| r.iter().map(format_core_value).collect())
                .collect();
        }
        Err(err) => {
            ws.admin_ui.sessions_error = Some(err.to_string());
        }
    }
}

fn kill_active_session(
    ws: &mut TableMinusWorkspace,
    pid: &str,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let engine = ws.controller.active_database_engine();
    let title = ws.controller.strings.admin_kill_session;
    let session_prefix = ws.controller.strings.admin_session_prefix;
    let failed_label = ws.controller.strings.failed;
    let Some(sql) = kill_session_sql(engine, pid) else {
        let detail = format!("{failed_label}: {session_prefix} {pid}");
        push_error_toast(title, Some(detail.into()), cx);
        return;
    };
    match ws.controller.execute_ddl_statements_blocking(&[sql]) {
        Ok(_) => {
            let detail = format!("{session_prefix} {pid}");
            push_success_toast(title, Some(detail.into()), cx);
            refresh_active_sessions(ws);
        }
        Err(err) => {
            push_error_toast(title, Some(err.to_string().into()), cx);
        }
    }
    cx.notify();
}

fn save_new_scheduled_task(ws: &mut TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) {
    let name = ws
        .admin_ui
        .task_name_input
        .read(cx)
        .value()
        .trim()
        .to_owned();
    let sql_or_target = ws
        .admin_ui
        .task_sql_input
        .read(cx)
        .value()
        .trim()
        .to_owned();
    let interval_raw = ws
        .admin_ui
        .task_interval_input
        .read(cx)
        .value()
        .trim()
        .to_owned();

    let interval_minutes = match parse_task_inputs(&name, &sql_or_target, &interval_raw) {
        Ok(mins) => mins,
        Err(()) => {
            ws.admin_ui.task_form_error =
                Some(ws.controller.strings.admin_task_validation_err.to_owned());
            cx.notify();
            return;
        }
    };

    let connection_id = ws
        .admin_ui
        .task_connection_id
        .clone()
        .or_else(|| ws.controller.active_connection_id().map(str::to_owned))
        .unwrap_or_default();

    let record = ScheduledTaskRecord {
        id: format!("task-{}", uuid::Uuid::new_v4()),
        name,
        connection_id,
        task_kind: ws.admin_ui.task_kind.clone(),
        sql_or_target,
        interval_minutes,
        enabled: true,
        last_run_unix: None,
        last_status: None,
        last_duration_ms: None,
        run_history: Vec::new(),
    };
    ws.controller.upsert_scheduled_task(record);
    ws.admin_ui.task_name_input.update(cx, |i, cx| {
        i.set_value(String::new());
        cx.notify();
    });
    ws.admin_ui.task_sql_input.update(cx, |i, cx| {
        i.set_value(String::new());
        cx.notify();
    });
    ws.admin_ui.task_form_error = None;
    ws.admin_ui.new_task_open = false;
    let msg = ws.controller.strings.admin_new_task;
    push_success_toast(msg, None, cx);
    cx.notify();
}

fn run_task_now_with_toast(
    ws: &mut TableMinusWorkspace,
    id: &str,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let title = ws.controller.strings.admin_run_now;
    match ws.controller.run_scheduled_task_now(id) {
        Ok(entry) => {
            push_success_toast(title, Some(entry.summary.into()), cx);
        }
        Err(err) => {
            push_error_toast(title, Some(err.to_string().into()), cx);
        }
    }
    cx.notify();
}

fn render_monitor_sessions_section(
    ws: &TableMinusWorkspace,
    is_connected: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    if !is_connected {
        return render_centered_note(s.no_active_connection, s.no_active_connection_desc, cx);
    }

    let sessions = ws
        .admin_ui
        .monitor_snapshot
        .as_ref()
        .map_or(&[][..], |snap| snap.sessions.as_slice());
    let toolbar = render_monitor_toolbar(
        ws,
        s.monitor_live_sessions,
        sessions.len(),
        is_connected,
        cx,
    );

    let mut root = render_monitor_section_shell(ws, toolbar, true, cx);
    if ws.admin_ui.monitor_notice.is_some() && sessions.is_empty() {
        return root.into_any_element();
    }
    if sessions.is_empty() {
        return root
            .child(render_monitor_empty_state(
                IconName::Activity,
                s.monitor_no_sessions,
                cx,
            ))
            .into_any_element();
    }

    let engine = ws.controller.active_database_engine();
    let mut table = div()
        .id("admin-monitor-sessions-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .overflow_x_scroll()
        .rounded(px(PANEL_RADIUS_PX))
        .border_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .flex_col()
        .child(render_live_sessions_header(ws, cx));

    for (row_idx, session) in sessions.iter().enumerate() {
        table = table.child(render_live_session_row(row_idx, session, engine, ws, cx));
    }

    root = root.child(table);
    root.into_any_element()
}

fn render_monitor_locks_section(
    ws: &TableMinusWorkspace,
    is_connected: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    if !is_connected {
        return render_centered_note(s.no_active_connection, s.no_active_connection_desc, cx);
    }

    let locks = ws
        .admin_ui
        .monitor_snapshot
        .as_ref()
        .map_or(&[][..], |snap| snap.locks.as_slice());
    let toolbar = render_monitor_toolbar(ws, s.monitor_locks_tab, locks.len(), is_connected, cx);

    let mut root = render_monitor_section_shell(ws, toolbar, false, cx);
    if ws.admin_ui.monitor_notice.is_some() && locks.is_empty() {
        return root.into_any_element();
    }
    if locks.is_empty() {
        return root
            .child(render_monitor_empty_state(
                IconName::Lock,
                s.monitor_no_locks,
                cx,
            ))
            .into_any_element();
    }

    let mut table = div()
        .id("admin-monitor-locks-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .overflow_x_scroll()
        .rounded(px(PANEL_RADIUS_PX))
        .border_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .flex_col()
        .child(render_locks_header(ws, cx));

    for (row_idx, lock) in locks.iter().enumerate() {
        table = table.child(render_server_lock_row(row_idx, lock, ws, cx));
    }

    root = root.child(table);
    root.into_any_element()
}

fn render_monitor_section_shell(
    ws: &TableMinusWorkspace,
    toolbar: AnyElement,
    show_kill_confirm: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> herogpui::gpui::Div {
    let s = &ws.controller.strings;
    let mut root = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .p(px(14.0))
        .gap(px(10.0))
        .child(toolbar);

    if let Some(msg) = ws.admin_ui.monitor_notice.as_deref() {
        root = root.child(
            Alert::new(s.admin_title)
                .status(HeroColor::Default)
                .description(msg.to_owned()),
        );
    }
    if let Some(err) = ws.admin_ui.monitor_error.as_deref() {
        root = root.child(
            Alert::new(s.query_error_title)
                .status(HeroColor::Danger)
                .description(err.to_owned()),
        );
    }
    if show_kill_confirm && let Some(sheet) = render_monitor_confirm_kill_guard(ws, cx) {
        root = root.child(sheet);
    }
    root
}

fn render_monitor_toolbar(
    ws: &TableMinusWorkspace,
    title: &'static str,
    row_count: usize,
    is_connected: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let loading = ws.admin_ui.monitor_loading;
    let auto_on = ws.admin_ui.monitor_auto_refresh;

    let mut left = div()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .truncate()
                .text_size(px(12.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.foreground)
                .child(title),
        )
        .child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .color(HeroColor::Default)
                .size(HeroSize::Sm)
                .child(ChipLabel::new().child(format!("{row_count}"))),
        );

    if loading {
        left = left.child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .color(HeroColor::Accent)
                .size(HeroSize::Sm)
                .child(ChipLabel::new().child(s.running)),
        );
    }

    let right = div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(12.0))
        .child(
            Switch::new("monitor-auto-refresh")
                .size(HeroSize::Sm)
                .is_selected(auto_on)
                .is_disabled(!is_connected)
                .label(s.monitor_auto_refresh)
                .on_change(cx.listener(|ws, &enabled: &bool, _w, cx| {
                    toggle_monitor_auto_refresh(ws, enabled, cx);
                })),
        )
        .child(
            Button::new("monitor-refresh-btn")
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .is_disabled(!is_connected || loading)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    refresh_monitor_snapshot(ws, cx);
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .child(Icon::new(IconName::RefreshCw).size(IconSize::Xs))
                        .child(s.refresh_label),
                ),
        );

    div()
        .flex()
        .items_center()
        .justify_between()
        .flex_wrap()
        .gap(px(8.0))
        .child(left)
        .child(right)
        .into_any_element()
}

fn render_monitor_confirm_kill_guard(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> Option<AnyElement> {
    let pending = ws.admin_ui.monitor_confirm_kill.as_ref()?;
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let mono = value_gpui_font(&ws.controller.settings);
    let killing = ws.admin_ui.monitor_killing;

    let user_text = if pending.user.is_empty() {
        "—"
    } else {
        pending.user.as_str()
    };
    let query_text = if pending.query.is_empty() {
        "—"
    } else {
        pending.query.as_str()
    };
    let detail = format!(
        "{}: {} · {}: {} · {}",
        s.monitor_pid_col, pending.id, s.monitor_user_col, user_text, query_text
    );

    Some(
        div()
            .w_full()
            .p(px(12.0))
            .rounded(px(PANEL_RADIUS_PX))
            .bg(elevated_overlay_bg(cx))
            .border_1()
            .border_color(colors.danger.color.opacity(0.45))
            .flex()
            .items_center()
            .justify_between()
            .flex_wrap()
            .gap(px(10.0))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .size(IconSize::Sm)
                            .color(colors.danger.color),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(colors.foreground)
                                    .child(s.monitor_confirm_kill),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .font(mono)
                                    .text_size(px(11.5))
                                    .text_color(colors.muted)
                                    .child(detail),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        Button::new("monitor-confirm-kill-cancel")
                            .variant(Variant::Ghost)
                            .size(HeroSize::Sm)
                            .is_disabled(killing)
                            .on_press(cx.listener(|ws, _, _w, cx| {
                                ws.admin_ui.monitor_confirm_kill = None;
                                cx.notify();
                            }))
                            .child(s.cancel_label),
                    )
                    .child(
                        Button::new("monitor-confirm-kill-exec")
                            .variant(Variant::Danger)
                            .size(HeroSize::Sm)
                            .is_disabled(killing)
                            .on_press(cx.listener(|ws, _, _w, cx| {
                                execute_confirm_kill_session(ws, cx);
                            }))
                            .child(s.monitor_kill_session),
                    ),
            )
            .into_any_element(),
    )
}

fn render_live_sessions_header(ws: &TableMinusWorkspace, cx: &App) -> AnyElement {
    let s = &ws.controller.strings;
    div()
        .w_full()
        .px(px(10.0))
        .py(px(7.0))
        .bg(muted_surface_bg(cx))
        .border_b_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            header_cell(s.monitor_pid_col, cx)
                .w(px(96.0))
                .flex_shrink_0(),
        )
        .child(
            header_cell(s.monitor_user_col, cx)
                .w(px(110.0))
                .flex_shrink_0(),
        )
        .child(
            header_cell(s.monitor_state_col, cx)
                .w(px(120.0))
                .flex_shrink_0(),
        )
        .child(
            header_cell(s.monitor_duration_col, cx)
                .w(px(88.0))
                .flex_shrink_0(),
        )
        .child(
            header_cell(s.monitor_query_col, cx)
                .flex_1()
                .min_w(px(180.0)),
        )
        .child(
            header_cell(s.monitor_kill_session, cx)
                .w(px(124.0))
                .flex_shrink_0(),
        )
        .into_any_element()
}

fn render_live_session_row(
    row_idx: usize,
    session: &LiveSessionRow,
    engine: DatabaseEngine,
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let mono_font = value_gpui_font(&ws.controller.settings);
    let sql_font = editor_gpui_font(&ws.controller.settings);
    let can_kill = !ws.admin_ui.monitor_killing
        && server_monitor::validate_session_id(engine, &session.id).is_ok();
    let is_armed = ws
        .admin_ui
        .monitor_confirm_kill
        .as_ref()
        .is_some_and(|pending| pending.id == session.id);

    let pending_payload = PendingKillSession {
        id: session.id.clone(),
        user: session.user.clone(),
        query: session.query.clone(),
    };
    let state_text = dash_if_empty(&session.state);
    let state_color = session_state_chip_color(&session.state);
    let user_text = dash_if_empty(&session.user);
    let query_text = dash_if_empty(&session.query);

    let mut row = div()
        .w_full()
        .px(px(10.0))
        .py(px(6.0))
        .border_b_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .items_center()
        .gap(px(8.0));
    if is_armed {
        row = row.bg(colors.danger.color.opacity(0.08));
    } else if let Some(zebra) = row_zebra_bg(row_idx, cx) {
        row = row.bg(zebra);
    }

    row.child(
        div()
            .w(px(96.0))
            .flex_shrink_0()
            .truncate()
            .font(mono_font.clone())
            .text_size(px(12.0))
            .text_color(colors.foreground)
            .child(session.id.clone()),
    )
    .child(
        div()
            .w(px(110.0))
            .flex_shrink_0()
            .truncate()
            .text_size(px(12.0))
            .text_color(colors.foreground)
            .child(user_text),
    )
    .child(
        div().w(px(120.0)).flex_shrink_0().min_w_0().child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .color(state_color)
                .size(HeroSize::Sm)
                .child(ChipLabel::new().child(state_text)),
        ),
    )
    .child(
        div()
            .w(px(88.0))
            .flex_shrink_0()
            .truncate()
            .font(mono_font)
            .text_size(px(12.0))
            .text_color(colors.muted)
            .child(format_monitor_duration(session.duration_ms)),
    )
    .child(
        div()
            .flex_1()
            .min_w(px(180.0))
            .truncate()
            .font(sql_font)
            .text_size(px(11.5))
            .text_color(colors.foreground)
            .child(query_text),
    )
    .child(
        div().w(px(124.0)).flex_shrink_0().child(
            Button::new(SharedString::from(format!("monitor-kill-{row_idx}")))
                .variant(if is_armed {
                    Variant::Danger
                } else {
                    Variant::DangerSoft
                })
                .size(HeroSize::Sm)
                .is_disabled(!can_kill)
                .on_press(cx.listener(move |ws, _, _w, cx| {
                    ws.admin_ui.monitor_confirm_kill = Some(pending_payload.clone());
                    cx.notify();
                }))
                .child(s.monitor_kill_session),
        ),
    )
    .into_any_element()
}

fn render_locks_header(ws: &TableMinusWorkspace, cx: &App) -> AnyElement {
    let s = &ws.controller.strings;
    div()
        .w_full()
        .px(px(10.0))
        .py(px(7.0))
        .bg(muted_surface_bg(cx))
        .border_b_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            header_cell(s.monitor_pid_col, cx)
                .w(px(100.0))
                .flex_shrink_0(),
        )
        .child(
            header_cell(s.monitor_relation_col, cx)
                .flex_1()
                .min_w(px(200.0)),
        )
        .child(
            header_cell(s.monitor_granted_col, cx)
                .w(px(140.0))
                .flex_shrink_0(),
        )
        .child(
            header_cell(s.monitor_blocked_by_col, cx)
                .w(px(130.0))
                .flex_shrink_0(),
        )
        .into_any_element()
}

fn dash_if_empty(value: &str) -> String {
    if value.is_empty() {
        "—".to_owned()
    } else {
        value.to_owned()
    }
}

fn render_lock_granted_chip(lock: &ServerLockRow, s: &crate::i18n::Strings) -> Chip {
    if lock.granted {
        Chip::new()
            .variant(ChipVariant::Soft)
            .color(HeroColor::Success)
            .size(HeroSize::Sm)
            .child(
                ChipLabel::new().child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(Icon::new(IconName::Check).size(IconSize::Xs))
                        .child(s.nullable_yes),
                ),
            )
    } else {
        Chip::new()
            .variant(ChipVariant::Soft)
            .color(HeroColor::Warning)
            .size(HeroSize::Sm)
            .child(
                ChipLabel::new().child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(Icon::new(IconName::Lock).size(IconSize::Xs))
                        .child(dash_if_empty(&lock.mode)),
                ),
            )
    }
}

fn render_lock_blocked_cell(
    blocked_by: Option<&str>,
    mono_font: herogpui::gpui::Font,
    muted: herogpui::gpui::Hsla,
) -> AnyElement {
    match blocked_by.filter(|b| !b.is_empty()) {
        Some(blocker) => div()
            .w(px(130.0))
            .flex_shrink_0()
            .child(
                Chip::new()
                    .variant(ChipVariant::Soft)
                    .color(HeroColor::Danger)
                    .size(HeroSize::Sm)
                    .child(ChipLabel::new().child(blocker.to_owned())),
            )
            .into_any_element(),
        None => div()
            .w(px(130.0))
            .flex_shrink_0()
            .font(mono_font)
            .text_size(px(12.0))
            .text_color(muted)
            .child("—")
            .into_any_element(),
    }
}

fn render_server_lock_row(
    row_idx: usize,
    lock: &ServerLockRow,
    ws: &TableMinusWorkspace,
    cx: &App,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors();
    let mono_font = value_gpui_font(&ws.controller.settings);

    let relation_text = dash_if_empty(&lock.relation);
    let meta_parts: Vec<&str> = [lock.lock_type.as_str(), lock.mode.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect();
    let meta_text = meta_parts.join(" · ");
    let granted_chip = render_lock_granted_chip(lock, s);
    let blocked_el =
        render_lock_blocked_cell(lock.blocked_by.as_deref(), mono_font.clone(), colors.muted);

    let mut row = div()
        .w_full()
        .px(px(10.0))
        .py(px(6.0))
        .border_b_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .items_center()
        .gap(px(8.0));
    if let Some(zebra) = row_zebra_bg(row_idx, cx) {
        row = row.bg(zebra);
    }

    let mut rel_cell = div()
        .flex_1()
        .min_w(px(200.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(12.0))
                .text_color(colors.foreground)
                .child(relation_text),
        );
    if !meta_text.is_empty() {
        rel_cell = rel_cell.child(
            div()
                .flex_shrink_0()
                .font(mono_font.clone())
                .text_size(px(11.0))
                .text_color(colors.muted)
                .child(meta_text),
        );
    }

    row.child(
        div()
            .w(px(100.0))
            .flex_shrink_0()
            .truncate()
            .font(mono_font)
            .text_size(px(12.0))
            .text_color(colors.foreground)
            .child(lock.pid.clone()),
    )
    .child(rel_cell)
    .child(div().w(px(140.0)).flex_shrink_0().child(granted_chip))
    .child(blocked_el)
    .into_any_element()
}

fn header_cell(label: &'static str, cx: &App) -> herogpui::gpui::Div {
    div()
        .truncate()
        .text_size(px(11.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.colors().foreground)
        .child(label)
}

fn render_monitor_empty_state(icon: IconName, message: &'static str, cx: &App) -> AnyElement {
    let colors = cx.colors();
    div()
        .flex_1()
        .min_h_0()
        .rounded(px(PANEL_RADIUS_PX))
        .border_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .p(px(24.0))
        .child(Icon::new(icon).size(IconSize::Md).color(colors.muted))
        .child(
            div()
                .text_size(px(12.5))
                .text_color(colors.muted)
                .child(message),
        )
        .into_any_element()
}

fn refresh_monitor_snapshot(ws: &mut TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) {
    let active_ws = ws.controller.active_workspace;
    ws.admin_ui.monitor_loaded_ws = Some(active_ws);
    ws.admin_ui.monitor_error = None;
    ws.admin_ui.monitor_notice = None;
    ws.admin_ui.monitor_fetch_gen = ws.admin_ui.monitor_fetch_gen.wrapping_add(1);
    ws.admin_ui.monitor_fetch_task = None;
    let engine = ws.controller.active_database_engine();

    if server_monitor::sessions_query(engine).is_none() {
        ws.admin_ui.monitor_loading = false;
        ws.admin_ui.monitor_snapshot = Some(ServerMonitorSnapshot {
            engine,
            sessions: Vec::new(),
            locks: Vec::new(),
            summary: server_monitor::format_snapshot_summary(engine, &[], &[]),
        });
        ws.admin_ui.monitor_notice = Some(embedded_engine_admin_message(
            engine,
            &ws.controller.strings,
        ));
        return;
    }

    let Some(services) = ws.controller.services.clone() else {
        ws.admin_ui.monitor_loading = false;
        ws.admin_ui.monitor_error = Some(ws.controller.strings.no_db_service_active.to_owned());
        return;
    };

    let generation = ws.admin_ui.monitor_fetch_gen;
    let no_session_err = ws.controller.strings.no_active_session_err.to_owned();
    ws.admin_ui.monitor_loading = true;

    ws.admin_ui.monitor_fetch_task = Some(cx.spawn(
        async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let sessions = services.sessions.clone();
            drop(services.runtime.spawn(async move {
                let session = {
                    let guard = sessions.read().await;
                    guard.session(active_ws)
                };
                let result = match session {
                    Some(session) => {
                        server_monitor::fetch_server_monitor_snapshot(&session, engine)
                            .await
                            .map_err(|err| err.to_string())
                    }
                    None => Err(no_session_err),
                };
                let _ = tx.send(result);
            }));

            if let Ok(result) = rx.await {
                let _ = this.update(cx, |ws, cx| {
                    if ws.admin_ui.monitor_fetch_gen != generation
                        || ws.controller.active_workspace != active_ws
                    {
                        return;
                    }
                    ws.admin_ui.monitor_loading = false;
                    match result {
                        Ok(snapshot) => {
                            ws.admin_ui.monitor_snapshot = Some(snapshot);
                            ws.admin_ui.monitor_error = None;
                        }
                        Err(err) => {
                            ws.admin_ui.monitor_error = Some(err);
                        }
                    }
                    cx.notify();
                });
            }
        },
    ));
}

fn toggle_monitor_auto_refresh(
    ws: &mut TableMinusWorkspace,
    enabled: bool,
    cx: &mut Context<TableMinusWorkspace>,
) {
    ws.admin_ui.monitor_auto_refresh = enabled;
    ws.admin_ui.monitor_auto_refresh_gen = ws.admin_ui.monitor_auto_refresh_gen.wrapping_add(1);
    if !enabled {
        ws.admin_ui.monitor_auto_refresh_task = None;
        cx.notify();
        return;
    }

    let generation = ws.admin_ui.monitor_auto_refresh_gen;
    if !ws.admin_ui.monitor_loading {
        refresh_monitor_snapshot(ws, cx);
    }

    ws.admin_ui.monitor_auto_refresh_task = Some(cx.spawn(
        async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
            loop {
                cx.background_executor()
                    .timer(MONITOR_AUTO_REFRESH_INTERVAL)
                    .await;
                let keep_going = this
                    .update(cx, |ws, cx| {
                        if !ws.admin_ui.monitor_auto_refresh
                            || ws.admin_ui.monitor_auto_refresh_gen != generation
                        {
                            return false;
                        }
                        let on_monitor_tab = matches!(
                            ws.admin_ui.active_section,
                            SECTION_MONITOR_SESSIONS | SECTION_MONITOR_LOCKS
                        );
                        let admin_open =
                            ws.workspace_pane.active_tool == Some(super::WorkspaceToolView::Admin);
                        let connected = matches!(
                            ws.controller
                                .connections
                                .phase(ws.controller.active_workspace),
                            Some(ConnectionWindowPhase::Connected)
                        );
                        if admin_open
                            && on_monitor_tab
                            && connected
                            && !ws.admin_ui.monitor_loading
                            && !ws.admin_ui.monitor_killing
                            && ws.admin_ui.monitor_confirm_kill.is_none()
                        {
                            refresh_monitor_snapshot(ws, cx);
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        },
    ));
    cx.notify();
}

fn execute_confirm_kill_session(ws: &mut TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) {
    if ws.admin_ui.monitor_killing {
        return;
    }
    let Some(pending) = ws.admin_ui.monitor_confirm_kill.clone() else {
        return;
    };
    let toast_title = ws.controller.strings.monitor_kill_session;
    let Some(services) = ws.controller.services.clone() else {
        push_error_toast(
            toast_title,
            Some(ws.controller.strings.no_db_service_active.into()),
            cx,
        );
        return;
    };

    let active_ws = ws.controller.active_workspace;
    let engine = ws.controller.active_database_engine();
    let session_prefix = ws.controller.strings.admin_session_prefix;
    let no_session_err = ws.controller.strings.no_active_session_err.to_owned();
    ws.admin_ui.monitor_killing = true;
    cx.notify();

    ws.admin_ui.monitor_kill_task = Some(cx.spawn(
        async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let pid_for_task = pending.id.clone();
            let sessions = services.sessions.clone();
            drop(services.runtime.spawn(async move {
                let session = {
                    let guard = sessions.read().await;
                    guard.session(active_ws)
                };
                let result = match session {
                    Some(session) => {
                        server_monitor::terminate_server_session(&session, engine, &pid_for_task)
                            .await
                            .map_err(|err| err.to_string())
                    }
                    None => Err(no_session_err),
                };
                let _ = tx.send(result);
            }));

            if let Ok(result) = rx.await {
                let _ = this.update(cx, |ws, cx| {
                    ws.admin_ui.monitor_killing = false;
                    match result {
                        Ok(()) => {
                            ws.admin_ui.monitor_confirm_kill = None;
                            let detail = format!("{session_prefix} {}", pending.id);
                            push_success_toast(toast_title, Some(detail.into()), cx);
                            refresh_monitor_snapshot(ws, cx);
                        }
                        Err(err) => {
                            push_error_toast(toast_title, Some(err.into()), cx);
                        }
                    }
                    cx.notify();
                });
            }
        },
    ));
}

fn format_monitor_duration(duration_ms: Option<u64>) -> String {
    let Some(ms) = duration_ms else {
        return "—".to_owned();
    };
    if ms < 1_000 {
        return format!("{ms} ms");
    }
    if ms < 60_000 {
        let secs = ms / 1_000;
        let tenths = (ms % 1_000) / 100;
        return format!("{secs}.{tenths} s");
    }
    let total_secs = ms / 1_000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{mins}m {secs:02}s")
}

fn session_state_chip_color(state: &str) -> HeroColor {
    let lower = state.trim().to_ascii_lowercase();
    if lower.is_empty() || lower == "idle" || lower == "sleep" || lower == "sleeping" {
        HeroColor::Default
    } else if lower.contains("wait") || lower.contains("lock") || lower.contains("block") {
        HeroColor::Warning
    } else if lower.contains("cancel") || lower.contains("kill") || lower.contains("fail") {
        HeroColor::Danger
    } else {
        HeroColor::Accent
    }
}

fn parse_task_inputs(name: &str, sql_or_target: &str, interval_raw: &str) -> Result<u32, ()> {
    if name.is_empty() || sql_or_target.is_empty() {
        return Err(());
    }
    let mins = interval_raw.parse::<u32>().map_err(|_| ())?;
    if mins == 0 {
        return Err(());
    }
    Ok(mins)
}

fn find_pid_column_index(cols: &[String]) -> usize {
    cols.iter()
        .position(|c| c.eq_ignore_ascii_case("pid"))
        .unwrap_or(0)
}

fn embedded_engine_admin_message(engine: DatabaseEngine, s: &crate::i18n::Strings) -> String {
    format!("{} ({})", s.admin_embedded_notice, engine.as_str())
}

fn format_unix_utc(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86_400);
    let secs_of_day = unix_secs.rem_euclid(86_400);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;

    // Howard Hinnant's civil-from-days algorithm.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(m <= 2);

    format!("{year:04}-{m:02}-{d:02} {hour:02}:{minute:02}:{second:02} UTC")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_input_validation_rejects_empty_and_zero_interval() {
        assert!(parse_task_inputs("", "VACUUM;", "15").is_err());
        assert!(parse_task_inputs("Vacuum", "", "15").is_err());
        assert!(parse_task_inputs("Vacuum", "VACUUM;", "0").is_err());
        assert!(parse_task_inputs("Vacuum", "VACUUM;", "abc").is_err());
        assert_eq!(parse_task_inputs("Vacuum", "VACUUM;", "30"), Ok(30));
    }

    #[test]
    fn unix_timestamp_formats_as_utc_calendar_date() {
        assert_eq!(format_unix_utc(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(format_unix_utc(1_763_180_100), "2025-11-15 04:15:00 UTC");
    }

    #[test]
    fn monitor_duration_formats_compact_ranges() {
        assert_eq!(format_monitor_duration(None), "—");
        assert_eq!(format_monitor_duration(Some(0)), "0 ms");
        assert_eq!(format_monitor_duration(Some(845)), "845 ms");
        assert_eq!(format_monitor_duration(Some(1_450)), "1.4 s");
        assert_eq!(format_monitor_duration(Some(125_000)), "2m 05s");
    }

    #[test]
    fn monitor_state_maps_to_semantic_chip_colors() {
        assert_eq!(session_state_chip_color("active"), HeroColor::Accent);
        assert_eq!(session_state_chip_color("idle"), HeroColor::Default);
        assert_eq!(
            session_state_chip_color("idle in transaction"),
            HeroColor::Accent
        );
        assert_eq!(session_state_chip_color("LockWait"), HeroColor::Warning);
        assert_eq!(session_state_chip_color("cancelled"), HeroColor::Danger);
    }
}
