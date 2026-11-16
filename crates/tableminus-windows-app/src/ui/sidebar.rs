//! Object explorer sidebar (schemas, tables, views, columns, indexes, functions, favorites, filter).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use herogpui::components::{
    Button, Card, CardContent, CardDescription, CardHeader, CardTitle, CardVariant, Chip,
    ChipLabel, ChipVariant, Color, ContextMenu, Dropdown, DropdownPlacement, Filter, Icon,
    IconName, IconSize, InputState, MenuItem, PickerItem, SearchField, Select, Sensitivity,
    Separator, Size, Skeleton, Spinner, SpinnerSize, TabItem, Tabs, TabsSize, TabsVariant, Tooltip,
    TreeItem, TreeView, Variant,
};
use herogpui::core::SelectionMode;
use herogpui::gpui::prelude::FluentBuilder as _;
use herogpui::gpui::{
    Animation, AnimationExt as _, AnyElement, App, AppContext as _, AsyncApp, ClipboardItem,
    Context, Entity, Focusable as _, FontFeatures, InteractiveElement, IntoElement, ParentElement,
    ScrollHandle, SharedString, StatefulInteractiveElement, Styled, WeakEntity, Window, div, px,
};
use herogpui::theme::{ActiveTheme, SkeletonAnimation};
use tableminus_state::connection_window::ConnectionWindowPhase;
use tableminus_state::tabs::TabConnectionId;
use tableminus_windows_platform::EnvironmentKind;
use tablepro_core::column::ColumnInfo;
use tablepro_core::meta::{IndexInfo, ObjectInfo, ObjectKind, ObjectScope, TableRef};
use tablepro_session::{OperationError, OperationKind};

use super::TableMinusWorkspace;
use super::shell_state::{ToastSeverity, WorkspaceSubView};
use super::theme::{
    COMPACT_GAP_PX, HAIRLINE_BORDER_PX, MICRO_GAP_PX, PANEL_RADIUS_PX, TEXT_BADGE_PX, TEXT_BODY_PX,
    TEXT_CONTROL_PX, ease_out_quad, hairline_border_color, is_reduced_motion, overlay_duration,
    panel_surface_bg,
};
use crate::app::ScreenMode;

fn tabular_font_features() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".to_owned(), 1)]))
}

const SIDEBAR_WIDTH_PX: f32 = 264.0;
const SECTION_TABLES: &str = "section:tables";
const SECTION_VIEWS: &str = "section:views";
const SECTION_ROUTINES: &str = "section:routines";
const SECTION_OTHER: &str = "section:other";

#[derive(Debug, Clone)]
enum TableDetailsState {
    Loading,
    Loaded {
        columns: Vec<ColumnInfo>,
        indexes: Vec<IndexInfo>,
    },
    Failed(String),
}

/// Persistent UI state for the object explorer sidebar.
#[derive(Debug)]
pub struct SidebarPanelState {
    pub filter_focused: bool,
    filter_input: Entity<InputState>,
    last_synced_filter: String,
    pub(crate) selected_schema: Option<String>,
    selected_node_key: Option<String>,
    expanded_keys: HashSet<SharedString>,
    table_details: HashMap<String, TableDetailsState>,
    schema_loading: bool,
    copied_object: Option<(String, Instant)>,
    tree_scroll_handle: ScrollHandle,
}

impl SidebarPanelState {
    /// Initializes sidebar state owned by [`TableMinusWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, cx: &mut Context<TableMinusWorkspace>) -> Self {
        let filter_input = cx.new(|cx| InputState::new(cx));
        let mut expanded_keys = HashSet::new();
        expanded_keys.insert(SharedString::from(SECTION_TABLES));
        expanded_keys.insert(SharedString::from(SECTION_VIEWS));

        Self {
            filter_focused: false,
            filter_input,
            last_synced_filter: String::new(),
            selected_schema: None,
            selected_node_key: None,
            expanded_keys,
            table_details: HashMap::new(),
            schema_loading: false,
            copied_object: None,
            tree_scroll_handle: ScrollHandle::new(),
        }
    }

    /// Requests focus on the sidebar filter input (`Ctrl+F`).
    pub fn focus_filter(&mut self, window: &mut Window, cx: &mut Context<TableMinusWorkspace>) {
        let handle = self.filter_input.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        self.filter_focused = true;
        cx.notify();
    }

    /// Clears cached table details and resets schema selection for a new connection session.
    pub fn reset_session_cache(&mut self) {
        self.table_details.clear();
        self.selected_schema = None;
        self.selected_node_key = None;
        self.schema_loading = false;
    }

    fn sync_filter_input(&mut self, controller_filter: &str, cx: &mut App) {
        if self.last_synced_filter != controller_filter {
            let next = controller_filter.to_owned();
            self.filter_input.update(cx, |input, cx| {
                input.set_value(next.clone());
                cx.notify();
            });
            self.last_synced_filter = next;
        }
    }
}

/// Renders the database object explorer sidebar.
#[must_use]
pub fn render_sidebar(
    ws: &mut TableMinusWorkspace,
    _window: &mut Window,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let controller_filter = ws.controller.sidebar_filter.clone();
    ws.sidebar_ui.sync_filter_input(&controller_filter, cx);

    let phase = ws
        .controller
        .connections
        .phase(ws.controller.active_workspace)
        .cloned()
        .unwrap_or(ConnectionWindowPhase::Disconnected);

    let body = match &phase {
        ConnectionWindowPhase::Connected => render_connected_body(ws, cx),
        ConnectionWindowPhase::Connecting { .. } | ConnectionWindowPhase::Reconnecting { .. } => {
            render_schema_skeletons(cx)
        }
        ConnectionWindowPhase::Disconnected => render_disconnected_card(&ws.controller.strings, cx),
        ConnectionWindowPhase::Unavailable(view) => {
            render_unavailable_sidebar_card(&ws.controller.strings, &view.summary, cx)
        }
    };

    let env_accent = matches!(phase, ConnectionWindowPhase::Connected)
        .then(|| ws.controller.active_connection_id())
        .flatten()
        .map(|id| ws.controller.connection_environment(id))
        .filter(|env| matches!(env, EnvironmentKind::Production | EnvironmentKind::Staging));

    div()
        .id("workspace-sidebar")
        .w(px(SIDEBAR_WIDTH_PX))
        .h_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .bg(panel_surface_bg(cx))
        .border_r(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .overflow_hidden()
        .when_some(env_accent, |el, env| {
            el.child(
                div()
                    .w_full()
                    .h(px(2.0))
                    .flex_shrink_0()
                    .bg(super::library::env_color_hsla(env)),
            )
        })
        .child(render_sidebar_header(ws, &phase, cx))
        .child(Separator::new())
        .child(div().flex_1().min_h_0().flex().flex_col().child(body))
        .into_any_element()
}

fn render_sidebar_header(
    ws: &mut TableMinusWorkspace,
    phase: &ConnectionWindowPhase,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let is_connected = matches!(phase, ConnectionWindowPhase::Connected);
    let is_loading = ws.sidebar_ui.schema_loading
        || matches!(
            phase,
            ConnectionWindowPhase::Connecting { .. } | ConnectionWindowPhase::Reconnecting { .. }
        );

    let total_objects = ws.controller.cached_objects.len();
    let count_pill = div()
        .flex_shrink_0()
        .font_features(tabular_font_features())
        .child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .color(if is_connected {
                    Color::Accent
                } else {
                    Color::Default
                })
                .size(Size::Sm)
                .child(ChipLabel::new().child(format!("{total_objects}"))),
        );

    let refresh_btn = render_refresh_button(
        ws.controller.strings.refresh_schema,
        is_connected,
        is_loading,
        cx,
    );

    let active_env = is_connected
        .then(|| ws.controller.active_connection_id())
        .flatten()
        .map(|id| ws.controller.connection_environment(id));
    let is_restoring = ws.controller.restoring_connection_id.is_some();

    let mut header = div()
        .flex()
        .flex_col()
        .gap(px(COMPACT_GAP_PX))
        .p(px(10.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(COMPACT_GAP_PX))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(COMPACT_GAP_PX))
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(TEXT_CONTROL_PX))
                                .font_weight(herogpui::gpui::FontWeight::SEMIBOLD)
                                .text_color(colors.foreground)
                                .child(ws.controller.strings.objects),
                        )
                        .child(count_pill),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(MICRO_GAP_PX))
                        .when_some(active_env, |el, env| {
                            el.child(super::library::render_env_badge(
                                env,
                                &ws.controller.strings,
                            ))
                        })
                        .child(refresh_btn),
                ),
        )
        .when(is_restoring, |el| {
            el.child(render_sidebar_restoring_banner(
                ws.controller.strings.session_restoring_connection,
                &colors,
            ))
        });

    if is_connected {
        header = header
            .child(render_database_and_schema_controls(ws, cx))
            .child(render_filter_controls(ws, cx));
    }

    header.into_any_element()
}

fn render_sidebar_restoring_banner(
    label: &'static str,
    colors: &herogpui::theme::ThemeColors,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(6.0))
        .py(px(4.0))
        .rounded(px(6.0))
        .bg(herogpui::core::with_alpha(colors.accent.color, 0.12))
        .child(
            Spinner::new("sidebar-restoring-spin")
                .size(SpinnerSize::Sm)
                .color(Color::Accent),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .text_xs()
                .text_color(colors.foreground)
                .child(label),
        )
        .into_any_element()
}

fn render_refresh_button(
    refresh_tooltip: &'static str,
    is_connected: bool,
    is_loading: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let reduced_motion = is_reduced_motion(cx);
    let btn = Button::new("sidebar-refresh-schemas")
        .variant(Variant::Tertiary)
        .size(Size::Sm)
        .is_icon_only(true)
        .aria_label(refresh_tooltip)
        .is_disabled(!is_connected || is_loading)
        .content(move |_| {
            if is_loading && !reduced_motion {
                Spinner::new("sidebar-refresh-spinner")
                    .size(SpinnerSize::Sm)
                    .color(Color::Accent)
                    .into_any_element()
            } else {
                Icon::new(IconName::RefreshCw)
                    .size(IconSize::Sm)
                    .color(colors.muted)
                    .into_any_element()
            }
        })
        .on_press(cx.listener(|ws, _, _, cx| {
            let schema = resolve_active_schema(ws);
            spawn_schema_refresh(ws, schema, cx);
        }));

    Tooltip::new(refresh_tooltip).child(btn).into_any_element()
}

fn render_database_and_schema_controls(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let active_schema = resolve_active_schema(ws);
    if active_schema.is_some() && ws.sidebar_ui.selected_schema.is_none() {
        ws.sidebar_ui.selected_schema = active_schema.clone();
    }

    let mut schema_items: Vec<PickerItem> = ws
        .controller
        .cached_schemas
        .iter()
        .map(|s| PickerItem::new(s.name.clone(), s.name.clone()))
        .collect();
    if schema_items.is_empty() {
        schema_items.push(PickerItem::new(
            "default",
            ws.controller.strings.default_schema,
        ));
    }

    let selected_key = active_schema.unwrap_or_else(|| "default".to_owned());
    let disabled_select = ws.controller.cached_schemas.is_empty() || ws.sidebar_ui.schema_loading;

    let schema_select = Select::new("sidebar-schema-select", schema_items)
        .aria_label(ws.controller.strings.active_schema_label)
        .placeholder(ws.controller.strings.select_schema)
        .value(Some(SharedString::from(selected_key)))
        .full_width(true)
        .height(px(28.0))
        .trigger_text_size(px(TEXT_CONTROL_PX))
        .row_text_size(px(TEXT_BODY_PX))
        .is_disabled(disabled_select)
        .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _, cx| {
            let Some(schema) = key.as_ref().map(|s| s.to_string()) else {
                return;
            };
            ws.sidebar_ui.selected_schema = Some(schema.clone());
            ws.sidebar_ui.selected_node_key = None;
            ws.sidebar_ui.table_details.clear();
            spawn_schema_refresh(ws, Some(schema), cx);
        }));

    div()
        .flex()
        .flex_col()
        .gap(px(MICRO_GAP_PX))
        .child(schema_select)
        .into_any_element()
}

fn render_filter_controls(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let fav_count = ws
        .controller
        .cached_objects
        .iter()
        .filter(|obj| ws.controller.favorite_objects.contains(&obj.name))
        .count();

    let s = &ws.controller.strings;
    let fav_label = if fav_count > 0 {
        format!("{} ({fav_count})", s.favorites)
    } else {
        s.favorites.to_owned()
    };

    let ws_for_clear = cx.entity().downgrade();
    let search = SearchField::new(ws.sidebar_ui.filter_input.clone())
        .placeholder(s.filter_shortcut_placeholder)
        .full_width()
        .height(px(30.0))
        .text_size(px(TEXT_CONTROL_PX))
        .on_change(cx.listener(|ws, value: &str, _, cx| {
            ws.sidebar_ui.last_synced_filter = value.to_owned();
            ws.controller.set_sidebar_filter(value);
            cx.notify();
        }))
        .on_clear(move |_window, cx| {
            let _ = ws_for_clear.update(cx, |ws, cx| {
                ws.sidebar_ui.last_synced_filter.clear();
                ws.controller.set_sidebar_filter("");
                cx.notify();
            });
        });

    let all_objs: Vec<&ObjectInfo> = ws.controller.cached_objects.iter().collect();
    let count_strip = render_count_badges_strip(
        &ws.controller.strings,
        SectionCounts::from_objects(&all_objs),
        cx,
    );
    let active_tab = if ws.controller.sidebar_favorites_only {
        "favorites"
    } else {
        "all"
    };

    let tabs = Tabs::new(
        "sidebar-scope-tabs",
        vec![
            TabItem::new("all", s.all),
            TabItem::new("favorites", fav_label),
        ],
        "all",
    )
    .selected_key(active_tab)
    .variant(TabsVariant::Secondary)
    .size(TabsSize::Sm)
    .on_selection_change(cx.listener(|ws, key: &SharedString, _, cx| {
        ws.controller.sidebar_favorites_only = key.as_ref() == "favorites";
        ws.controller.rebuild_sidebar_rows();
        cx.notify();
    }));

    div()
        .flex()
        .flex_col()
        .gap(px(COMPACT_GAP_PX))
        .child(search)
        .child(tabs)
        .child(count_strip)
        .into_any_element()
}

fn render_connected_body(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    if ws.sidebar_ui.schema_loading && ws.controller.cached_objects.is_empty() {
        return render_schema_skeletons(cx);
    }

    let filtered = filtered_objects(ws);
    if ws.controller.cached_objects.is_empty() {
        return render_empty_schema_card(&ws.controller.strings, cx);
    }
    if filtered.is_empty() {
        return render_empty_filtered_card(ws, cx);
    }

    let tree_items = build_tree_items(ws, &filtered);
    let selected_object = resolve_selected_object(ws).cloned();

    let mut selected_keys = HashSet::new();
    if let Some(ref key) = ws.sidebar_ui.selected_node_key {
        selected_keys.insert(SharedString::from(key.clone()));
    }
    let expanded_keys = ws.sidebar_ui.expanded_keys.clone();

    let tree = TreeView::new("sidebar-object-tree", tree_items)
        .selection_mode(SelectionMode::Single)
        .selected_keys(selected_keys)
        .expanded_keys(expanded_keys)
        .on_selection_change(cx.listener(|ws, keys: &HashSet<SharedString>, _, cx| {
            ws.sidebar_ui.selected_node_key = keys.iter().next().map(|k| k.to_string());
            if let Some(obj) = resolve_selected_object(ws).cloned()
                && is_tabular_object(obj.kind)
            {
                open_object_in_grid(ws, &obj, cx);
                return;
            }
            cx.notify();
        }))
        .on_expanded_change(cx.listener(|ws, keys: &HashSet<SharedString>, _, cx| {
            let newly_opened: Vec<String> = keys
                .iter()
                .filter(|k| !ws.sidebar_ui.expanded_keys.contains(*k))
                .map(|k| k.to_string())
                .collect();
            ws.sidebar_ui.expanded_keys = keys.clone();
            for key in newly_opened {
                maybe_load_expanded_table_details(ws, &key, cx);
            }
            cx.notify();
        }));

    let tree_section = render_tree_section(ws, tree, &selected_object, cx);

    let mut root = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(tree_section);

    if let Some(ref obj) = selected_object {
        root = root
            .child(Separator::new())
            .child(render_selected_node_bar(ws, obj, cx));
    }

    root.into_any_element()
}

fn render_tree_section(
    ws: &mut TableMinusWorkspace,
    tree: TreeView,
    selected_object: &Option<ObjectInfo>,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let ctx_items = build_object_menu_items(selected_object.as_ref(), ws);
    let tree_interactive = div()
        .w_full()
        .on_mouse_down(
            herogpui::gpui::MouseButton::Left,
            cx.listener(|ws, event: &herogpui::gpui::MouseDownEvent, _, cx| {
                if event.click_count >= 2
                    && let Some(obj) = resolve_selected_object(ws).cloned()
                    && is_tabular_object(obj.kind)
                {
                    open_object_in_new_grid_tab(ws, &obj, cx);
                }
            }),
        )
        .child(tree);
    let tree_with_ctx = ContextMenu::new(
        "sidebar-tree-context-menu",
        div().w_full().p(px(6.0)).child(tree_interactive),
        ctx_items,
    )
    .on_action(cx.listener(|ws, action_key: &SharedString, _, cx| {
        execute_node_action(ws, action_key.as_ref(), cx);
    }));

    div()
        .id("sidebar-object-tree-scroll")
        .track_scroll(&ws.sidebar_ui.tree_scroll_handle)
        .flex_1()
        .min_h_0()
        .w_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .child(tree_with_ctx)
        .into_any_element()
}

#[derive(Default, Clone, Copy)]
struct SectionCounts {
    tables: usize,
    views: usize,
    routines: usize,
}

impl SectionCounts {
    fn from_objects(objects: &[&ObjectInfo]) -> Self {
        let mut counts = Self::default();
        for obj in objects {
            match obj.kind {
                ObjectKind::Table | ObjectKind::PartitionedTable | ObjectKind::ForeignTable => {
                    counts.tables += 1;
                }
                ObjectKind::View | ObjectKind::MaterializedView => counts.views += 1,
                ObjectKind::Function | ObjectKind::Procedure | ObjectKind::Trigger => {
                    counts.routines += 1;
                }
                ObjectKind::Sequence | ObjectKind::Dictionary | ObjectKind::Key(_) | _ => {}
            }
        }
        counts
    }
}

fn render_count_badges_strip(
    s: &crate::i18n::Strings,
    counts: SectionCounts,
    cx: &Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .flex()
        .items_center()
        .gap(px(MICRO_GAP_PX))
        .flex_shrink_0()
        .child(summary_chip(s.tables, counts.tables, colors.muted))
        .child(summary_chip(s.views, counts.views, colors.muted))
        .child(summary_chip(s.routines, counts.routines, colors.muted))
        .into_any_element()
}

fn summary_chip(label: &str, count: usize, muted: herogpui::gpui::Hsla) -> AnyElement {
    Chip::new()
        .variant(ChipVariant::Secondary)
        .size(Size::Sm)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(5.0))
                .child(ChipLabel::new().child(label.to_owned()))
                .child(
                    div()
                        .flex_shrink_0()
                        .px(px(5.0))
                        .py(px(1.0))
                        .rounded(px(99.0))
                        .bg(muted.alpha(0.14))
                        .text_size(px(TEXT_BADGE_PX))
                        .font_features(tabular_font_features())
                        .child(format!("{count}")),
                ),
        )
        .into_any_element()
}

fn build_tree_items(ws: &TableMinusWorkspace, objects: &[&ObjectInfo]) -> Vec<TreeItem> {
    let mut tables = Vec::new();
    let mut views = Vec::new();
    let mut routines = Vec::new();
    let mut other = Vec::new();

    for obj in objects {
        let item = build_object_tree_item(ws, obj);
        match obj.kind {
            ObjectKind::Table | ObjectKind::PartitionedTable | ObjectKind::ForeignTable => {
                tables.push(item);
            }
            ObjectKind::View | ObjectKind::MaterializedView => views.push(item),
            ObjectKind::Function | ObjectKind::Procedure | ObjectKind::Trigger => {
                routines.push(item);
            }
            ObjectKind::Sequence | ObjectKind::Dictionary | ObjectKind::Key(_) | _ => {
                other.push(item);
            }
        }
    }

    let s = &ws.controller.strings;
    let mut roots = Vec::new();
    if !tables.is_empty() {
        roots.push(
            TreeItem::new(SECTION_TABLES, format!("{} ({})", s.tables, tables.len()))
                .icon(IconName::Folder.path())
                .children(tables),
        );
    }
    if !views.is_empty() {
        roots.push(
            TreeItem::new(SECTION_VIEWS, format!("{} ({})", s.views, views.len()))
                .icon(IconName::Folder.path())
                .children(views),
        );
    }
    if !routines.is_empty() {
        roots.push(
            TreeItem::new(
                SECTION_ROUTINES,
                format!("{} ({})", s.routines, routines.len()),
            )
            .icon(IconName::Folder.path())
            .children(routines),
        );
    }
    if !other.is_empty() {
        roots.push(
            TreeItem::new(
                SECTION_OTHER,
                format!("{} ({})", s.other_objects, other.len()),
            )
            .icon(IconName::Folder.path())
            .children(other),
        );
    }
    roots
}

fn build_object_tree_item(ws: &TableMinusWorkspace, obj: &ObjectInfo) -> TreeItem {
    let key = object_node_key(obj);
    let is_fav = ws.controller.favorite_objects.contains(&obj.name);
    let display = truncate_middle(&obj.name, 24);
    let label = if is_fav {
        format!("★ {display}")
    } else {
        display
    };

    let mut item = TreeItem::new(key.clone(), label).icon(object_icon(obj.kind).path());

    if is_tabular_object(obj.kind) {
        let children = match ws.sidebar_ui.table_details.get(&key) {
            Some(TableDetailsState::Loaded { columns, indexes }) => {
                let mut groups = Vec::new();
                let col_items: Vec<TreeItem> = columns
                    .iter()
                    .map(|col| {
                        let pk = if col.primary_key { " PK" } else { "" };
                        let col_label = format!(
                            "{} : {}{}",
                            truncate_middle(&col.name, 16),
                            col.column_type.name,
                            pk
                        );
                        let icon = if col.primary_key {
                            IconName::Key.path()
                        } else {
                            IconName::Columns2.path()
                        };
                        TreeItem::new(format!("col:{key}:{}", col.name), col_label).icon(icon)
                    })
                    .collect();
                groups.push(
                    TreeItem::new(
                        format!("grp:cols:{key}"),
                        format!(
                            "{} ({})",
                            ws.controller.strings.columns_button,
                            col_items.len()
                        ),
                    )
                    .icon(IconName::Columns2.path())
                    .children(col_items),
                );
                if !indexes.is_empty() {
                    let idx_items: Vec<TreeItem> = indexes
                        .iter()
                        .map(|idx| {
                            let tag = if idx.primary {
                                " [PK]"
                            } else if idx.unique {
                                " [UNIQUE]"
                            } else {
                                ""
                            };
                            TreeItem::new(
                                format!("idx:{key}:{}", idx.name),
                                format!("{}{}", truncate_middle(&idx.name, 18), tag),
                            )
                            .icon(IconName::ListOrdered.path())
                        })
                        .collect();
                    groups.push(
                        TreeItem::new(
                            format!("grp:idxs:{key}"),
                            format!(
                                "{} ({})",
                                ws.controller.strings.indexes_label,
                                idx_items.len()
                            ),
                        )
                        .icon(IconName::ListOrdered.path())
                        .children(idx_items),
                    );
                }
                groups
            }
            Some(TableDetailsState::Failed(err)) => vec![
                TreeItem::new(format!("err:{key}"), truncate_middle(err, 26))
                    .icon(IconName::TriangleAlert.path())
                    .is_disabled(true),
            ],
            Some(TableDetailsState::Loading) | None => vec![
                TreeItem::new(
                    format!("loading:{key}"),
                    ws.controller.strings.expand_to_load_columns,
                )
                .icon(IconName::LoaderCircle.path())
                .is_disabled(true),
            ],
        };
        item = item.children(children);
    }

    item
}

fn render_selected_node_dropdown(
    ws: &TableMinusWorkspace,
    obj: &ObjectInfo,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let muted = cx.colors().muted;
    Dropdown::uncontrolled(
        "sidebar-selected-dropdown",
        Button::new("sidebar-selected-more")
            .variant(Variant::Tertiary)
            .size(Size::Sm)
            .is_icon_only(true)
            .aria_label(ws.controller.strings.object_actions_aria)
            .content(move |_| {
                Icon::new(IconName::EllipsisVertical)
                    .size(IconSize::Sm)
                    .color(muted)
                    .into_any_element()
            }),
        build_object_menu_items(Some(obj), ws),
    )
    .placement(DropdownPlacement::TopEnd)
    .on_action(cx.listener(|ws, action_key: &SharedString, _, cx| {
        execute_node_action(ws, action_key.as_ref(), cx);
    }))
    .into_any_element()
}

fn render_selected_node_actions(
    ws: &mut TableMinusWorkspace,
    obj: &ObjectInfo,
    qualified: &str,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let is_fav = ws.controller.favorite_objects.contains(&obj.name);
    let recently_copied = ws
        .sidebar_ui
        .copied_object
        .as_ref()
        .is_some_and(|(name, at)| name == qualified && at.elapsed() < Duration::from_secs(2));

    let fav_btn = Tooltip::new(if is_fav {
        ws.controller.strings.remove_favorite
    } else {
        ws.controller.strings.add_favorite
    })
    .child(
        Button::new("sidebar-selected-fav")
            .variant(Variant::Tertiary)
            .size(Size::Sm)
            .is_icon_only(true)
            .aria_label(ws.controller.strings.toggle_favorite_aria)
            .content(move |_| {
                Icon::new(IconName::Star)
                    .size(IconSize::Sm)
                    .color(if is_fav {
                        colors.warning.color
                    } else {
                        colors.muted
                    })
                    .into_any_element()
            })
            .on_press(cx.listener(|ws, _, _, cx| {
                execute_node_action(ws, "toggle_favorite", cx);
            })),
    );

    let copy_btn = Tooltip::new(ws.controller.strings.copy_qualified_name).child(
        Button::new("sidebar-selected-copy")
            .variant(Variant::Tertiary)
            .size(Size::Sm)
            .is_icon_only(true)
            .aria_label(ws.controller.strings.copy_qualified_name)
            .content(move |_| {
                Icon::new(if recently_copied {
                    IconName::Check
                } else {
                    IconName::Copy
                })
                .size(IconSize::Sm)
                .color(if recently_copied {
                    colors.success.color
                } else {
                    colors.muted
                })
                .into_any_element()
            })
            .on_press(cx.listener(|ws, _, _, cx| {
                execute_node_action(ws, "copy_qualified", cx);
            })),
    );

    let actions_dropdown = render_selected_node_dropdown(ws, obj, cx);

    div()
        .flex()
        .items_center()
        .gap(px(2.0))
        .child(fav_btn)
        .child(copy_btn)
        .child(actions_dropdown)
        .into_any_element()
}

fn render_selected_node_bar(
    ws: &mut TableMinusWorkspace,
    obj: &ObjectInfo,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let qualified = qualified_name(obj);
    let actions = render_selected_node_actions(ws, obj, &qualified, cx);

    let mut bar = div()
        .flex()
        .flex_col()
        .gap(px(MICRO_GAP_PX))
        .px(px(10.0))
        .py(px(8.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(MICRO_GAP_PX))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .font_weight(herogpui::gpui::FontWeight::MEDIUM)
                        .text_color(colors.foreground)
                        .child(truncate_middle(&qualified, 22)),
                )
                .child(actions),
        );

    if is_tabular_object(obj.kind) {
        bar = bar.child(
            div()
                .flex()
                .items_center()
                .gap(px(MICRO_GAP_PX))
                .child(
                    Button::new("sidebar-open-data")
                        .label(ws.controller.strings.data_tab)
                        .variant(Variant::Secondary)
                        .size(Size::Sm)
                        .grow(true)
                        .on_press(cx.listener(|ws, _, _, cx| {
                            execute_node_action(ws, "open_data", cx);
                        })),
                )
                .child(
                    Button::new("sidebar-open-structure")
                        .label(ws.controller.strings.structure_tab)
                        .variant(Variant::Secondary)
                        .size(Size::Sm)
                        .grow(true)
                        .on_press(cx.listener(|ws, _, _, cx| {
                            execute_node_action(ws, "view_structure", cx);
                        })),
                ),
        );
    }

    if is_reduced_motion(cx) {
        return bar.into_any_element();
    }

    let anim_key = ws
        .sidebar_ui
        .selected_node_key
        .clone()
        .unwrap_or_else(|| qualified.clone());
    bar.with_animation(
        SharedString::from(format!("sidebar-selected-bar:{anim_key}")),
        Animation::new(overlay_duration(cx)).with_easing(ease_out_quad),
        |el, delta| el.opacity(delta).pt(px(8.0 + 4.0 * (1.0 - delta))),
    )
    .into_any_element()
}

fn build_object_menu_items(
    selected: Option<&ObjectInfo>,
    ws: &TableMinusWorkspace,
) -> Vec<MenuItem> {
    let has_table = selected.is_some_and(|obj| is_tabular_object(obj.kind));
    let has_any = selected.is_some();
    let is_fav = selected.is_some_and(|obj| ws.controller.favorite_objects.contains(&obj.name));

    let s = &ws.controller.strings;
    let mut items = Vec::new();
    if has_table {
        items.push(MenuItem::new("open_data", s.ctx_open_table_data).icon(IconName::Table.path()));
        items.push(
            MenuItem::new("view_structure", s.ctx_view_structure).icon(IconName::Columns2.path()),
        );
        items.push(
            MenuItem::new("new_select", s.ctx_new_select_query).icon(IconName::FileCode.path()),
        );
        items.push(MenuItem::Separator);
    }
    if has_any {
        items.push(
            MenuItem::new("copy_qualified", s.copy_qualified_name).icon(IconName::Copy.path()),
        );
        items.push(
            MenuItem::new(
                "toggle_favorite",
                if is_fav {
                    s.remove_from_favorites
                } else {
                    s.add_to_favorites
                },
            )
            .icon(IconName::Star.path()),
        );
        items.push(MenuItem::Separator);
    }
    items.push(
        MenuItem::new("refresh_schema", s.refresh_schema_short).icon(IconName::RefreshCw.path()),
    );
    items
}

fn active_engine_kind(ws: &TableMinusWorkspace) -> tableminus_gfx::glyphs::EngineKind {
    use tableminus_gfx::glyphs::EngineKind;
    if let Some(conn_id) = ws.controller.active_connection_id()
        && let Some(conn) = ws
            .controller
            .library
            .storage()
            .connections
            .values()
            .find(|c| c.id.0 == conn_id)
    {
        return crate::views::library::engine_from_driver_id(&conn.driver_id);
    }
    let version = ws.controller.toolbar_status.engine_version.as_str();
    EngineKind::ALL
        .iter()
        .copied()
        .find(|kind| version.starts_with(kind.display_name()))
        .or_else(|| {
            ws.controller
                .rail_items
                .iter()
                .find(|item| item.connected)
                .map(|item| item.engine)
        })
        .unwrap_or(EngineKind::Sqlite)
}

fn quote_ident(engine: tableminus_gfx::glyphs::EngineKind, ident: &str) -> String {
    use tableminus_gfx::glyphs::EngineKind;
    match engine {
        EngineKind::Mysql | EngineKind::Mariadb | EngineKind::Clickhouse => {
            format!("`{}`", ident.replace('`', "``"))
        }
        EngineKind::Mssql => format!("[{}]", ident.replace(']', "]]")),
        _ => format!("\"{}\"", ident.replace('"', "\"\"")),
    }
}

fn quote_table_ref_parts(
    engine: tableminus_gfx::glyphs::EngineKind,
    schema: Option<&str>,
    table_name: &str,
) -> String {
    let table = quote_ident(engine, table_name);
    match schema.map(str::trim) {
        Some(s) if !s.is_empty() => format!("{}.{table}", quote_ident(engine, s)),
        _ => table,
    }
}

fn format_order_by(
    engine: tableminus_gfx::glyphs::EngineKind,
    sort_cols: &[(&str, bool)],
) -> String {
    let parts: Vec<String> = sort_cols
        .iter()
        .map(|(col, asc)| {
            let dir = if *asc { "ASC" } else { "DESC" };
            format!("{} {dir}", quote_ident(engine, col))
        })
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!(" ORDER BY {}", parts.join(", "))
    }
}

fn build_table_open_queries(
    engine: tableminus_gfx::glyphs::EngineKind,
    schema: Option<&str>,
    table_name: &str,
    sort_cols: &[(&str, bool)],
    limit: u64,
    offset: u64,
) -> Vec<(super::console_log::ConsoleLogKind, String)> {
    use super::console_log::ConsoleLogKind;
    use tableminus_gfx::glyphs::EngineKind;

    let limit = limit.max(1);
    match engine {
        EngineKind::Mongodb => {
            let escaped = table_name.replace('"', "\\\"");
            return vec![
                (
                    ConsoleLogKind::Query,
                    format!(
                        "db.getCollection(\"{escaped}\").find({{}}).skip({offset}).limit({limit});"
                    ),
                ),
                (
                    ConsoleLogKind::Count,
                    format!("db.getCollection(\"{escaped}\").countDocuments({{}});"),
                ),
                (
                    ConsoleLogKind::Schema,
                    format!("db.getCollection(\"{escaped}\").findOne();"),
                ),
            ];
        }
        EngineKind::Redis => {
            return vec![
                (
                    ConsoleLogKind::Query,
                    format!("SCAN 0 MATCH {table_name}* COUNT {limit}"),
                ),
                (ConsoleLogKind::Count, "DBSIZE".to_owned()),
            ];
        }
        _ => {}
    }

    let qualified = quote_table_ref_parts(engine, schema, table_name);
    let order_by = format_order_by(engine, sort_cols);
    let select_sql = match engine {
        EngineKind::Mssql if order_by.is_empty() => {
            format!("SELECT TOP {limit} * FROM {qualified};")
        }
        EngineKind::Mssql => {
            format!(
                "SELECT * FROM {qualified}{order_by} OFFSET {offset} ROWS FETCH NEXT {limit} ROWS ONLY;"
            )
        }
        _ => format!("SELECT * FROM {qualified}{order_by} LIMIT {limit} OFFSET {offset};"),
    };
    let count_sql = format!("SELECT COUNT(*) FROM {qualified};");
    let escaped_name = table_name.replace('\'', "''");
    let schema_sql = match engine {
        EngineKind::Sqlite => {
            format!("PRAGMA table_info({});", quote_ident(engine, table_name))
        }
        EngineKind::Duckdb => format!("DESCRIBE {qualified};"),
        EngineKind::Clickhouse | EngineKind::Cassandra | EngineKind::Scylladb => {
            format!("DESCRIBE TABLE {qualified};")
        }
        EngineKind::Mysql | EngineKind::Mariadb => format!("SHOW FULL COLUMNS FROM {qualified};"),
        EngineKind::Postgresql => {
            let schema_name = schema
                .filter(|s| !s.is_empty())
                .unwrap_or("public")
                .replace('\'', "''");
            format!(
                "SELECT column_name, data_type, is_nullable FROM information_schema.columns WHERE table_schema = '{schema_name}' AND table_name = '{escaped_name}' ORDER BY ordinal_position;"
            )
        }
        EngineKind::Mssql => {
            let schema_name = schema
                .filter(|s| !s.is_empty())
                .unwrap_or("dbo")
                .replace('\'', "''");
            format!(
                "SELECT COLUMN_NAME, DATA_TYPE, IS_NULLABLE FROM INFORMATION_SCHEMA.COLUMNS WHERE TABLE_SCHEMA = '{schema_name}' AND TABLE_NAME = '{escaped_name}' ORDER BY ORDINAL_POSITION;"
            )
        }
        EngineKind::Mongodb | EngineKind::Redis => unreachable!(),
    };

    vec![
        (ConsoleLogKind::Query, select_sql),
        (ConsoleLogKind::Count, count_sql),
        (ConsoleLogKind::Schema, schema_sql),
    ]
}

fn log_table_open_queries(ws: &mut TableMinusWorkspace, obj: &ObjectInfo) {
    let engine = active_engine_kind(ws);
    let limit = ws.controller.grid_model.page.limit.max(1);
    let offset = ws.controller.grid_model.page.offset;
    let cols = ws.controller.grid_model.columns.columns();
    let sort_cols: Vec<(&str, bool)> = ws
        .controller
        .grid_model
        .sort
        .keys
        .iter()
        .filter_map(|sk| {
            let col = cols.iter().find(|c| c.id == sk.column_id)?;
            let asc = matches!(
                sk.direction,
                tableminus_state::data_grid::SortDirection::Ascending
            );
            Some((col.name.as_str(), asc))
        })
        .collect();

    let entries = build_table_open_queries(
        engine,
        obj.schema.as_deref(),
        &obj.name,
        &sort_cols,
        limit,
        offset,
    );
    for (kind, sql) in entries {
        ws.console_log.push(sql, kind);
    }
}

fn open_object_in_grid(
    ws: &mut TableMinusWorkspace,
    obj: &ObjectInfo,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let table_ref = TableRef::new(obj.schema.clone(), obj.name.clone());
    match ws.controller.open_table_tab_blocking(table_ref) {
        Ok(()) => {
            log_table_open_queries(ws, obj);
            ws.welcome_ui.close();
            ws.controller.screen_mode = ScreenMode::Workspace;
            ws.sync_active_tab_sub_view();
            ws.workspace_pane.sub_view = WorkspaceSubView::DataGrid;
            cx.notify();
        }
        Err(err) => {
            ws.console_log.push(
                format!("-- Failed to open {}: {err}", obj.name),
                super::console_log::ConsoleLogKind::Error,
            );
            let msg = ws.controller.strings.failed_to_open_table;
            ws.notify_toast(ToastSeverity::Error, msg, Some(err.to_string().into()), cx);
        }
    }
}

fn open_object_in_new_grid_tab(
    ws: &mut TableMinusWorkspace,
    obj: &ObjectInfo,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let conn_id = TabConnectionId(format!("ws-{}", ws.controller.active_workspace.0));
    let new_tab = tableminus_state::tabs::TabItem {
        id: tableminus_state::tabs::TabId(0),
        connection_id: conn_id.clone(),
        title: obj.name.clone(),
        kind: tableminus_state::tabs::TabKind::TableData {
            table_name: obj.name.clone(),
        },
        pinned: false,
        has_unsaved_edits: false,
        has_active_filters: false,
        has_active_sort: false,
    };
    ws.controller.tabs.attach_tab(new_tab, conn_id);

    let table_ref = TableRef::new(obj.schema.clone(), obj.name.clone());
    ws.controller.active_table = Some(table_ref.clone());
    ws.controller.screen_mode = ScreenMode::Workspace;
    ws.controller.sync_layout_counts();

    match ws
        .controller
        .query_active_table_page_blocking(&tablepro_core::meta::PagePosition::First)
    {
        Ok(()) => {
            log_table_open_queries(ws, obj);
            let _ = ws.controller.refresh_table_row_count_blocking();
            let _ = ws.controller.load_table_structure_blocking(&table_ref);
            ws.welcome_ui.close();
            ws.sync_active_tab_sub_view();
            ws.workspace_pane.sub_view = WorkspaceSubView::DataGrid;
            cx.notify();
        }
        Err(err) => {
            ws.console_log.push(
                format!("-- Failed to open {}: {err}", obj.name),
                super::console_log::ConsoleLogKind::Error,
            );
            let msg = ws.controller.strings.failed_to_open_table;
            ws.notify_toast(ToastSeverity::Error, msg, Some(err.to_string().into()), cx);
        }
    }
}

fn execute_node_action(
    ws: &mut TableMinusWorkspace,
    action: &str,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let selected = resolve_selected_object(ws).cloned();
    match action {
        "open_data" => {
            let Some(obj) = selected else { return };
            open_object_in_grid(ws, &obj, cx);
        }
        "view_structure" => {
            let Some(obj) = selected else { return };
            let table_ref = TableRef::new(obj.schema.clone(), obj.name.clone());
            let _ = ws.controller.open_table_tab_blocking(table_ref.clone());
            match ws.controller.load_table_structure_blocking(&table_ref) {
                Ok(()) => {
                    ws.workspace_pane.sub_view = WorkspaceSubView::Structure;
                    cx.notify();
                }
                Err(err) => {
                    let msg = ws.controller.strings.load_structure_failed_toast;
                    ws.notify_toast(ToastSeverity::Error, msg, Some(err.to_string().into()), cx);
                }
            }
        }
        "new_select" => {
            let Some(obj) = selected else { return };
            let qualified = qualified_name(&obj);
            let sql = format!("SELECT *\nFROM {qualified}\nLIMIT 100;\n");
            let conn_id = TabConnectionId(format!("ws-{}", ws.controller.active_workspace.0));
            let tab_title = format!("{} - {}", ws.controller.strings.new_query_tab, obj.name);
            ws.controller.tabs.open_query_tab(conn_id, Some(&tab_title));
            ws.controller.set_editor_sql(&sql);
            ws.workspace_pane.sub_view = WorkspaceSubView::SqlEditor;
            let ready_msg = ws.controller.strings.query_ready_toast;
            ws.notify_toast(ToastSeverity::Info, ready_msg, Some(qualified.into()), cx);
            cx.notify();
        }
        "copy_qualified" => {
            let Some(obj) = selected else { return };
            let qualified = qualified_name(&obj);
            cx.write_to_clipboard(ClipboardItem::new_string(qualified.clone()));
            ws.sidebar_ui.copied_object = Some((qualified.clone(), Instant::now()));
            let copied_msg = ws.controller.strings.copied;
            ws.notify_toast(
                ToastSeverity::Success,
                copied_msg,
                Some(qualified.into()),
                cx,
            );
            cx.notify();
        }
        "toggle_favorite" => {
            let Some(obj) = selected else { return };
            ws.controller.toggle_object_favorite(&obj.name);
            let added = ws.controller.favorite_objects.contains(&obj.name);
            let title = if added {
                ws.controller.strings.added_to_favorites_toast
            } else {
                ws.controller.strings.removed_from_favorites_toast
            };
            ws.notify_toast(ToastSeverity::Info, title, Some(obj.name.into()), cx);
            cx.notify();
        }
        "refresh_schema" => {
            refresh_active_schema(ws, cx);
        }
        _ => {}
    }
}

/// Refreshes the active schema and object list asynchronously, then reloads details
/// for any tables/views that are currently expanded in the sidebar tree.
pub(crate) fn refresh_active_schema(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let schema = resolve_active_schema(ws);
    spawn_schema_refresh(ws, schema, cx);
}

fn spawn_schema_refresh(
    ws: &mut TableMinusWorkspace,
    schema_override: Option<String>,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let Some(services) = ws.controller.services.clone() else {
        let title = ws.controller.strings.unavailable;
        let desc = ws.controller.strings.service_runtime_not_init;
        ws.notify_toast(ToastSeverity::Warning, title, Some(desc.into()), cx);
        return;
    };
    let workspace_id = ws.controller.active_workspace;
    let no_session_err = ws.controller.strings.no_active_session_err;
    ws.sidebar_ui.schema_loading = true;
    cx.notify();

    cx.spawn(
        async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let svc = services.clone();
            drop(services.runtime.spawn(async move {
                let guard = svc.sessions.read().await;
                let Some(session) = guard.session(workspace_id) else {
                    let _ = tx.send(Err(no_session_err.to_owned()));
                    return;
                };
                let schemas = match session.refresh_schemas().await {
                    Ok(s) => s.to_vec(),
                    Err(err) => {
                        let _ = tx.send(Err(err.to_string()));
                        return;
                    }
                };
                let target_schema = schema_override.or_else(|| {
                    schemas
                        .iter()
                        .find(|s| s.is_default)
                        .or_else(|| schemas.first())
                        .map(|s| s.name.clone())
                });
                let scope = match target_schema.clone() {
                    Some(name) => ObjectScope::Schema(name),
                    None => ObjectScope::AllSchemas,
                };
                match session.refresh_objects(scope).await {
                    Ok(objs) => {
                        let _ = tx.send(Ok((schemas, objs.to_vec(), target_schema)));
                    }
                    Err(err) => {
                        let _ = tx.send(Err(err.to_string()));
                    }
                }
            }));

            if let Ok(result) = rx.await {
                let _ = this.update(cx, |ws, cx| {
                    ws.sidebar_ui.schema_loading = false;
                    match result {
                        Ok((schemas, objects, selected)) => {
                            ws.controller.cached_schemas = schemas;
                            ws.controller.cached_objects = objects;
                            ws.sidebar_ui.selected_schema = selected;
                            ws.sidebar_ui.table_details.clear();
                            ws.controller.rebuild_sidebar_rows();
                            let expanded: Vec<String> = ws
                                .sidebar_ui
                                .expanded_keys
                                .iter()
                                .map(|k| k.to_string())
                                .collect();
                            for key in expanded {
                                maybe_load_expanded_table_details(ws, &key, cx);
                            }
                        }
                        Err(err) => {
                            let msg = ws.controller.strings.schema_refresh_failed_toast;
                            ws.notify_toast(ToastSeverity::Error, msg, Some(err.into()), cx);
                        }
                    }
                    cx.notify();
                });
            }
        },
    )
    .detach();
}

fn maybe_load_expanded_table_details(
    ws: &mut TableMinusWorkspace,
    node_key: &str,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let Some(obj) = ws
        .controller
        .cached_objects
        .iter()
        .find(|o| object_node_key(o) == node_key)
        .cloned()
    else {
        return;
    };
    if !is_tabular_object(obj.kind) {
        return;
    }
    if matches!(
        ws.sidebar_ui.table_details.get(node_key),
        Some(TableDetailsState::Loading | TableDetailsState::Loaded { .. })
    ) {
        return;
    }
    let Some(services) = ws.controller.services.clone() else {
        return;
    };

    let key_owned = node_key.to_owned();
    ws.sidebar_ui
        .table_details
        .insert(key_owned.clone(), TableDetailsState::Loading);

    let workspace_id = ws.controller.active_workspace;
    let session_unavailable_err = ws.controller.strings.session_unavailable_err;
    let table_ref = TableRef::new(obj.schema, obj.name);

    cx.spawn(
        async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let svc = services.clone();
            drop(services.runtime.spawn(async move {
                let guard = svc.sessions.read().await;
                let Some(session) = guard.session(workspace_id) else {
                    let _ = tx.send(Err(session_unavailable_err.to_owned()));
                    return;
                };
                let columns = match session.columns(table_ref.clone()).await {
                    Ok(cols) => cols.to_vec(),
                    Err(err) => {
                        let _ = tx.send(Err(err.to_string()));
                        return;
                    }
                };
                let indexes = session
                    .run(OperationKind::Metadata, |lease, opts| {
                        let conn = lease.connection().clone();
                        let tbl = table_ref.clone();
                        async move {
                            conn.fetch_indexes(&tbl, opts)
                                .await
                                .map_err(OperationError::from)
                        }
                    })
                    .await
                    .unwrap_or_default();
                let _ = tx.send(Ok((columns, indexes)));
            }));

            if let Ok(result) = rx.await {
                let _ = this.update(cx, |ws, cx| {
                    let entry = match result {
                        Ok((columns, indexes)) => TableDetailsState::Loaded { columns, indexes },
                        Err(err) => TableDetailsState::Failed(err),
                    };
                    ws.sidebar_ui.table_details.insert(key_owned, entry);
                    cx.notify();
                });
            }
        },
    )
    .detach();
}

fn render_schema_skeletons(cx: &Context<TableMinusWorkspace>) -> AnyElement {
    let anim = if is_reduced_motion(cx) {
        SkeletonAnimation::None
    } else {
        SkeletonAnimation::Shimmer
    };

    let mut stack = div().flex().flex_col().gap(px(8.0)).p(px(12.0));
    for (ix, width) in [180.0_f32, 150.0, 200.0, 140.0, 170.0, 160.0]
        .into_iter()
        .enumerate()
    {
        stack = stack.child(
            Skeleton::new()
                .id(("sidebar-skel", ix))
                .w(px(width))
                .h(px(22.0))
                .animation_type(anim),
        );
    }
    stack.into_any_element()
}

fn render_disconnected_card(
    s: &crate::i18n::Strings,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .p(px(12.0))
        .child(
            Card::new()
                .variant(CardVariant::Secondary)
                .radius(px(PANEL_RADIUS_PX))
                .child(
                    CardHeader::new()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(COMPACT_GAP_PX))
                                .child(
                                    Icon::new(IconName::Database)
                                        .size(IconSize::Md)
                                        .color(colors.muted),
                                )
                                .child(CardTitle::new().child(s.no_active_connection)),
                        )
                        .child(CardDescription::new().child(s.no_active_connection_desc)),
                )
                .child(
                    CardContent::new().child(
                        Button::new("sidebar-open-connections")
                            .label(s.open_connections)
                            .variant(Variant::Primary)
                            .size(Size::Sm)
                            .full_width(true)
                            .on_press(cx.listener(|ws, _, _, cx| {
                                ws.switch_screen(ScreenMode::ConnectionsLibrary, cx);
                            })),
                    ),
                ),
        )
        .into_any_element()
}

fn render_unavailable_sidebar_card(
    s: &crate::i18n::Strings,
    summary: &str,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .p(px(12.0))
        .child(
            Card::new()
                .variant(CardVariant::Secondary)
                .radius(px(PANEL_RADIUS_PX))
                .child(
                    CardHeader::new()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(COMPACT_GAP_PX))
                                .child(
                                    Icon::new(IconName::TriangleAlert)
                                        .size(IconSize::Md)
                                        .color(colors.danger.color),
                                )
                                .child(CardTitle::new().child(s.unavailable_long)),
                        )
                        .child(CardDescription::new().child(summary.to_owned())),
                )
                .child(
                    CardContent::new().child(
                        Button::new("sidebar-unavailable-connections")
                            .label(s.open_connections)
                            .variant(Variant::Secondary)
                            .size(Size::Sm)
                            .full_width(true)
                            .on_press(cx.listener(|ws, _, _, cx| {
                                ws.switch_screen(ScreenMode::ConnectionsLibrary, cx);
                            })),
                    ),
                ),
        )
        .into_any_element()
}

fn render_empty_schema_card(
    s: &crate::i18n::Strings,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    div()
        .p(px(12.0))
        .child(
            Card::new()
                .variant(CardVariant::Secondary)
                .child(
                    CardHeader::new()
                        .child(CardTitle::new().child(s.schema_empty_title))
                        .child(CardDescription::new().child(s.schema_empty_desc)),
                )
                .child(
                    CardContent::new().child(
                        Button::new("sidebar-empty-refresh")
                            .label(s.refresh_schema_short)
                            .variant(Variant::Secondary)
                            .size(Size::Sm)
                            .full_width(true)
                            .on_press(cx.listener(|ws, _, _, cx| {
                                let schema = resolve_active_schema(ws);
                                spawn_schema_refresh(ws, schema, cx);
                            })),
                    ),
                ),
        )
        .into_any_element()
}

fn render_empty_filtered_card(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let reason = if ws.controller.sidebar_favorites_only {
        s.no_matching_favorites_desc
    } else {
        s.no_matching_objects_desc
    };

    div()
        .p(px(12.0))
        .child(
            Card::new()
                .variant(CardVariant::Secondary)
                .child(
                    CardHeader::new()
                        .child(CardTitle::new().child(s.no_matching_objects_title))
                        .child(CardDescription::new().child(reason)),
                )
                .child(
                    CardContent::new().child(
                        Button::new("sidebar-clear-filter")
                            .label(s.reset_filter_btn)
                            .variant(Variant::Secondary)
                            .size(Size::Sm)
                            .full_width(true)
                            .on_press(cx.listener(|ws, _, _, cx| {
                                ws.controller.sidebar_favorites_only = false;
                                ws.sidebar_ui.last_synced_filter.clear();
                                ws.sidebar_ui.filter_input.update(cx, |input, cx| {
                                    input.set_value("");
                                    cx.notify();
                                });
                                ws.controller.set_sidebar_filter("");
                                cx.notify();
                            })),
                    ),
                ),
        )
        .into_any_element()
}

fn filtered_objects(ws: &TableMinusWorkspace) -> Vec<&ObjectInfo> {
    let filter = Filter::new(Sensitivity::Base);
    let query = ws.controller.sidebar_filter.trim();
    ws.controller
        .cached_objects
        .iter()
        .filter(|obj| {
            if ws.controller.sidebar_favorites_only
                && !ws.controller.favorite_objects.contains(&obj.name)
            {
                return false;
            }
            if query.is_empty() {
                return true;
            }
            filter.contains(&obj.name, query)
                || obj
                    .schema
                    .as_deref()
                    .is_some_and(|s| filter.contains(s, query))
        })
        .collect()
}

fn resolve_active_schema(ws: &TableMinusWorkspace) -> Option<String> {
    if let Some(ref selected) = ws.sidebar_ui.selected_schema
        && ws
            .controller
            .cached_schemas
            .iter()
            .any(|s| &s.name == selected)
    {
        return Some(selected.clone());
    }
    ws.controller
        .cached_schemas
        .iter()
        .find(|s| s.is_default)
        .or_else(|| ws.controller.cached_schemas.first())
        .map(|s| s.name.clone())
}

fn resolve_selected_object(ws: &TableMinusWorkspace) -> Option<&ObjectInfo> {
    let key = ws.sidebar_ui.selected_node_key.as_deref()?;
    ws.controller
        .cached_objects
        .iter()
        .find(|obj| object_node_key(obj) == key)
}

fn object_node_key(obj: &ObjectInfo) -> String {
    let schema = obj.schema.as_deref().unwrap_or("");
    format!("obj:{:?}:{schema}:{}", obj.kind, obj.name)
}

fn qualified_name(obj: &ObjectInfo) -> String {
    match obj.schema.as_deref().map(str::trim) {
        Some(schema) if !schema.is_empty() => format!("{schema}.{}", obj.name),
        _ => obj.name.clone(),
    }
}

const fn is_tabular_object(kind: ObjectKind) -> bool {
    matches!(
        kind,
        ObjectKind::Table
            | ObjectKind::PartitionedTable
            | ObjectKind::ForeignTable
            | ObjectKind::View
            | ObjectKind::MaterializedView
    )
}

const fn object_icon(kind: ObjectKind) -> IconName {
    match kind {
        ObjectKind::Table | ObjectKind::PartitionedTable | ObjectKind::ForeignTable => {
            IconName::Table
        }
        ObjectKind::View | ObjectKind::MaterializedView => IconName::Eye,
        ObjectKind::Function | ObjectKind::Procedure | ObjectKind::Trigger => IconName::Code,
        ObjectKind::Sequence | ObjectKind::Dictionary | ObjectKind::Key(_) | _ => IconName::Key,
    }
}

fn truncate_middle(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars || max_chars < 6 {
        return text.to_owned();
    }
    let keep = max_chars.saturating_sub(1);
    let head = keep / 2 + (keep % 2);
    let tail = keep - head;
    let prefix: String = text.chars().take(head).collect();
    let suffix: String = text.chars().skip(total.saturating_sub(tail)).collect();
    format!("{prefix}…{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::console_log::ConsoleLogKind;
    use tableminus_gfx::glyphs::EngineKind;

    #[test]
    fn dialect_queries_cover_all_nine_engines() {
        for engine in EngineKind::ALL {
            let entries = build_table_open_queries(engine, None, "orders", &[], 100, 0);
            assert!(
                entries.len() >= 2,
                "engine {engine:?} should emit at least query + count"
            );
            assert_eq!(entries[0].0, ConsoleLogKind::Query);
            assert_eq!(entries[1].0, ConsoleLogKind::Count);
            if !matches!(engine, EngineKind::Redis) {
                assert_eq!(entries.len(), 3);
                assert_eq!(entries[2].0, ConsoleLogKind::Schema);
            }
        }
    }

    #[test]
    fn quotes_special_identifiers_and_non_default_schemas() {
        let pg = build_table_open_queries(
            EngineKind::Postgresql,
            Some("analytics"),
            "user \"events\" 2026",
            &[("created at", false)],
            300,
            600,
        );
        assert_eq!(
            pg[0].1,
            "SELECT * FROM \"analytics\".\"user \"\"events\"\" 2026\" ORDER BY \"created at\" DESC LIMIT 300 OFFSET 600;"
        );
        assert!(pg[2].1.contains("table_schema = 'analytics'"));

        let mysql = build_table_open_queries(
            EngineKind::Mysql,
            Some("sales"),
            "order`items",
            &[("id", true)],
            50,
            0,
        );
        assert_eq!(
            mysql[0].1,
            "SELECT * FROM `sales`.`order``items` ORDER BY `id` ASC LIMIT 50 OFFSET 0;"
        );
        assert_eq!(mysql[2].1, "SHOW FULL COLUMNS FROM `sales`.`order``items`;");

        let mssql = build_table_open_queries(
            EngineKind::Mssql,
            Some("reporting"),
            "my]table",
            &[("tenant_id", true), ("seq", false)],
            200,
            400,
        );
        assert_eq!(
            mssql[0].1,
            "SELECT * FROM [reporting].[my]]table] ORDER BY [tenant_id] ASC, [seq] DESC OFFSET 400 ROWS FETCH NEXT 200 ROWS ONLY;"
        );
        assert!(mssql[2].1.contains("TABLE_SCHEMA = 'reporting'"));
    }

    #[test]
    fn omits_order_by_when_no_sort_or_pk_and_handles_mssql_top() {
        let sqlite =
            build_table_open_queries(EngineKind::Sqlite, None, "stress_events_10m", &[], 100, 0);
        assert_eq!(
            sqlite[0].1,
            "SELECT * FROM \"stress_events_10m\" LIMIT 100 OFFSET 0;"
        );
        assert!(!sqlite[0].1.contains("ORDER BY"));
        assert_eq!(sqlite[2].1, "PRAGMA table_info(\"stress_events_10m\");");

        let mssql = build_table_open_queries(EngineKind::Mssql, None, "heap_logs", &[], 250, 0);
        assert_eq!(mssql[0].1, "SELECT TOP 250 * FROM [heap_logs];");
    }
}
