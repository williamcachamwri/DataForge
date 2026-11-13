//! Object explorer sidebar (schemas, tables, views, columns, indexes, functions, favorites, filter).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dataforge_state::connection_window::ConnectionWindowPhase;
use dataforge_state::tabs::TabConnectionId;
use herogpui::components::{
    Badge, BadgeLabel, BadgeVariant, Button, Card, CardContent, CardDescription, CardHeader,
    CardTitle, CardVariant, Chip, ChipLabel, ChipVariant, Color, ContextMenu, Dropdown,
    DropdownPlacement, Filter, Icon, IconName, IconSize, InputState, MenuItem, PickerItem,
    SearchField, Select, Sensitivity, Separator, Size, Skeleton, Spinner, SpinnerSize, TabItem,
    Tabs, TabsSize, TabsVariant, Tooltip, TreeItem, TreeView, Variant,
};
use herogpui::core::SelectionMode;
use herogpui::gpui::{
    AnyElement, App, AppContext as _, AsyncApp, ClipboardItem, Context, Entity, Focusable as _,
    FontFeatures, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, WeakEntity,
    Window, div, px,
};
use herogpui::theme::{ActiveTheme, SkeletonAnimation};
use tablepro_core::column::ColumnInfo;
use tablepro_core::meta::{IndexInfo, ObjectInfo, ObjectKind, ObjectScope, TableRef};
use tablepro_session::{OperationError, OperationKind};

use super::DataForgeWorkspace;
use super::engine_icons::engine_icon;
use super::shell_state::{ToastSeverity, WorkspaceSubView};
use super::theme::{
    COMPACT_GAP_PX, HAIRLINE_BORDER_PX, MICRO_GAP_PX, PANEL_RADIUS_PX, hairline_border_color,
    is_reduced_motion, panel_surface_bg,
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
    selected_schema: Option<String>,
    selected_node_key: Option<String>,
    expanded_keys: HashSet<SharedString>,
    table_details: HashMap<String, TableDetailsState>,
    schema_loading: bool,
    copied_object: Option<(String, Instant)>,
}

impl SidebarPanelState {
    /// Initializes sidebar state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, cx: &mut Context<DataForgeWorkspace>) -> Self {
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
        }
    }

    /// Requests focus on the sidebar filter input (`Ctrl+F`).
    pub fn focus_filter(&mut self, window: &mut Window, cx: &mut Context<DataForgeWorkspace>) {
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
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
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
        ConnectionWindowPhase::Disconnected => render_disconnected_card(cx),
        ConnectionWindowPhase::Unavailable(view) => {
            render_unavailable_sidebar_card(&view.summary, cx)
        }
    };

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
        .child(render_sidebar_header(ws, &phase, cx))
        .child(Separator::new())
        .child(div().flex_1().min_h_0().flex().flex_col().child(body))
        .into_any_element()
}

fn render_sidebar_header(
    ws: &mut DataForgeWorkspace,
    phase: &ConnectionWindowPhase,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let is_connected = matches!(phase, ConnectionWindowPhase::Connected);
    let is_loading = ws.sidebar_ui.schema_loading
        || matches!(
            phase,
            ConnectionWindowPhase::Connecting { .. } | ConnectionWindowPhase::Reconnecting { .. }
        );

    let total_objects = ws.controller.cached_objects.len();
    let count_badge = Badge::new()
        .variant(BadgeVariant::Soft)
        .color(if is_connected {
            Color::Accent
        } else {
            Color::Default
        })
        .size(Size::Sm)
        .child(
            BadgeLabel::new()
                .child(format!("{total_objects}"))
                .sx(|d| d.font_features(tabular_font_features())),
        );

    let refresh_btn = render_refresh_button(is_connected, is_loading, cx);

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
                                .text_xs()
                                .font_weight(herogpui::gpui::FontWeight::SEMIBOLD)
                                .text_color(colors.foreground)
                                .child("Objects"),
                        )
                        .child(count_badge),
                )
                .child(refresh_btn),
        );

    if is_connected {
        header = header
            .child(render_database_and_schema_controls(ws, cx))
            .child(render_filter_controls(ws, cx));
    }

    header.into_any_element()
}

fn render_refresh_button(
    is_connected: bool,
    is_loading: bool,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let reduced_motion = is_reduced_motion(cx);
    let btn = Button::new("sidebar-refresh-schemas")
        .variant(Variant::Tertiary)
        .size(Size::Sm)
        .is_icon_only(true)
        .aria_label("Refresh schemas")
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

    Tooltip::new("Refresh schemas & objects")
        .child(btn)
        .into_any_element()
}

fn render_database_and_schema_controls(
    ws: &mut DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let engine = ws.controller.connection_form.engine;
    let db_name = if !ws.controller.toolbar_status.database.trim().is_empty() {
        ws.controller.toolbar_status.database.trim().to_owned()
    } else if !ws.controller.connection_form.database.trim().is_empty() {
        ws.controller.connection_form.database.trim().to_owned()
    } else {
        engine.display_name().to_owned()
    };

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
        schema_items.push(PickerItem::new("default", "Default schema"));
    }

    let selected_key = active_schema.unwrap_or_else(|| "default".to_owned());
    let disabled_select = ws.controller.cached_schemas.is_empty() || ws.sidebar_ui.schema_loading;

    let schema_select = Select::new("sidebar-schema-select", schema_items)
        .aria_label("Active schema")
        .placeholder("Select schema")
        .value(Some(SharedString::from(selected_key)))
        .full_width(true)
        .height(px(28.0))
        .trigger_text_size(px(12.0))
        .row_text_size(px(12.0))
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
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(MICRO_GAP_PX))
                .min_w_0()
                .child(engine_icon(engine, px(13.0), colors.muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(colors.muted)
                        .child(truncate_middle(&db_name, 26)),
                ),
        )
        .child(schema_select)
        .into_any_element()
}

fn render_filter_controls(
    ws: &mut DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let fav_count = ws
        .controller
        .cached_objects
        .iter()
        .filter(|obj| ws.controller.favorite_objects.contains(&obj.name))
        .count();

    let fav_label = if fav_count > 0 {
        format!("Favorites ({fav_count})")
    } else {
        "Favorites".to_owned()
    };

    let ws_for_clear = cx.entity().downgrade();
    let search = SearchField::new(ws.sidebar_ui.filter_input.clone())
        .placeholder("Filter objects (Ctrl+F)")
        .full_width()
        .height(px(30.0))
        .text_size(px(12.0))
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

    let active_tab = if ws.controller.sidebar_favorites_only {
        "favorites"
    } else {
        "all"
    };

    let tabs = Tabs::new(
        "sidebar-scope-tabs",
        vec![
            TabItem::new("all", "All"),
            TabItem::new("favorites", fav_label),
        ],
        "all",
    )
    .selected_key(active_tab)
    .variant(TabsVariant::Secondary)
    .size(TabsSize::Sm)
    .full_width(true)
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
        .into_any_element()
}

fn render_connected_body(
    ws: &mut DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    if ws.sidebar_ui.schema_loading && ws.controller.cached_objects.is_empty() {
        return render_schema_skeletons(cx);
    }

    let filtered = filtered_objects(ws);
    if ws.controller.cached_objects.is_empty() {
        return render_empty_schema_card(cx);
    }
    if filtered.is_empty() {
        return render_empty_filtered_card(ws, cx);
    }

    let counts = SectionCounts::from_objects(&filtered);
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

    let ctx_items = build_object_menu_items(selected_object.as_ref(), ws);
    let tree_with_ctx = ContextMenu::new(
        "sidebar-tree-context-menu",
        div()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .p(px(6.0))
            .child(tree),
        ctx_items,
    )
    .on_action(cx.listener(|ws, action_key: &SharedString, _, cx| {
        execute_node_action(ws, action_key.as_ref(), cx);
    }));

    let mut root = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(render_count_badges_strip(counts, cx))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(tree_with_ctx),
        );

    if let Some(ref obj) = selected_object {
        root = root
            .child(Separator::new())
            .child(render_selected_node_bar(ws, obj, cx));
    }

    root.into_any_element()
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
    counts: SectionCounts,
    cx: &Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(MICRO_GAP_PX))
        .px(px(10.0))
        .py(px(6.0))
        .border_b(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .child(summary_chip("Tables", counts.tables, colors.muted))
        .child(summary_chip("Views", counts.views, colors.muted))
        .child(summary_chip("Routines", counts.routines, colors.muted))
        .into_any_element()
}

fn summary_chip(label: &str, count: usize, _muted: herogpui::gpui::Hsla) -> AnyElement {
    Chip::new()
        .variant(ChipVariant::Secondary)
        .size(Size::Sm)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(ChipLabel::new().child(label.to_owned()))
                .child(
                    Badge::new()
                        .variant(BadgeVariant::Soft)
                        .size(Size::Sm)
                        .child(
                            BadgeLabel::new()
                                .child(format!("{count}"))
                                .sx(|d| d.font_features(tabular_font_features())),
                        ),
                ),
        )
        .into_any_element()
}

fn build_tree_items(ws: &DataForgeWorkspace, objects: &[&ObjectInfo]) -> Vec<TreeItem> {
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

    let mut roots = Vec::new();
    if !tables.is_empty() {
        roots.push(
            TreeItem::new(SECTION_TABLES, format!("Tables ({})", tables.len()))
                .icon(IconName::Folder.path())
                .children(tables),
        );
    }
    if !views.is_empty() {
        roots.push(
            TreeItem::new(SECTION_VIEWS, format!("Views ({})", views.len()))
                .icon(IconName::Folder.path())
                .children(views),
        );
    }
    if !routines.is_empty() {
        roots.push(
            TreeItem::new(SECTION_ROUTINES, format!("Functions ({})", routines.len()))
                .icon(IconName::Folder.path())
                .children(routines),
        );
    }
    if !other.is_empty() {
        roots.push(
            TreeItem::new(SECTION_OTHER, format!("Sequences & Keys ({})", other.len()))
                .icon(IconName::Folder.path())
                .children(other),
        );
    }
    roots
}

fn build_object_tree_item(ws: &DataForgeWorkspace, obj: &ObjectInfo) -> TreeItem {
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
                        format!("Columns ({})", col_items.len()),
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
                            format!("Indexes ({})", idx_items.len()),
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
                TreeItem::new(format!("loading:{key}"), "Expand to load columns...")
                    .icon(IconName::LoaderCircle.path())
                    .is_disabled(true),
            ],
        };
        item = item.children(children);
    }

    item
}

fn render_selected_node_dropdown(
    ws: &DataForgeWorkspace,
    obj: &ObjectInfo,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let muted = cx.colors().muted;
    Dropdown::uncontrolled(
        "sidebar-selected-dropdown",
        Button::new("sidebar-selected-more")
            .variant(Variant::Tertiary)
            .size(Size::Sm)
            .is_icon_only(true)
            .aria_label("Object actions")
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
    ws: &mut DataForgeWorkspace,
    obj: &ObjectInfo,
    qualified: &str,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let is_fav = ws.controller.favorite_objects.contains(&obj.name);
    let recently_copied = ws
        .sidebar_ui
        .copied_object
        .as_ref()
        .is_some_and(|(name, at)| name == qualified && at.elapsed() < Duration::from_secs(2));

    let fav_btn = Tooltip::new(if is_fav {
        "Remove favorite"
    } else {
        "Add favorite"
    })
    .child(
        Button::new("sidebar-selected-fav")
            .variant(Variant::Tertiary)
            .size(Size::Sm)
            .is_icon_only(true)
            .aria_label("Toggle favorite")
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

    let copy_btn = Tooltip::new("Copy qualified name").child(
        Button::new("sidebar-selected-copy")
            .variant(Variant::Tertiary)
            .size(Size::Sm)
            .is_icon_only(true)
            .aria_label("Copy qualified name")
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
    ws: &mut DataForgeWorkspace,
    obj: &ObjectInfo,
    cx: &mut Context<DataForgeWorkspace>,
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
                        .label("Data")
                        .variant(Variant::Secondary)
                        .size(Size::Sm)
                        .grow(true)
                        .on_press(cx.listener(|ws, _, _, cx| {
                            execute_node_action(ws, "open_data", cx);
                        })),
                )
                .child(
                    Button::new("sidebar-open-structure")
                        .label("Structure")
                        .variant(Variant::Secondary)
                        .size(Size::Sm)
                        .grow(true)
                        .on_press(cx.listener(|ws, _, _, cx| {
                            execute_node_action(ws, "view_structure", cx);
                        })),
                ),
        );
    }

    bar.into_any_element()
}

fn build_object_menu_items(
    selected: Option<&ObjectInfo>,
    ws: &DataForgeWorkspace,
) -> Vec<MenuItem> {
    let has_table = selected.is_some_and(|obj| is_tabular_object(obj.kind));
    let has_any = selected.is_some();
    let is_fav = selected.is_some_and(|obj| ws.controller.favorite_objects.contains(&obj.name));

    let mut items = Vec::new();
    if has_table {
        items.push(MenuItem::new("open_data", "Open Table Data").icon(IconName::Table.path()));
        items.push(
            MenuItem::new("view_structure", "View Structure").icon(IconName::Columns2.path()),
        );
        items.push(MenuItem::new("new_select", "New SELECT Query").icon(IconName::FileCode.path()));
        items.push(MenuItem::Separator);
    }
    if has_any {
        items.push(
            MenuItem::new("copy_qualified", "Copy Qualified Name").icon(IconName::Copy.path()),
        );
        items.push(
            MenuItem::new(
                "toggle_favorite",
                if is_fav {
                    "Remove from Favorites"
                } else {
                    "Add to Favorites"
                },
            )
            .icon(IconName::Star.path()),
        );
        items.push(MenuItem::Separator);
    }
    items.push(MenuItem::new("refresh_schema", "Refresh Schema").icon(IconName::RefreshCw.path()));
    items
}

fn execute_node_action(
    ws: &mut DataForgeWorkspace,
    action: &str,
    cx: &mut Context<DataForgeWorkspace>,
) {
    let selected = resolve_selected_object(ws).cloned();
    match action {
        "open_data" => {
            let Some(obj) = selected else { return };
            let table_ref = TableRef::new(obj.schema.clone(), obj.name.clone());
            match ws.controller.open_table_tab_blocking(table_ref) {
                Ok(()) => {
                    ws.workspace_pane.sub_view = WorkspaceSubView::DataGrid;
                    cx.notify();
                }
                Err(err) => {
                    ws.notify_toast(
                        ToastSeverity::Error,
                        "Open Table Failed",
                        Some(err.to_string().into()),
                        cx,
                    );
                }
            }
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
                    ws.notify_toast(
                        ToastSeverity::Error,
                        "Load Structure Failed",
                        Some(err.to_string().into()),
                        cx,
                    );
                }
            }
        }
        "new_select" => {
            let Some(obj) = selected else { return };
            let qualified = qualified_name(&obj);
            let sql = format!("SELECT *\nFROM {qualified}\nLIMIT 100;\n");
            let conn_id = TabConnectionId(format!("ws-{}", ws.controller.active_workspace.0));
            ws.controller
                .tabs
                .open_query_tab(conn_id, Some(&format!("Query - {}", obj.name)));
            ws.controller.set_editor_sql(&sql);
            ws.workspace_pane.sub_view = WorkspaceSubView::SqlEditor;
            ws.notify_toast(
                ToastSeverity::Info,
                "Query Ready",
                Some(qualified.into()),
                cx,
            );
            cx.notify();
        }
        "copy_qualified" => {
            let Some(obj) = selected else { return };
            let qualified = qualified_name(&obj);
            cx.write_to_clipboard(ClipboardItem::new_string(qualified.clone()));
            ws.sidebar_ui.copied_object = Some((qualified.clone(), Instant::now()));
            ws.notify_toast(ToastSeverity::Success, "Copied", Some(qualified.into()), cx);
            cx.notify();
        }
        "toggle_favorite" => {
            let Some(obj) = selected else { return };
            ws.controller.toggle_object_favorite(&obj.name);
            let added = ws.controller.favorite_objects.contains(&obj.name);
            let title = if added {
                "Added to Favorites"
            } else {
                "Removed from Favorites"
            };
            ws.notify_toast(ToastSeverity::Info, title, Some(obj.name.into()), cx);
            cx.notify();
        }
        "refresh_schema" => {
            let schema = resolve_active_schema(ws);
            spawn_schema_refresh(ws, schema, cx);
        }
        _ => {}
    }
}

fn spawn_schema_refresh(
    ws: &mut DataForgeWorkspace,
    schema_override: Option<String>,
    cx: &mut Context<DataForgeWorkspace>,
) {
    let Some(services) = ws.controller.services.clone() else {
        ws.notify_toast(
            ToastSeverity::Warning,
            "Unavailable",
            Some("Runtime services are not initialized.".into()),
            cx,
        );
        return;
    };
    let workspace_id = ws.controller.active_workspace;
    ws.sidebar_ui.schema_loading = true;
    cx.notify();

    cx.spawn(
        async move |this: WeakEntity<DataForgeWorkspace>, cx: &mut AsyncApp| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let svc = services.clone();
            drop(services.runtime.spawn(async move {
                let guard = svc.sessions.read().await;
                let Some(session) = guard.session(workspace_id) else {
                    let _ = tx.send(Err("No active session for workspace".to_owned()));
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
                        }
                        Err(err) => {
                            ws.notify_toast(
                                ToastSeverity::Error,
                                "Schema Refresh Failed",
                                Some(err.into()),
                                cx,
                            );
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
    ws: &mut DataForgeWorkspace,
    node_key: &str,
    cx: &mut Context<DataForgeWorkspace>,
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
    let table_ref = TableRef::new(obj.schema, obj.name);

    cx.spawn(
        async move |this: WeakEntity<DataForgeWorkspace>, cx: &mut AsyncApp| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let svc = services.clone();
            drop(services.runtime.spawn(async move {
                let guard = svc.sessions.read().await;
                let Some(session) = guard.session(workspace_id) else {
                    let _ = tx.send(Err("Session unavailable".to_owned()));
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

fn render_schema_skeletons(cx: &Context<DataForgeWorkspace>) -> AnyElement {
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

fn render_disconnected_card(cx: &mut Context<DataForgeWorkspace>) -> AnyElement {
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
                                .child(CardTitle::new().child("No Active Connection")),
                        )
                        .child(CardDescription::new().child(
                            "Connect to a database or open a local file to browse objects.",
                        )),
                )
                .child(
                    CardContent::new().child(
                        Button::new("sidebar-open-connections")
                            .label("Open Connections")
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
    summary: &str,
    cx: &mut Context<DataForgeWorkspace>,
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
                                .child(CardTitle::new().child("Connection Unavailable")),
                        )
                        .child(CardDescription::new().child(summary.to_owned())),
                )
                .child(
                    CardContent::new().child(
                        Button::new("sidebar-unavailable-connections")
                            .label("Open Connections")
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

fn render_empty_schema_card(cx: &mut Context<DataForgeWorkspace>) -> AnyElement {
    div()
        .p(px(12.0))
        .child(
            Card::new()
                .variant(CardVariant::Secondary)
                .child(
                    CardHeader::new()
                        .child(CardTitle::new().child("Schema Is Empty"))
                        .child(
                            CardDescription::new()
                                .child("No tables, views, or routines were returned."),
                        ),
                )
                .child(
                    CardContent::new().child(
                        Button::new("sidebar-empty-refresh")
                            .label("Refresh Schema")
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
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let reason = if ws.controller.sidebar_favorites_only {
        "No favorite objects match the current filter."
    } else {
        "No database objects match the filter text."
    };

    div()
        .p(px(12.0))
        .child(
            Card::new()
                .variant(CardVariant::Secondary)
                .child(
                    CardHeader::new()
                        .child(CardTitle::new().child("No Matching Objects"))
                        .child(CardDescription::new().child(reason)),
                )
                .child(
                    CardContent::new().child(
                        Button::new("sidebar-clear-filter")
                            .label("Reset Filter")
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

fn filtered_objects(ws: &DataForgeWorkspace) -> Vec<&ObjectInfo> {
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

fn resolve_active_schema(ws: &DataForgeWorkspace) -> Option<String> {
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

fn resolve_selected_object(ws: &DataForgeWorkspace) -> Option<&ObjectInfo> {
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
