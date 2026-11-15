//! Global metadata search modal for schemas, tables, views, routines, and columns.

use std::time::Duration;

use herogpui::components::{Alert, Button, Icon, IconName, IconSize, InputState, SearchField};
use herogpui::core::{Color as HeroColor, Size as HeroSize, Variant, with_alpha};
use herogpui::gpui::prelude::FluentBuilder as _;
use herogpui::gpui::{
    Animation, AnimationExt as _, AnyElement, App, AppContext as _, AsyncApp, Context, Entity,
    Focusable as _, FontWeight, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, Task, WeakEntity, Window, div,
    px,
};
use herogpui::theme::ActiveTheme;
use tableminus_state::connection_window::{ConnectionWindowPhase, WorkspaceId};
use tableminus_state::tabs::TabConnectionId;
use tablepro_core::ddl::TableDefinition;
use tablepro_core::meta::{ObjectInfo, ObjectKind, TableRef};

use super::shell_state::{ToastSeverity, WorkspaceSubView};
use super::theme::{ease_out_cubic, ease_out_quint, effective_duration_ms, hairline_border_color};
use super::{TableMinusWorkspace, TableViewMode};
use crate::app::ScreenMode;

const MAX_DISPLAYED_RESULTS: usize = 120;

/// Filter category pills in the global Metadata Search modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MetadataFilterKind {
    #[default]
    All,
    Tables,
    Views,
    Columns,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SearchResultKind {
    Table,
    View,
    Routine,
    Other,
    Column,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchResultEntry {
    kind: SearchResultKind,
    title: String,
    subtitle: String,
    detail: String,
    badge: Option<String>,
    target_table: Option<TableRef>,
    qualified_name: String,
}

/// Persistent UI state for the global Metadata Search spotlight palette.
#[derive(Debug)]
pub struct MetadataSearchState {
    pub open: bool,
    closing: bool,
    anim_gen: u64,
    close_task: Option<Task<()>>,
    pub query: String,
    filter: MetadataFilterKind,
    selected_index: usize,
    query_input: Entity<InputState>,
    cached_defs: Vec<TableDefinition>,
    loaded_for: Option<(WorkspaceId, usize)>,
    last_error: Option<String>,
}

impl MetadataSearchState {
    /// Creates initial Metadata Search palette state owned by [`TableMinusWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, cx: &mut Context<TableMinusWorkspace>) -> Self {
        Self {
            open: false,
            closing: false,
            anim_gen: 0,
            close_task: None,
            query: String::new(),
            filter: MetadataFilterKind::All,
            selected_index: 0,
            query_input: cx.new(|cx| InputState::new(cx)),
            cached_defs: Vec::new(),
            loaded_for: None,
            last_error: None,
        }
    }

    /// Opens the Metadata Search palette and focuses the search input.
    pub fn open_modal(&mut self, window: &mut Window, cx: &mut Context<TableMinusWorkspace>) {
        self.close_task = None;
        self.closing = false;
        if !self.open {
            self.anim_gen = self.anim_gen.wrapping_add(1);
        }
        self.open = true;
        self.selected_index = 0;
        self.last_error = None;
        let handle = self.query_input.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        cx.notify();
    }

    /// Immediately closes the Metadata Search palette without waiting for exit transition.
    pub fn close_modal(&mut self) {
        self.close_task = None;
        self.closing = false;
        self.open = false;
    }

    /// Closes the Metadata Search palette with a 140ms exit transition matching `Cmd+K`.
    pub fn close_modal_animated(&mut self, cx: &mut Context<TableMinusWorkspace>) {
        if !self.open || self.closing {
            return;
        }
        let exit_ms = effective_duration_ms(140, cx);
        if exit_ms == 0 {
            self.close_modal();
            cx.notify();
            return;
        }
        self.open = false;
        self.closing = true;
        self.anim_gen = self.anim_gen.wrapping_add(1);
        let expected_gen = self.anim_gen;
        self.close_task = Some(cx.spawn(
            async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
                cx.background_executor()
                    .timer(Duration::from_millis(exit_ms + 32))
                    .await;
                let _ = this.update(cx, |ws, cx| {
                    if ws.metadata_search_ui.closing
                        && ws.metadata_search_ui.anim_gen == expected_gen
                    {
                        ws.metadata_search_ui.closing = false;
                        ws.metadata_search_ui.close_task = None;
                        cx.notify();
                    }
                });
            },
        ));
        cx.notify();
    }
}

/// Renders the Cmd+K style Metadata Search spotlight overlay above the workspace.
#[must_use]
pub fn render_metadata_search_modal(
    ws: &mut TableMinusWorkspace,
    _window: &mut Window,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let is_open = ws.metadata_search_ui.open;
    let is_closing = ws.metadata_search_ui.closing;
    if !is_open && !is_closing {
        return div().into_any_element();
    }
    let anim_gen = ws.metadata_search_ui.anim_gen;

    let is_connected = matches!(
        ws.controller
            .connections
            .phase(ws.controller.active_workspace),
        Some(ConnectionWindowPhase::Connected)
    );
    ensure_columns_cached_once(ws, is_connected);

    let colors = cx.colors().clone();
    let opaque_surface = with_alpha(colors.overlay.background, 1.0);
    let border = hairline_border_color(cx);

    let matches = collect_search_results(
        &ws.metadata_search_ui.query,
        ws.metadata_search_ui.filter,
        &ws.controller.cached_objects,
        &ws.metadata_search_ui.cached_defs,
    );
    let visible_count = matches.len().min(MAX_DISPLAYED_RESULTS);
    if visible_count == 0 {
        ws.metadata_search_ui.selected_index = 0;
    } else if ws.metadata_search_ui.selected_index >= visible_count {
        ws.metadata_search_ui.selected_index = visible_count - 1;
    }

    let enter_targets: Vec<(Option<TableRef>, String)> = matches
        .iter()
        .take(MAX_DISPLAYED_RESULTS)
        .map(|m| (m.target_table.clone(), m.qualified_name.clone()))
        .collect();

    let palette = div()
        .id("metadata-search-palette")
        .w_full()
        .max_w(px(620.0))
        .rounded(px(12.0))
        .border_1()
        .border_color(border)
        .bg(opaque_surface)
        .shadow_lg()
        .overflow_hidden()
        .flex()
        .flex_col()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .on_key_down(cx.listener(move |ws, ev: &KeyDownEvent, _w, cx| {
            match ev.keystroke.key.as_str() {
                "escape" => {
                    ws.metadata_search_ui.close_modal_animated(cx);
                }
                "up" => {
                    ws.metadata_search_ui.selected_index =
                        ws.metadata_search_ui.selected_index.saturating_sub(1);
                    cx.notify();
                }
                "down" if visible_count > 0 => {
                    ws.metadata_search_ui.selected_index =
                        (ws.metadata_search_ui.selected_index + 1).min(visible_count - 1);
                    cx.notify();
                }
                "enter" => {
                    if let Some((tbl, qual)) =
                        enter_targets.get(ws.metadata_search_ui.selected_index)
                    {
                        activate_search_result(ws, tbl.as_ref(), qual, cx);
                    }
                }
                _ => {}
            }
        }))
        .child(render_search_controls(ws, is_connected, cx))
        .child(div().h(px(1.0)).w_full().bg(border))
        .child(render_results_container(ws, is_connected, &matches, cx))
        .child(div().h(px(1.0)).w_full().bg(border))
        .child(render_palette_footer(ws, matches.len(), cx));

    let enter_ms = effective_duration_ms(160, cx);
    let exit_ms = effective_duration_ms(140, cx);
    let anim_ms = if is_closing { exit_ms } else { enter_ms };
    let animated_palette = wrap_metadata_palette_anim(palette, anim_gen, anim_ms, is_closing);

    let backdrop = div()
        .id("metadata-search-backdrop")
        .absolute()
        .inset_0()
        .bg(colors.backdrop)
        .flex()
        .justify_center()
        .items_start()
        .pt(px(88.0))
        .px(px(20.0))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|ws, _, _w, cx| {
                ws.metadata_search_ui.close_modal_animated(cx);
            }),
        )
        .child(animated_palette);

    wrap_metadata_backdrop_anim(backdrop, anim_gen, anim_ms, is_closing)
}

fn wrap_metadata_palette_anim(
    palette: herogpui::gpui::Stateful<herogpui::gpui::Div>,
    anim_gen: u64,
    anim_ms: u64,
    is_closing: bool,
) -> AnyElement {
    if anim_ms == 0 {
        return palette.into_any_element();
    }
    let easing = if is_closing {
        ease_out_cubic
    } else {
        ease_out_quint
    };
    palette
        .with_animation(
            ("metadata-search-palette-anim", anim_gen),
            Animation::new(Duration::from_millis(anim_ms)).with_easing(easing),
            move |el, delta| {
                if is_closing {
                    el.opacity((1.0 - delta).max(0.0)).mt(px(-6.0 * delta))
                } else if delta >= 1.0 {
                    el
                } else {
                    el.opacity(delta).mt(px(-10.0 * (1.0 - delta)))
                }
            },
        )
        .into_any_element()
}

fn wrap_metadata_backdrop_anim(
    backdrop: herogpui::gpui::Stateful<herogpui::gpui::Div>,
    anim_gen: u64,
    anim_ms: u64,
    is_closing: bool,
) -> AnyElement {
    if anim_ms == 0 {
        return backdrop.into_any_element();
    }
    backdrop
        .with_animation(
            ("metadata-search-backdrop-anim", anim_gen),
            Animation::new(Duration::from_millis(anim_ms)).with_easing(ease_out_cubic),
            move |el, delta| {
                if is_closing {
                    el.opacity((1.0 - delta).max(0.0))
                } else if delta >= 1.0 {
                    el
                } else {
                    el.opacity(delta)
                }
            },
        )
        .into_any_element()
}

fn ensure_columns_cached_once(ws: &mut TableMinusWorkspace, is_connected: bool) {
    if !ws.metadata_search_ui.open || !is_connected {
        return;
    }
    let key = (
        ws.controller.active_workspace,
        ws.controller.cached_objects.len(),
    );
    if ws.metadata_search_ui.loaded_for == Some(key) {
        return;
    }
    ws.metadata_search_ui.loaded_for = Some(key);
    refresh_cached_columns(ws);
}

fn refresh_cached_columns(ws: &mut TableMinusWorkspace) {
    match ws.controller.fetch_all_table_definitions_blocking() {
        Ok(defs) => {
            ws.metadata_search_ui.cached_defs = defs;
            ws.metadata_search_ui.last_error = None;
        }
        Err(err) => {
            ws.metadata_search_ui.last_error = Some(err.to_string());
        }
    }
}

fn render_search_controls(
    ws: &TableMinusWorkspace,
    is_connected: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let ws_clear = cx.entity().downgrade();
    let search_input = SearchField::new(ws.metadata_search_ui.query_input.clone())
        .placeholder(s.metadata_search_placeholder)
        .full_width()
        .height(px(36.0))
        .text_size(px(13.5))
        .on_change(cx.listener(|ws, val: &str, _w, cx| {
            ws.metadata_search_ui.query = val.to_owned();
            ws.metadata_search_ui.selected_index = 0;
            cx.notify();
        }))
        .on_clear(move |_w, cx| {
            let _ = ws_clear.update(cx, |ws, cx| {
                ws.metadata_search_ui.query.clear();
                ws.metadata_search_ui.selected_index = 0;
                cx.notify();
            });
        });

    let refresh_btn = Button::new("metadata-search-refresh")
        .variant(Variant::Ghost)
        .size(HeroSize::Sm)
        .is_disabled(!is_connected)
        .on_press(cx.listener(|ws, _, _w, cx| {
            refresh_cached_columns(ws);
            cx.notify();
        }))
        .child(Icon::new(IconName::RefreshCw).size(IconSize::Xs));

    let filter = ws.metadata_search_ui.filter;
    let pills = div()
        .px(px(12.0))
        .pb(px(8.0))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .items_center()
                .flex_wrap()
                .gap(px(4.0))
                .child(filter_pill(
                    "meta-filter-all",
                    s.metadata_type_all,
                    MetadataFilterKind::All,
                    filter == MetadataFilterKind::All,
                    cx,
                ))
                .child(filter_pill(
                    "meta-filter-tables",
                    s.tables,
                    MetadataFilterKind::Tables,
                    filter == MetadataFilterKind::Tables,
                    cx,
                ))
                .child(filter_pill(
                    "meta-filter-views",
                    s.views,
                    MetadataFilterKind::Views,
                    filter == MetadataFilterKind::Views,
                    cx,
                ))
                .child(filter_pill(
                    "meta-filter-columns",
                    s.columns_button,
                    MetadataFilterKind::Columns,
                    filter == MetadataFilterKind::Columns,
                    cx,
                )),
        )
        .child(refresh_btn);

    div()
        .flex()
        .flex_col()
        .child(
            div()
                .px(px(10.0))
                .pt(px(8.0))
                .pb(px(4.0))
                .child(search_input),
        )
        .child(pills)
        .into_any_element()
}

fn filter_pill(
    id: &'static str,
    label: &'static str,
    kind: MetadataFilterKind,
    active: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let bg = if active {
        with_alpha(colors.accent.color, 0.16)
    } else {
        with_alpha(colors.foreground, 0.0)
    };
    let fg = if active {
        colors.accent.color
    } else {
        colors.muted
    };
    let hover_bg = colors.surface_secondary;
    div()
        .id(id)
        .px(px(8.0))
        .py(px(3.0))
        .rounded(px(6.0))
        .bg(bg)
        .hover(move |s| s.bg(hover_bg))
        .cursor_pointer()
        .text_size(px(11.5))
        .font_weight(if active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::MEDIUM
        })
        .text_color(fg)
        .on_click(cx.listener(move |ws, _, _w, cx| {
            ws.metadata_search_ui.filter = kind;
            ws.metadata_search_ui.selected_index = 0;
            cx.notify();
        }))
        .child(label)
        .into_any_element()
}

fn render_results_container(
    ws: &TableMinusWorkspace,
    is_connected: bool,
    matches: &[SearchResultEntry],
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();

    if !is_connected {
        return div()
            .h(px(220.0))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.foreground)
                    .child(s.no_active_connection),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(colors.muted)
                    .child(s.no_active_connection_desc),
            )
            .into_any_element();
    }

    let mut container = div().h(px(360.0)).flex().flex_col();
    if let Some(err) = ws.metadata_search_ui.last_error.as_ref() {
        container = container.child(
            div().px(px(10.0)).pt(px(8.0)).child(
                Alert::new(s.structure_metadata_incomplete)
                    .status(HeroColor::Warning)
                    .description(err.clone()),
            ),
        );
    }

    if matches.is_empty() {
        return container
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(4.0))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.foreground)
                            .child(s.no_matching_objects_title),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(colors.muted)
                            .child(s.no_matching_objects_desc),
                    ),
            )
            .into_any_element();
    }

    let selected_idx = ws.metadata_search_ui.selected_index;
    let mut list = div()
        .id("metadata-search-results")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .p(px(6.0))
        .flex()
        .flex_col()
        .gap(px(2.0));

    for (idx, entry) in matches.iter().take(MAX_DISPLAYED_RESULTS).enumerate() {
        list = list.child(render_result_row(idx, idx == selected_idx, entry, cx));
    }

    container.child(list).into_any_element()
}

fn render_result_row(
    idx: usize,
    is_selected: bool,
    entry: &SearchResultEntry,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let (icon, icon_color, tag_color) = match entry.kind {
        SearchResultKind::Table => (IconName::Table, colors.accent.color, colors.accent.color),
        SearchResultKind::View => (IconName::Layers, colors.accent.color, colors.muted),
        SearchResultKind::Column => (IconName::Columns2, colors.muted, colors.muted),
        SearchResultKind::Routine | SearchResultKind::Other => {
            (IconName::Code, colors.muted, colors.muted)
        }
    };

    let target_table = entry.target_table.clone();
    let qualified = entry.qualified_name.clone();
    let hover_bg = colors.surface_secondary;
    let row_bg = if is_selected {
        with_alpha(colors.accent.color, 0.14)
    } else {
        with_alpha(colors.overlay.background, 0.0)
    };

    div()
        .id(SharedString::from(format!("meta-res-{idx}")))
        .w_full()
        .h(px(34.0))
        .px(px(10.0))
        .rounded(px(6.0))
        .bg(row_bg)
        .hover(move |s| s.bg(hover_bg))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .on_click(cx.listener(move |ws, _, _w, cx| {
            activate_search_result(ws, target_table.as_ref(), &qualified, cx);
        }))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .items_center()
                .gap(px(9.0))
                .child(Icon::new(icon).size(IconSize::Sm).color(icon_color))
                .child(
                    div()
                        .truncate()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.foreground)
                        .child(entry.title.clone()),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(11.5))
                        .text_color(colors.muted)
                        .child(format!("· {}", entry.subtitle)),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(8.0))
                .when_some(entry.badge.clone(), |el, badge| {
                    el.child(
                        div()
                            .px(px(5.0))
                            .py(px(1.0))
                            .rounded(px(4.0))
                            .bg(with_alpha(colors.accent.color, 0.14))
                            .text_size(px(10.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(colors.accent.color)
                            .child(badge),
                    )
                })
                .child(
                    div()
                        .text_size(px(10.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(tag_color)
                        .child(entry.detail.clone()),
                )
                .when(is_selected, |el| {
                    el.child(
                        div()
                            .text_size(px(11.0))
                            .text_color(colors.muted)
                            .child("↵"),
                    )
                }),
        )
        .into_any_element()
}

fn render_palette_footer(ws: &TableMinusWorkspace, total: usize, cx: &App) -> AnyElement {
    let colors = cx.colors();
    let s = &ws.controller.strings;
    let shown = total.min(MAX_DISPLAYED_RESULTS);
    let count_text = if total > shown {
        format!("{shown} / {total} {}", s.object_plural)
    } else if total == 1 {
        format!("1 {}", s.object_singular)
    } else {
        format!("{total} {}", s.object_plural)
    };
    div()
        .px(px(12.0))
        .py(px(7.0))
        .flex()
        .items_center()
        .justify_between()
        .text_size(px(11.0))
        .text_color(colors.muted)
        .child("↑↓   ↵   Esc")
        .child(count_text)
        .into_any_element()
}

fn activate_search_result(
    ws: &mut TableMinusWorkspace,
    target_table: Option<&TableRef>,
    qualified_name: &str,
    cx: &mut Context<TableMinusWorkspace>,
) {
    ws.controller.screen_mode = ScreenMode::Workspace;
    ws.workspace_pane.close_tool();

    if let Some(table_ref) = target_table {
        ws.workspace_pane.sub_view = WorkspaceSubView::DataGrid;
        ws.controller.status_bar.view_mode = TableViewMode::Data;
        if let Err(err) = ws.controller.open_table_tab_blocking(table_ref.clone()) {
            ws.notify_toast(
                ToastSeverity::Error,
                ws.controller.strings.failed_to_open_table,
                Some(err.to_string().into()),
                cx,
            );
        }
    } else {
        let conn_id = TabConnectionId(format!("ws-{}", ws.controller.active_workspace.0));
        let title = format!("{} - {qualified_name}", ws.controller.strings.new_query_tab);
        ws.controller.tabs.open_query_tab(conn_id, Some(&title));
        ws.controller
            .set_editor_sql(&format!("-- {qualified_name}\n"));
        ws.workspace_pane.sub_view = WorkspaceSubView::SqlEditor;
    }

    ws.metadata_search_ui.close_modal_animated(cx);
}

fn collect_search_results(
    raw_query: &str,
    filter: MetadataFilterKind,
    objects: &[ObjectInfo],
    table_defs: &[TableDefinition],
) -> Vec<SearchResultEntry> {
    let needle = raw_query.trim().to_ascii_lowercase();
    let mut results = Vec::new();

    if !matches!(filter, MetadataFilterKind::Columns) {
        for obj in objects {
            let kind = match obj.kind {
                ObjectKind::Table | ObjectKind::PartitionedTable | ObjectKind::ForeignTable => {
                    SearchResultKind::Table
                }
                ObjectKind::View | ObjectKind::MaterializedView => SearchResultKind::View,
                ObjectKind::Function | ObjectKind::Procedure => SearchResultKind::Routine,
                _ => SearchResultKind::Other,
            };
            let include_by_filter = match filter {
                MetadataFilterKind::All => true,
                MetadataFilterKind::Tables => matches!(kind, SearchResultKind::Table),
                MetadataFilterKind::Views => matches!(kind, SearchResultKind::View),
                MetadataFilterKind::Columns => false,
            };
            if !include_by_filter {
                continue;
            }
            let qualified = qualify_name(obj.schema.as_deref(), &obj.name);
            if !matches_query(&needle, &[&obj.name, &qualified]) {
                continue;
            }
            let target_table = matches!(kind, SearchResultKind::Table | SearchResultKind::View)
                .then(|| TableRef::new(obj.schema.clone(), obj.name.clone()));
            let kind_tag = match kind {
                SearchResultKind::Table => "TABLE",
                SearchResultKind::View => "VIEW",
                SearchResultKind::Routine => "ROUTINE",
                SearchResultKind::Other => "OBJECT",
                SearchResultKind::Column => "COLUMN",
            };
            results.push(SearchResultEntry {
                kind,
                title: obj.name.clone(),
                subtitle: obj.schema.clone().unwrap_or_else(|| "default".to_owned()),
                detail: kind_tag.to_owned(),
                badge: None,
                target_table,
                qualified_name: qualified,
            });
        }
    }

    if matches!(
        filter,
        MetadataFilterKind::All | MetadataFilterKind::Columns
    ) {
        for def in table_defs {
            let table_qualified = qualify_name(def.table.schema.as_deref(), &def.table.name);
            for col in &def.columns {
                let col_name = col.name();
                let col_type = col.info.column_type.name.as_str();
                if !matches_query(&needle, &[col_name, &table_qualified, col_type]) {
                    continue;
                }
                results.push(SearchResultEntry {
                    kind: SearchResultKind::Column,
                    title: col_name.to_owned(),
                    subtitle: table_qualified.clone(),
                    detail: col_type.to_owned(),
                    badge: col.info.primary_key.then(|| "PK".to_owned()),
                    target_table: Some(def.table.clone()),
                    qualified_name: format!("{table_qualified}.{col_name}"),
                });
            }
        }
    }

    results
}

fn qualify_name(schema: Option<&str>, name: &str) -> String {
    match schema.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => format!("{s}.{name}"),
        None => name.to_owned(),
    }
}

fn matches_query(needle: &str, fields: &[&str]) -> bool {
    if needle.is_empty() {
        return true;
    }
    fields
        .iter()
        .any(|f| f.to_ascii_lowercase().contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tablepro_core::column::{ColumnDefault, ColumnInfo, ColumnKind, ColumnType, ReadForm};
    use tablepro_core::ddl::{ColumnAttributes, ColumnDefinition};

    fn sample_col(name: &str, ty: &str, pk: bool) -> ColumnDefinition {
        ColumnDefinition::new(
            ColumnInfo {
                name: name.to_owned(),
                column_type: ColumnType {
                    name: ty.to_owned(),
                    kind: ColumnKind::Other,
                    catalog: None,
                    dynamic_storage: false,
                    read_form: ReadForm::Native,
                },
                nullable: !pk,
                primary_key: pk,
                is_auto_increment: false,
                is_generated: false,
                partition_scoped: false,
                default: ColumnDefault::None,
                comment: None,
            },
            ColumnAttributes::default(),
        )
    }

    #[test]
    fn metadata_search_filters_tables_views_and_columns() {
        let objects = vec![
            ObjectInfo::new(Some("public".to_owned()), "users", ObjectKind::Table),
            ObjectInfo::new(Some("public".to_owned()), "active_users", ObjectKind::View),
        ];
        let defs = vec![TableDefinition {
            table: TableRef::new(Some("public".to_owned()), "users"),
            columns: vec![
                sample_col("id", "INTEGER", true),
                sample_col("email", "TEXT", false),
            ],
            primary_key: None,
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            inbound_foreign_keys: Vec::new(),
            source: None,
            options: None,
        }];

        let all = collect_search_results("user", MetadataFilterKind::All, &objects, &defs);
        assert_eq!(all.len(), 4);

        let only_views = collect_search_results("user", MetadataFilterKind::Views, &objects, &defs);
        assert_eq!(only_views.len(), 1);
        assert_eq!(only_views[0].title, "active_users");

        let only_cols =
            collect_search_results("email", MetadataFilterKind::Columns, &objects, &defs);
        assert_eq!(only_cols.len(), 1);
        assert_eq!(only_cols[0].title, "email");
        assert_eq!(
            only_cols[0].target_table,
            Some(TableRef::new(Some("public".to_owned()), "users"))
        );
    }
}
