//! Interactive Entity-Relationship (ER) diagram canvas for tables and foreign keys.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use tableminus_state::connection_window::ConnectionWindowPhase;
use herogpui::components::{Button, Chip, ChipLabel, ChipVariant, Icon, IconName, IconSize};
use herogpui::core::{Color as HeroColor, Size as HeroSize, Variant, with_alpha};
use herogpui::gpui::{
    self, AnyElement, App, AsyncApp, Bounds, ClipboardItem, Context, FontWeight, Hsla,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Path, PathBuilder, Pixels,
    SharedString, StatefulInteractiveElement, Styled, Task, WeakEntity, Window, canvas, div, point,
    px,
};
use herogpui::prelude::FluentBuilder;
use herogpui::theme::ActiveTheme;
use tablepro_core::ddl::TableDefinition;

use super::shell_state::ToastSeverity;
use super::{TableMinusWorkspace, theme};

const CARD_WIDTH_PX: f32 = 248.0;
const CARD_HEADER_HEIGHT_PX: f32 = 36.0;
const CARD_ROW_HEIGHT_PX: f32 = 23.0;
const CARD_PADDING_BOTTOM_PX: f32 = 8.0;
const MAX_VISIBLE_COLS: usize = 12;
const MIN_ZOOM: f32 = 0.4;
const MAX_ZOOM: f32 = 2.5;
const ZOOM_STEP: f32 = 0.15;
const GRID_STEP_PX: f32 = 24.0;

/// Single column metadata entry displayed inside an ER diagram table card.
#[derive(Debug, Clone)]
pub struct ErColumnItem {
    pub name: String,
    pub data_type: String,
    pub is_pk: bool,
    pub is_fk: bool,
    pub fk_target: Option<String>,
    pub nullable: bool,
}

/// Positioned table node rendered on the ER diagram canvas.
#[derive(Debug, Clone)]
pub struct ErTableNode {
    pub key: String,
    pub schema: Option<String>,
    pub name: String,
    pub columns: Vec<ErColumnItem>,
    pub has_inbound_fk: bool,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Directed foreign-key relationship (`from_key` child -> `to_key` parent).
#[derive(Debug, Clone)]
pub struct ErRelationEdge {
    pub name: String,
    pub from_key: String,
    pub from_cols: Vec<String>,
    pub to_key: String,
    pub to_cols: Vec<String>,
}

/// Active card drag gesture state.
#[derive(Debug, Clone)]
pub struct ErNodeDrag {
    pub key: String,
    pub start_mouse_x: f32,
    pub start_mouse_y: f32,
    pub start_node_x: f32,
    pub start_node_y: f32,
}

/// Active canvas pan gesture state.
#[derive(Debug, Clone, Copy)]
pub struct ErCanvasPan {
    pub start_mouse_x: f32,
    pub start_mouse_y: f32,
    pub start_pan_x: f32,
    pub start_pan_y: f32,
}

/// Persistent UI state for the ER Diagram workbench view.
#[derive(Debug, Default)]
pub struct ErDiagramState {
    pub zoom: f32,
    pub pan_x: f32,
    pub pan_y: f32,
    pub nodes: Vec<ErTableNode>,
    pub edges: Vec<ErRelationEdge>,
    pub schemas: Vec<String>,
    pub schema_filter: Option<String>,
    pub selected_table: Option<String>,
    pub hovered_table: Option<String>,
    pub dragging_node: Option<ErNodeDrag>,
    pub panning_canvas: Option<ErCanvasPan>,
    pub loaded_for_connection: Option<String>,
    pub last_error: Option<String>,
    pub export_task: Option<Task<()>>,
}

impl ErDiagramState {
    /// Creates initial ER Diagram state owned by [`TableMinusWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, _cx: &mut Context<TableMinusWorkspace>) -> Self {
        Self {
            zoom: 1.0,
            pan_x: 36.0,
            pan_y: 32.0,
            ..Self::default()
        }
    }
}

/// Renders the ER Diagram workbench tool view.
#[must_use]
pub fn render_er_diagram(
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
    maybe_autoload_schema(ws, is_connected);

    let header = render_er_header(ws, is_connected, cx);
    let body = if !is_connected || ws.er_diagram_ui.nodes.is_empty() {
        render_er_empty_state(ws, is_connected, cx)
    } else {
        render_er_canvas_stage(ws, cx)
    };

    div()
        .id("er-diagram-view")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme::panel_surface_bg(cx))
        .child(header)
        .child(div().flex_1().min_h_0().w_full().child(body))
        .into_any_element()
}

fn maybe_autoload_schema(ws: &mut TableMinusWorkspace, is_connected: bool) {
    if !is_connected {
        ws.er_diagram_ui.loaded_for_connection = None;
        return;
    }
    let conn_sig = format!(
        "{}::{}::{}",
        ws.controller.toolbar_status.host,
        ws.controller.toolbar_status.database,
        ws.controller.cached_objects.len()
    );
    if ws.er_diagram_ui.loaded_for_connection.as_deref() != Some(conn_sig.as_str())
        && !ws.controller.cached_objects.is_empty()
    {
        ws.er_diagram_ui.loaded_for_connection = Some(conn_sig);
        load_er_schema(ws);
    }
}

fn load_er_schema(ws: &mut TableMinusWorkspace) {
    match ws.controller.fetch_all_table_definitions_blocking() {
        Ok(defs) => {
            let prev_positions: HashMap<String, (f32, f32)> = ws
                .er_diagram_ui
                .nodes
                .iter()
                .map(|n| (n.key.clone(), (n.x, n.y)))
                .collect();
            let (mut nodes, edges, schemas) = build_er_graph(&defs);
            let mut has_all_prev = !nodes.is_empty();
            for node in &mut nodes {
                if let Some(&(px_x, px_y)) = prev_positions.get(&node.key) {
                    node.x = px_x;
                    node.y = px_y;
                } else {
                    has_all_prev = false;
                }
            }
            if !has_all_prev {
                apply_auto_layout(&mut nodes, &edges);
            }
            ws.er_diagram_ui.nodes = nodes;
            ws.er_diagram_ui.edges = edges;
            ws.er_diagram_ui.schemas = schemas;
            ws.er_diagram_ui.last_error = None;
            if let Some(ref filter) = ws.er_diagram_ui.schema_filter
                && !ws.er_diagram_ui.schemas.contains(filter)
            {
                ws.er_diagram_ui.schema_filter = None;
            }
        }
        Err(err) => {
            ws.er_diagram_ui.last_error = Some(err.to_string());
        }
    }
}

fn canonical_table_key(schema: Option<&str>, name: &str) -> String {
    match schema.map(str::trim).filter(|s| !s.is_empty()) {
        Some(sc) => format!("{sc}.{name}"),
        None => name.to_owned(),
    }
}

fn build_er_graph(
    defs: &[TableDefinition],
) -> (Vec<ErTableNode>, Vec<ErRelationEdge>, Vec<String>) {
    let mut key_lookup: HashMap<(Option<String>, String), String> = HashMap::new();
    let mut name_fallback: HashMap<String, String> = HashMap::new();
    let mut schema_set = BTreeSet::new();

    for def in defs {
        let sc = def
            .table
            .schema
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        if let Some(ref s) = sc {
            schema_set.insert(s.clone());
        }
        let key = canonical_table_key(sc.as_deref(), &def.table.name);
        key_lookup.insert((sc, def.table.name.clone()), key.clone());
        name_fallback
            .entry(def.table.name.to_ascii_lowercase())
            .or_insert(key);
    }

    let mut edges = Vec::new();
    let mut inbound_targets = HashSet::new();
    let mut fk_col_targets: HashMap<(String, String), String> = HashMap::new();

    for def in defs {
        let from_sc = def
            .table
            .schema
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let from_key = canonical_table_key(from_sc.as_deref(), &def.table.name);

        for fk in &def.foreign_keys {
            let ref_sc = fk
                .ref_schema
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            let target_key = key_lookup
                .get(&(ref_sc.clone(), fk.ref_table.clone()))
                .or_else(|| key_lookup.get(&(from_sc.clone(), fk.ref_table.clone())))
                .or_else(|| name_fallback.get(&fk.ref_table.to_ascii_lowercase()))
                .cloned();
            let Some(to_key) = target_key else {
                continue;
            };
            inbound_targets.insert(to_key.clone());
            for (idx, col) in fk.columns.iter().enumerate() {
                let target_col = fk.ref_columns.get(idx).map_or_else(
                    || fk.ref_table.clone(),
                    |rc| format!("{}.{rc}", fk.ref_table),
                );
                fk_col_targets.insert((from_key.clone(), col.clone()), target_col);
            }
            edges.push(ErRelationEdge {
                name: fk.name.clone(),
                from_key: from_key.clone(),
                from_cols: fk.columns.clone(),
                to_key,
                to_cols: fk.ref_columns.clone(),
            });
        }
    }

    let nodes = defs
        .iter()
        .map(|def| build_node_from_def(def, &inbound_targets, &fk_col_targets))
        .collect();
    (nodes, edges, schema_set.into_iter().collect())
}

fn build_node_from_def(
    def: &TableDefinition,
    inbound_targets: &HashSet<String>,
    fk_col_targets: &HashMap<(String, String), String>,
) -> ErTableNode {
    let schema = def
        .table
        .schema
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let key = canonical_table_key(schema.as_deref(), &def.table.name);
    let columns: Vec<ErColumnItem> = def
        .columns
        .iter()
        .map(|col| {
            let is_pk = col.info.primary_key
                || def
                    .primary_key
                    .as_ref()
                    .is_some_and(|pk| pk.columns.iter().any(|c| c == &col.info.name));
            let fk_target = fk_col_targets
                .get(&(key.clone(), col.info.name.clone()))
                .cloned();
            ErColumnItem {
                name: col.info.name.clone(),
                data_type: col.info.column_type.name.clone(),
                is_pk,
                is_fk: fk_target.is_some(),
                fk_target,
                nullable: col.info.nullable,
            }
        })
        .collect();
    let visible_rows = columns.len().clamp(1, MAX_VISIBLE_COLS);
    let extra_row = usize::from(columns.len() > MAX_VISIBLE_COLS);
    let height = CARD_HEADER_HEIGHT_PX
        + ((visible_rows + extra_row) as f32) * CARD_ROW_HEIGHT_PX
        + CARD_PADDING_BOTTOM_PX;
    ErTableNode {
        has_inbound_fk: inbound_targets.contains(&key),
        key,
        schema,
        name: def.table.name.clone(),
        columns,
        x: 0.0,
        y: 0.0,
        width: CARD_WIDTH_PX,
        height,
    }
}

/// Topological + degree-ordered layout preventing card overlap across connected components.
fn apply_auto_layout(nodes: &mut [ErTableNode], edges: &[ErRelationEdge]) {
    if nodes.is_empty() {
        return;
    }
    let idx_by_key: HashMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.key.clone(), i))
        .collect();

    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    let mut in_deg: Vec<usize> = vec![0; nodes.len()];
    for edge in edges {
        if let (Some(&u), Some(&v)) = (idx_by_key.get(&edge.from_key), idx_by_key.get(&edge.to_key))
            && u != v
        {
            adj[u].push(v);
            adj[v].push(u);
            in_deg[v] = in_deg[v].saturating_add(1);
        }
    }

    let mut visited = vec![false; nodes.len()];
    let mut connected_comps: Vec<Vec<usize>> = Vec::new();
    let mut isolated: Vec<usize> = Vec::new();

    for start in 0..nodes.len() {
        if visited[start] {
            continue;
        }
        if adj[start].is_empty() {
            visited[start] = true;
            isolated.push(start);
            continue;
        }
        let mut comp = Vec::new();
        let mut queue = VecDeque::new();
        visited[start] = true;
        queue.push_back(start);
        while let Some(cur) = queue.pop_front() {
            comp.push(cur);
            for &nxt in &adj[cur] {
                if !visited[nxt] {
                    visited[nxt] = true;
                    queue.push_back(nxt);
                }
            }
        }
        connected_comps.push(comp);
    }

    connected_comps.sort_by_key(|a| std::cmp::Reverse(a.len()));
    let mut cursor_y = 24.0_f32;
    let h_gap = 92.0_f32;
    let v_gap = 48.0_f32;

    for comp in connected_comps {
        cursor_y = layout_component(nodes, &adj, &in_deg, &comp, cursor_y, h_gap, v_gap);
    }
    if !isolated.is_empty() {
        layout_isolated_grid(nodes, &isolated, cursor_y, h_gap, v_gap);
    }
}

fn layout_component(
    nodes: &mut [ErTableNode],
    adj: &[Vec<usize>],
    in_deg: &[usize],
    comp: &[usize],
    start_y: f32,
    h_gap: f32,
    v_gap: f32,
) -> f32 {
    let root = comp
        .iter()
        .copied()
        .max_by_key(|&i| (in_deg[i], adj[i].len()))
        .unwrap_or(comp[0]);

    let mut level_map: HashMap<usize, usize> = HashMap::new();
    let mut queue = VecDeque::new();
    level_map.insert(root, 0);
    queue.push_back(root);
    while let Some(u) = queue.pop_front() {
        let lvl = level_map.get(&u).copied().unwrap_or(0);
        for &v in &adj[u] {
            if let std::collections::hash_map::Entry::Vacant(e) = level_map.entry(v) {
                e.insert(lvl + 1);
                queue.push_back(v);
            }
        }
    }

    let max_lvl = level_map.values().copied().max().unwrap_or(0);
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); max_lvl + 1];
    for &idx in comp {
        let lvl = level_map.get(&idx).copied().unwrap_or(0);
        layers[lvl].push(idx);
    }
    for layer in &mut layers {
        layer.sort_by(|&a, &b| nodes[a].name.cmp(&nodes[b].name));
    }

    let mut comp_bottom = start_y;
    for (col_idx, layer) in layers.iter().enumerate() {
        let x = 24.0 + (col_idx as f32) * (CARD_WIDTH_PX + h_gap);
        let mut y = start_y;
        for &node_idx in layer {
            nodes[node_idx].x = x;
            nodes[node_idx].y = y;
            y += nodes[node_idx].height + v_gap;
        }
        comp_bottom = comp_bottom.max(y);
    }
    comp_bottom + 12.0
}

fn layout_isolated_grid(
    nodes: &mut [ErTableNode],
    isolated: &[usize],
    start_y: f32,
    h_gap: f32,
    v_gap: f32,
) {
    let mut sorted = isolated.to_vec();
    sorted.sort_by(|&a, &b| nodes[a].name.cmp(&nodes[b].name));
    let cols = 4_usize;
    let mut y = start_y;
    for chunk in sorted.chunks(cols) {
        let mut row_max_h = 0.0_f32;
        for (c, &idx) in chunk.iter().enumerate() {
            nodes[idx].x = 24.0 + (c as f32) * (CARD_WIDTH_PX + h_gap);
            nodes[idx].y = y;
            row_max_h = row_max_h.max(nodes[idx].height);
        }
        y += row_max_h + v_gap;
    }
}

fn render_er_header(
    ws: &mut TableMinusWorkspace,
    is_connected: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    div()
        .min_h(px(42.0))
        .px(px(14.0))
        .py(px(5.0))
        .border_b_1()
        .border_color(theme::hairline_border_color(cx))
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .child(render_er_header_left(ws, cx))
        .child(render_er_header_actions(ws, is_connected, cx))
        .into_any_element()
}

fn render_er_header_actions(
    ws: &mut TableMinusWorkspace,
    is_connected: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let muted = cx.colors().muted;
    let zoom_pct = format!("{}%", (ws.er_diagram_ui.zoom * 100.0).round() as i32);
    let schema_label = ws
        .er_diagram_ui
        .schema_filter
        .clone()
        .unwrap_or_else(|| s.er_filter_schema.to_owned());
    let has_schemas = ws.er_diagram_ui.schemas.len() > 1;
    let has_nodes = !ws.er_diagram_ui.nodes.is_empty();

    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.0))
        .when(has_schemas, |el| {
            el.child(
                Button::new("er-schema-filter-btn")
                    .variant(if ws.er_diagram_ui.schema_filter.is_some() {
                        Variant::Secondary
                    } else {
                        Variant::Tertiary
                    })
                    .size(HeroSize::Sm)
                    .child(Icon::new(IconName::Funnel).size(IconSize::Xs))
                    .label(schema_label)
                    .on_press(cx.listener(|ws, _, _, cx| {
                        cycle_schema_filter(ws);
                        cx.notify();
                    })),
            )
        })
        .child(
            Button::new("er-refresh-schema-btn")
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .is_disabled(!is_connected)
                .child(Icon::new(IconName::RefreshCw).size(IconSize::Xs))
                .label(s.refresh_schema)
                .on_press(cx.listener(|ws, _, _, cx| {
                    load_er_schema(ws);
                    if let Some(err) = ws.er_diagram_ui.last_error.clone() {
                        ws.notify_toast(ToastSeverity::Error, err, None, cx);
                    }
                    cx.notify();
                })),
        )
        .child(
            Button::new("er-auto-layout-btn")
                .variant(Variant::Tertiary)
                .size(HeroSize::Sm)
                .is_disabled(!has_nodes)
                .child(Icon::new(IconName::LayoutGrid).size(IconSize::Xs))
                .label(s.er_auto_layout)
                .on_press(cx.listener(|ws, _, _, cx| {
                    apply_auto_layout(&mut ws.er_diagram_ui.nodes, &ws.er_diagram_ui.edges);
                    ws.er_diagram_ui.pan_x = 36.0;
                    ws.er_diagram_ui.pan_y = 32.0;
                    cx.notify();
                })),
        )
        .child(render_zoom_controls(ws, &zoom_pct, cx))
        .child(
            Button::new("er-export-svg-btn")
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .is_disabled(!has_nodes)
                .child(Icon::new(IconName::Download).size(IconSize::Xs))
                .label(s.er_export_svg)
                .on_press(cx.listener(|ws, _, _, cx| {
                    export_er_svg(ws, cx);
                })),
        )
        .child(
            Button::new("er-diagram-close")
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .is_icon_only(true)
                .aria_label(s.close_label)
                .on_press(cx.listener(|ws, _, _, cx| {
                    ws.workspace_pane.close_tool();
                    cx.notify();
                }))
                .child(Icon::new(IconName::X).size(IconSize::Xs).color(muted)),
        )
        .into_any_element()
}

fn render_er_header_left(ws: &TableMinusWorkspace, cx: &App) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors();
    let table_count = visible_nodes(ws).len();
    let rel_count = visible_edges(ws, &visible_node_keys(ws)).len();

    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            Icon::new(IconName::Share2)
                .size(IconSize::Sm)
                .color(colors.accent.color),
        )
        .child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.foreground)
                .child(s.er_diagram),
        )
        .when(table_count > 0, |el| {
            el.child(
                Chip::new()
                    .variant(ChipVariant::Secondary)
                    .size(HeroSize::Sm)
                    .child(ChipLabel::new().child(format!("{table_count} {}", s.tables))),
            )
            .child(
                Chip::new()
                    .variant(ChipVariant::Soft)
                    .size(HeroSize::Sm)
                    .color(HeroColor::Accent)
                    .child(ChipLabel::new().child(format!("{rel_count} {}", s.er_fk_relations))),
            )
        })
        .into_any_element()
}

fn render_zoom_controls(
    ws: &TableMinusWorkspace,
    zoom_pct: &str,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let border = theme::hairline_border_color(cx);

    div()
        .flex()
        .items_center()
        .gap(px(2.0))
        .px(px(4.0))
        .rounded(px(theme::CONTROL_RADIUS_PX))
        .border_1()
        .border_color(border)
        .bg(theme::muted_surface_bg(cx))
        .child(
            Button::new("er-zoom-out-btn")
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .is_icon_only(true)
                .aria_label(s.er_zoom_out)
                .on_press(cx.listener(|ws, _, _, cx| {
                    ws.er_diagram_ui.zoom =
                        (ws.er_diagram_ui.zoom - ZOOM_STEP).clamp(MIN_ZOOM, MAX_ZOOM);
                    cx.notify();
                }))
                .child(Icon::new(IconName::ZoomOut).size(IconSize::Xs)),
        )
        .child(
            Button::new("er-zoom-reset-btn")
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .aria_label(s.er_zoom_reset)
                .label(zoom_pct.to_owned())
                .on_press(cx.listener(|ws, _, _, cx| {
                    ws.er_diagram_ui.zoom = 1.0;
                    ws.er_diagram_ui.pan_x = 36.0;
                    ws.er_diagram_ui.pan_y = 32.0;
                    cx.notify();
                })),
        )
        .child(
            Button::new("er-zoom-in-btn")
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .is_icon_only(true)
                .aria_label(s.er_zoom_in)
                .on_press(cx.listener(|ws, _, _, cx| {
                    ws.er_diagram_ui.zoom =
                        (ws.er_diagram_ui.zoom + ZOOM_STEP).clamp(MIN_ZOOM, MAX_ZOOM);
                    cx.notify();
                }))
                .child(Icon::new(IconName::ZoomIn).size(IconSize::Xs)),
        )
        .into_any_element()
}

fn cycle_schema_filter(ws: &mut TableMinusWorkspace) {
    let schemas = &ws.er_diagram_ui.schemas;
    if schemas.is_empty() {
        ws.er_diagram_ui.schema_filter = None;
        return;
    }
    ws.er_diagram_ui.schema_filter = match &ws.er_diagram_ui.schema_filter {
        None => schemas.first().cloned(),
        Some(cur) => {
            let idx = schemas.iter().position(|s| s == cur);
            match idx {
                Some(i) if i + 1 < schemas.len() => Some(schemas[i + 1].clone()),
                _ => None,
            }
        }
    };
}

fn render_er_empty_state(
    ws: &mut TableMinusWorkspace,
    is_connected: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let detail = if !is_connected {
        s.no_active_connection_desc.to_owned()
    } else if let Some(err) = &ws.er_diagram_ui.last_error {
        err.clone()
    } else {
        s.er_no_tables.to_owned()
    };

    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.0))
        .p(px(24.0))
        .child(
            div()
                .w(px(44.0))
                .h(px(44.0))
                .rounded(px(12.0))
                .bg(with_alpha(colors.accent.color, 0.12))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(IconName::Share2)
                        .size(IconSize::Md)
                        .color(colors.accent.color),
                ),
        )
        .child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.foreground)
                .child(s.er_no_tables),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(colors.muted)
                .child(detail),
        )
        .when(is_connected, |el| {
            el.child(
                Button::new("er-empty-load-btn")
                    .variant(Variant::Secondary)
                    .size(HeroSize::Sm)
                    .child(Icon::new(IconName::RefreshCw).size(IconSize::Xs))
                    .label(s.refresh_schema)
                    .on_press(cx.listener(|ws, _, _, cx| {
                        load_er_schema(ws);
                        cx.notify();
                    })),
            )
        })
        .into_any_element()
}

fn visible_nodes(ws: &TableMinusWorkspace) -> Vec<&ErTableNode> {
    match &ws.er_diagram_ui.schema_filter {
        Some(filter) => ws
            .er_diagram_ui
            .nodes
            .iter()
            .filter(|n| n.schema.as_deref() == Some(filter.as_str()))
            .collect(),
        None => ws.er_diagram_ui.nodes.iter().collect(),
    }
}

fn visible_node_keys(ws: &TableMinusWorkspace) -> HashSet<String> {
    visible_nodes(ws)
        .into_iter()
        .map(|n| n.key.clone())
        .collect()
}

fn visible_edges<'a>(
    ws: &'a TableMinusWorkspace,
    keys: &HashSet<String>,
) -> Vec<&'a ErRelationEdge> {
    ws.er_diagram_ui
        .edges
        .iter()
        .filter(|e| keys.contains(&e.from_key) && keys.contains(&e.to_key))
        .collect()
}

fn build_er_stage_container(cx: &mut Context<TableMinusWorkspace>) -> gpui::Stateful<gpui::Div> {
    div()
        .id("er-canvas-stage")
        .relative()
        .size_full()
        .overflow_hidden()
        .bg(theme::muted_surface_bg(cx))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|ws, ev: &gpui::MouseDownEvent, _w, cx| {
                ws.er_diagram_ui.selected_table = None;
                ws.er_diagram_ui.panning_canvas = Some(ErCanvasPan {
                    start_mouse_x: f32::from(ev.position.x),
                    start_mouse_y: f32::from(ev.position.y),
                    start_pan_x: ws.er_diagram_ui.pan_x,
                    start_pan_y: ws.er_diagram_ui.pan_y,
                });
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(|ws, ev: &gpui::MouseMoveEvent, _w, cx| {
            handle_canvas_mouse_move(ws, ev, cx);
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|ws, _ev, _w, cx| {
                if ws.er_diagram_ui.dragging_node.is_some()
                    || ws.er_diagram_ui.panning_canvas.is_some()
                {
                    ws.er_diagram_ui.dragging_node = None;
                    ws.er_diagram_ui.panning_canvas = None;
                    cx.notify();
                }
            }),
        )
        .on_scroll(cx.listener(|ws, ev: &gpui::ScrollEvent, _w, cx| {
            let delta = ev.delta.pixel_delta(px(24.0));
            if ev.modifiers.control {
                let step = if f32::from(delta.y) > 0.0 {
                    0.08
                } else {
                    -0.08
                };
                ws.er_diagram_ui.zoom = (ws.er_diagram_ui.zoom + step).clamp(MIN_ZOOM, MAX_ZOOM);
            } else {
                ws.er_diagram_ui.pan_x += f32::from(delta.x);
                ws.er_diagram_ui.pan_y += f32::from(delta.y);
            }
            cx.notify();
        }))
}

fn render_er_canvas_stage(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let zoom = ws.er_diagram_ui.zoom;
    let pan_x = ws.er_diagram_ui.pan_x;
    let pan_y = ws.er_diagram_ui.pan_y;
    let focus_table = ws
        .er_diagram_ui
        .hovered_table
        .clone()
        .or_else(|| ws.er_diagram_ui.selected_table.clone());

    let keys = visible_node_keys(ws);
    let vis_nodes: Vec<ErTableNode> = visible_nodes(ws).into_iter().cloned().collect();
    let vis_edges: Vec<ErRelationEdge> = visible_edges(ws, &keys).into_iter().cloned().collect();
    let node_map: HashMap<String, ErTableNode> = vis_nodes
        .iter()
        .map(|n| (n.key.clone(), n.clone()))
        .collect();

    let colors = cx.colors().clone();
    let dot_color = with_alpha(colors.muted, 0.22);
    let edge_default_color = with_alpha(colors.muted, 0.55);
    let edge_dim_color = with_alpha(colors.muted, 0.22);
    let edge_active_color = colors.accent.color;

    let connector_specs = compute_connector_specs(&vis_edges, &node_map, zoom, pan_x, pan_y);
    let specs_for_canvas = connector_specs.clone();
    let focus_for_canvas = focus_table.clone();

    let mut stage = build_er_stage_container(cx).child(
        canvas(
            move |bounds, _window, _cx| {
                build_canvas_geometry(
                    bounds,
                    zoom,
                    pan_x,
                    pan_y,
                    &specs_for_canvas,
                    focus_for_canvas.as_deref(),
                )
            },
            move |_bounds, geom, window, _cx| {
                paint_canvas_geometry(
                    &geom,
                    dot_color,
                    edge_default_color,
                    edge_dim_color,
                    edge_active_color,
                    window,
                );
            },
        )
        .absolute()
        .inset_0(),
    );

    stage = append_er_stage_overlays(
        stage,
        ws,
        (&connector_specs, &vis_nodes, &vis_edges),
        focus_table.as_deref(),
        (zoom, pan_x, pan_y),
        cx,
    );

    stage.into_any_element()
}

fn append_er_stage_overlays(
    mut stage: gpui::Stateful<gpui::Div>,
    ws: &mut TableMinusWorkspace,
    graph_elements: (&[ConnectorSpec], &[ErTableNode], &[ErRelationEdge]),
    focus_table: Option<&str>,
    viewport: (f32, f32, f32),
    cx: &mut Context<TableMinusWorkspace>,
) -> gpui::Stateful<gpui::Div> {
    let (connector_specs, vis_nodes, vis_edges) = graph_elements;
    let (zoom, pan_x, pan_y) = viewport;
    let colors = cx.colors().clone();
    for spec in connector_specs {
        let is_highlighted = focus_table.is_some_and(|t| t == spec.from_key || t == spec.to_key);
        let badge_color = if is_highlighted {
            colors.accent.color
        } else {
            colors.muted
        };
        stage = stage
            .child(render_cardinality_badge(
                spec.start_badge_pos,
                "*",
                badge_color,
                cx,
            ))
            .child(render_cardinality_badge(
                spec.end_badge_pos,
                "1",
                badge_color,
                cx,
            ));
    }

    for node in vis_nodes {
        let is_connected_to_focus = focus_table.is_some_and(|ft| {
            ft == node.key
                || vis_edges.iter().any(|e| {
                    (e.from_key == ft && e.to_key == node.key)
                        || (e.to_key == ft && e.from_key == node.key)
                })
        });
        stage = stage.child(render_er_table_card(
            ws,
            node,
            zoom,
            pan_x,
            pan_y,
            is_connected_to_focus,
            focus_table == Some(node.key.as_str()),
            cx,
        ));
    }

    stage
}

fn handle_canvas_mouse_move(
    ws: &mut TableMinusWorkspace,
    ev: &gpui::MouseMoveEvent,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let mx = f32::from(ev.position.x);
    let my = f32::from(ev.position.y);
    if let Some(drag) = ws.er_diagram_ui.dragging_node.clone() {
        let zoom = ws.er_diagram_ui.zoom.max(0.1);
        let dx = (mx - drag.start_mouse_x) / zoom;
        let dy = (my - drag.start_mouse_y) / zoom;
        if let Some(node) = ws
            .er_diagram_ui
            .nodes
            .iter_mut()
            .find(|n| n.key == drag.key)
        {
            node.x = drag.start_node_x + dx;
            node.y = drag.start_node_y + dy;
        }
        cx.notify();
    } else if let Some(pan) = ws.er_diagram_ui.panning_canvas {
        ws.er_diagram_ui.pan_x = pan.start_pan_x + (mx - pan.start_mouse_x);
        ws.er_diagram_ui.pan_y = pan.start_pan_y + (my - pan.start_mouse_y);
        cx.notify();
    }
}

#[derive(Clone)]
struct ConnectorSpec {
    from_key: String,
    to_key: String,
    p0: (f32, f32),
    c1: (f32, f32),
    c2: (f32, f32),
    p3: (f32, f32),
    arrow_a: (f32, f32),
    arrow_b: (f32, f32),
    start_badge_pos: (f32, f32),
    end_badge_pos: (f32, f32),
}

struct CanvasGeometry {
    grid_dots: Vec<Path<Pixels>>,
    dim_strokes: Vec<Path<Pixels>>,
    default_strokes: Vec<Path<Pixels>>,
    active_strokes: Vec<Path<Pixels>>,
    dim_arrows: Vec<Path<Pixels>>,
    default_arrows: Vec<Path<Pixels>>,
    active_arrows: Vec<Path<Pixels>>,
}

fn compute_connector_specs(
    edges: &[ErRelationEdge],
    node_map: &HashMap<String, ErTableNode>,
    zoom: f32,
    pan_x: f32,
    pan_y: f32,
) -> Vec<ConnectorSpec> {
    let mut specs = Vec::with_capacity(edges.len());
    for edge in edges {
        let (Some(from_node), Some(to_node)) =
            (node_map.get(&edge.from_key), node_map.get(&edge.to_key))
        else {
            continue;
        };
        let from_y_local = column_anchor_y(from_node, edge.from_cols.first().map(String::as_str));
        let to_y_local = column_anchor_y(to_node, edge.to_cols.first().map(String::as_str));

        let fx = from_node.x * zoom + pan_x;
        let fy = (from_node.y + from_y_local) * zoom + pan_y;
        let fw = from_node.width * zoom;
        let tx = to_node.x * zoom + pan_x;
        let ty = (to_node.y + to_y_local) * zoom + pan_y;
        let tw = to_node.width * zoom;

        let spec = if edge.from_key == edge.to_key {
            build_self_loop_spec(edge, fx + fw, fy, to_node.y * zoom + pan_y, zoom)
        } else {
            build_bezier_spec(edge, fx, fy, fw, tx, ty, tw, zoom)
        };
        specs.push(spec);
    }
    specs
}

fn column_anchor_y(node: &ErTableNode, col_name: Option<&str>) -> f32 {
    if let Some(name) = col_name
        && let Some(idx) = node.columns.iter().position(|c| c.name == name)
    {
        let clamped_idx = idx.min(MAX_VISIBLE_COLS.saturating_sub(1));
        return CARD_HEADER_HEIGHT_PX + (clamped_idx as f32 + 0.5) * CARD_ROW_HEIGHT_PX;
    }
    CARD_HEADER_HEIGHT_PX + (node.height - CARD_HEADER_HEIGHT_PX) * 0.5
}

#[expect(
    clippy::too_many_arguments,
    reason = "connector geometry helper needs source/target card screen coordinates and zoom"
)]
fn build_bezier_spec(
    edge: &ErRelationEdge,
    fx: f32,
    fy: f32,
    fw: f32,
    tx: f32,
    ty: f32,
    tw: f32,
    zoom: f32,
) -> ConnectorSpec {
    let from_cx = fx + fw * 0.5;
    let to_cx = tx + tw * 0.5;
    let (start_x, end_x, out_dir, in_dir) = if to_cx > from_cx + 36.0 * zoom {
        (fx + fw, tx, 1.0_f32, -1.0_f32)
    } else if to_cx + 36.0 * zoom < from_cx {
        (fx, tx + tw, -1.0_f32, 1.0_f32)
    } else {
        (fx + fw, tx + tw, 1.0_f32, 1.0_f32)
    };

    let ctrl_offset = ((end_x - start_x).abs() * 0.45).max(52.0 * zoom);
    let c1 = (start_x + out_dir * ctrl_offset, fy);
    let c2 = (end_x + in_dir * ctrl_offset, ty);

    let arrow_len = 7.0 * zoom.clamp(0.7, 1.3);
    let arrow_half = 4.2 * zoom.clamp(0.7, 1.3);
    let tip = (end_x, ty);
    let base_x = end_x + in_dir * arrow_len;
    let arrow_a = (base_x, ty - arrow_half);
    let arrow_b = (base_x, ty + arrow_half);

    let start_badge_pos = (start_x + out_dir * (10.0 * zoom), fy - 14.0 * zoom);
    let end_badge_pos = (end_x + in_dir * (15.0 * zoom), ty - 14.0 * zoom);

    ConnectorSpec {
        from_key: edge.from_key.clone(),
        to_key: edge.to_key.clone(),
        p0: (start_x, fy),
        c1,
        c2,
        p3: tip,
        arrow_a,
        arrow_b,
        start_badge_pos,
        end_badge_pos,
    }
}

fn build_self_loop_spec(
    edge: &ErRelationEdge,
    right_x: f32,
    fy: f32,
    top_y: f32,
    zoom: f32,
) -> ConnectorSpec {
    let loop_w = 48.0 * zoom;
    let end_y = (top_y + 18.0 * zoom).min(fy - 10.0 * zoom);
    let arrow_len = 7.0 * zoom.clamp(0.7, 1.3);
    let arrow_half = 4.0 * zoom.clamp(0.7, 1.3);
    ConnectorSpec {
        from_key: edge.from_key.clone(),
        to_key: edge.to_key.clone(),
        p0: (right_x, fy),
        c1: (right_x + loop_w, fy),
        c2: (right_x + loop_w, end_y),
        p3: (right_x, end_y),
        arrow_a: (right_x + arrow_len, end_y - arrow_half),
        arrow_b: (right_x + arrow_len, end_y + arrow_half),
        start_badge_pos: (right_x + 8.0 * zoom, fy - 14.0 * zoom),
        end_badge_pos: (right_x + 12.0 * zoom, end_y - 14.0 * zoom),
    }
}

fn build_canvas_geometry(
    bounds: Bounds<Pixels>,
    zoom: f32,
    pan_x: f32,
    pan_y: f32,
    specs: &[ConnectorSpec],
    focus_table: Option<&str>,
) -> CanvasGeometry {
    let ox = f32::from(bounds.origin.x);
    let oy = f32::from(bounds.origin.y);
    let bw = f32::from(bounds.size.width);
    let bh = f32::from(bounds.size.height);

    let grid_dots = build_dot_grid_paths(ox, oy, bw, bh, zoom, pan_x, pan_y);
    let mut dim_strokes = Vec::new();
    let mut default_strokes = Vec::new();
    let mut active_strokes = Vec::new();
    let mut dim_arrows = Vec::new();
    let mut default_arrows = Vec::new();
    let mut active_arrows = Vec::new();

    for spec in specs {
        let is_active = focus_table.is_some_and(|ft| ft == spec.from_key || ft == spec.to_key);
        let stroke_w = if is_active { 2.2 } else { 1.35 };

        let mut sb = PathBuilder::stroke(px(stroke_w));
        sb.move_to(point(px(ox + spec.p0.0), px(oy + spec.p0.1)));
        sb.cubic_bezier_to(
            point(px(ox + spec.p3.0), px(oy + spec.p3.1)),
            point(px(ox + spec.c1.0), px(oy + spec.c1.1)),
            point(px(ox + spec.c2.0), px(oy + spec.c2.1)),
        );
        let mut ab = PathBuilder::fill();
        ab.move_to(point(px(ox + spec.p3.0), px(oy + spec.p3.1)));
        ab.line_to(point(px(ox + spec.arrow_a.0), px(oy + spec.arrow_a.1)));
        ab.line_to(point(px(ox + spec.arrow_b.0), px(oy + spec.arrow_b.1)));
        ab.close();

        if let (Ok(sp), Ok(ap)) = (sb.build(), ab.build()) {
            if is_active {
                active_strokes.push(sp);
                active_arrows.push(ap);
            } else if focus_table.is_some() {
                dim_strokes.push(sp);
                dim_arrows.push(ap);
            } else {
                default_strokes.push(sp);
                default_arrows.push(ap);
            }
        }
    }

    CanvasGeometry {
        grid_dots,
        dim_strokes,
        default_strokes,
        active_strokes,
        dim_arrows,
        default_arrows,
        active_arrows,
    }
}

fn build_dot_grid_paths(
    ox: f32,
    oy: f32,
    bw: f32,
    bh: f32,
    zoom: f32,
    pan_x: f32,
    pan_y: f32,
) -> Vec<Path<Pixels>> {
    let step = (GRID_STEP_PX * zoom).max(12.0);
    let start_x = pan_x.rem_euclid(step);
    let start_y = pan_y.rem_euclid(step);
    let mut builder = PathBuilder::fill();
    let r = 1.0_f32;
    let mut y = start_y;
    let mut count = 0_usize;
    while y < bh && count < 3600 {
        let mut x = start_x;
        while x < bw && count < 3600 {
            let cx = ox + x;
            let cy = oy + y;
            builder.move_to(point(px(cx - r), px(cy - r)));
            builder.line_to(point(px(cx + r), px(cy - r)));
            builder.line_to(point(px(cx + r), px(cy + r)));
            builder.line_to(point(px(cx - r), px(cy + r)));
            builder.close();
            x += step;
            count += 1;
        }
        y += step;
    }
    builder.build().into_iter().collect()
}

fn paint_canvas_geometry(
    geom: &CanvasGeometry,
    dot_color: Hsla,
    default_color: Hsla,
    dim_color: Hsla,
    active_color: Hsla,
    window: &mut Window,
) {
    for path in &geom.grid_dots {
        window.paint_path(path.clone(), dot_color);
    }
    for path in &geom.dim_strokes {
        window.paint_path(path.clone(), dim_color);
    }
    for path in &geom.dim_arrows {
        window.paint_path(path.clone(), dim_color);
    }
    for path in &geom.default_strokes {
        window.paint_path(path.clone(), default_color);
    }
    for path in &geom.default_arrows {
        window.paint_path(path.clone(), default_color);
    }
    for path in &geom.active_strokes {
        window.paint_path(path.clone(), active_color);
    }
    for path in &geom.active_arrows {
        window.paint_path(path.clone(), active_color);
    }
}

fn render_cardinality_badge(
    pos: (f32, f32),
    text: &'static str,
    color: Hsla,
    cx: &App,
) -> AnyElement {
    div()
        .absolute()
        .left(px(pos.0 - 7.0))
        .top(px(pos.1))
        .px(px(4.0))
        .py(px(1.0))
        .rounded(px(4.0))
        .bg(theme::elevated_overlay_bg(cx))
        .border_1()
        .border_color(theme::hairline_border_color(cx))
        .text_size(px(10.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .child(text)
        .into_any_element()
}

#[expect(
    clippy::too_many_arguments,
    reason = "ER card renderer consumes positioned node, zoom/pan transform, and focus flags"
)]
fn render_er_table_card(
    ws: &TableMinusWorkspace,
    node: &ErTableNode,
    zoom: f32,
    pan_x: f32,
    pan_y: f32,
    is_highlighted: bool,
    is_primary_focus: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let sx = node.x * zoom + pan_x;
    let sy = node.y * zoom + pan_y;
    let card_w = node.width * zoom;
    let card_h = node.height * zoom;
    let header_h = CARD_HEADER_HEIGHT_PX * zoom;
    let row_h = CARD_ROW_HEIGHT_PX * zoom;
    let title_sz = (12.5 * zoom).clamp(8.5, 18.0);
    let col_sz = (11.0 * zoom).clamp(7.5, 16.0);

    let border_col = if is_primary_focus {
        colors.accent.color
    } else if is_highlighted {
        with_alpha(colors.accent.color, 0.55)
    } else {
        theme::hairline_border_color(cx)
    };
    let header_bg = if is_primary_focus {
        with_alpha(colors.accent.color, 0.16)
    } else {
        with_alpha(colors.accent.color, 0.08)
    };

    let key_down = node.key.clone();
    let key_hover = node.key.clone();
    let node_x = node.x;
    let node_y = node.y;
    let card_id = SharedString::from(format!("er-card-{}", node.key));

    let mut card = div()
        .id(card_id)
        .absolute()
        .left(px(sx))
        .top(px(sy))
        .w(px(card_w))
        .h(px(card_h))
        .rounded(px((10.0 * zoom).clamp(5.0, 14.0)))
        .border_1()
        .border_color(border_col)
        .bg(theme::elevated_overlay_bg(cx))
        .overflow_hidden()
        .flex()
        .flex_col()
        .cursor_grab()
        .on_hover(cx.listener(move |ws, hovered: &bool, _w, cx| {
            if *hovered {
                ws.er_diagram_ui.hovered_table = Some(key_hover.clone());
            } else if ws.er_diagram_ui.hovered_table.as_deref() == Some(key_hover.as_str()) {
                ws.er_diagram_ui.hovered_table = None;
            }
            cx.notify();
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, ev: &gpui::MouseDownEvent, _w, cx| {
                ws.er_diagram_ui.selected_table = Some(key_down.clone());
                ws.er_diagram_ui.dragging_node = Some(ErNodeDrag {
                    key: key_down.clone(),
                    start_mouse_x: f32::from(ev.position.x),
                    start_mouse_y: f32::from(ev.position.y),
                    start_node_x: node_x,
                    start_node_y: node_y,
                });
                cx.stop_propagation();
                cx.notify();
            }),
        )
        .child(render_er_card_header(
            node,
            header_h,
            title_sz,
            header_bg,
            s.er_ref_badge,
            &colors,
        ));

    card = card.child(render_er_card_rows(
        node,
        row_h,
        col_sz,
        zoom,
        s.er_fk_badge,
        s.er_more_suffix,
        &colors,
    ));
    card.into_any_element()
}

fn render_er_card_rows(
    node: &ErTableNode,
    row_h: f32,
    col_sz: f32,
    zoom: f32,
    fk_badge: &'static str,
    more_suffix: &'static str,
    colors: &herogpui::theme::ThemeColors,
) -> AnyElement {
    let mut rows_col = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .py(px(3.0 * zoom));
    for col in node.columns.iter().take(MAX_VISIBLE_COLS) {
        rows_col = rows_col.child(render_er_column_row(
            col, row_h, col_sz, zoom, fk_badge, colors,
        ));
    }
    if node.columns.len() > MAX_VISIBLE_COLS {
        let extra = node.columns.len() - MAX_VISIBLE_COLS;
        rows_col = rows_col.child(
            div()
                .h(px(row_h))
                .px(px(10.0 * zoom))
                .flex()
                .items_center()
                .text_size(px((10.5 * zoom).clamp(7.0, 14.0)))
                .text_color(colors.muted)
                .child(format!("+{extra} {more_suffix}")),
        );
    }
    rows_col.into_any_element()
}

fn render_er_card_header(
    node: &ErTableNode,
    header_h: f32,
    title_sz: f32,
    header_bg: Hsla,
    ref_badge: &'static str,
    colors: &herogpui::theme::ThemeColors,
) -> AnyElement {
    div()
        .h(px(header_h))
        .px(px(10.0))
        .bg(header_bg)
        .border_b_1()
        .border_color(colors.border)
        .flex()
        .items_center()
        .justify_between()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .min_w_0()
                .child(
                    Icon::new(IconName::Table)
                        .size(IconSize::Xs)
                        .color(colors.accent.color),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(title_sz))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(node.name.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .flex_shrink_0()
                .when(node.has_inbound_fk, |el| {
                    el.child(
                        div()
                            .px(px(5.0))
                            .py(px(1.0))
                            .rounded(px(4.0))
                            .bg(with_alpha(colors.success.color, 0.16))
                            .text_size(px((title_sz - 2.5).max(7.5)))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.success.color)
                            .child(ref_badge),
                    )
                })
                .child(
                    div()
                        .px(px(6.0))
                        .py(px(1.0))
                        .rounded(px(999.0))
                        .bg(with_alpha(colors.foreground, 0.08))
                        .text_size(px((title_sz - 2.0).max(7.5)))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.muted)
                        .child(node.columns.len().to_string()),
                ),
        )
        .into_any_element()
}

fn render_er_column_row(
    col: &ErColumnItem,
    row_h: f32,
    col_sz: f32,
    zoom: f32,
    fk_badge: &'static str,
    colors: &herogpui::theme::ThemeColors,
) -> AnyElement {
    let pk_color = colors.warning.color;
    let fk_color = colors.accent.color;

    div()
        .h(px(row_h))
        .px(px(10.0 * zoom))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(5.0 * zoom))
                .min_w_0()
                .when(col.is_pk, |el| {
                    el.child(Icon::new(IconName::Key).size(IconSize::Xs).color(pk_color))
                })
                .when(col.is_fk, |el| {
                    el.child(
                        div()
                            .px(px(4.0))
                            .rounded(px(3.0))
                            .bg(with_alpha(fk_color, 0.16))
                            .text_size(px((col_sz - 2.0).max(7.0)))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(fk_color)
                            .child(fk_badge),
                    )
                })
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(col_sz))
                        .font_weight(if col.is_pk {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(colors.foreground)
                        .child(col.name.clone()),
                ),
        )
        .when(zoom >= 0.55, |el| {
            let type_text = if col.nullable {
                format!("{}?", col.data_type)
            } else {
                col.data_type.clone()
            };
            el.child(
                div()
                    .flex_shrink_0()
                    .max_w(px(86.0 * zoom))
                    .truncate()
                    .text_size(px((col_sz - 1.0).max(7.0)))
                    .text_color(colors.muted)
                    .child(type_text),
            )
        })
        .into_any_element()
}

fn export_er_svg(ws: &mut TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) {
    let vis_nodes: Vec<ErTableNode> = visible_nodes(ws).into_iter().cloned().collect();
    let keys = visible_node_keys(ws);
    let vis_edges: Vec<ErRelationEdge> = visible_edges(ws, &keys).into_iter().cloned().collect();
    if vis_nodes.is_empty() {
        return;
    }

    let svg = build_er_svg_xml(&vis_nodes, &vis_edges);
    cx.write_to_clipboard(ClipboardItem::new_string(svg.clone()));
    let export_label = ws.controller.strings.er_export_svg;
    ws.notify_toast(
        ToastSeverity::Success,
        format!("{export_label}: {}", ws.controller.strings.copied),
        None,
        cx,
    );

    let start_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let rx = cx.prompt_for_new_path(&start_dir, Some("er_diagram.svg"));
    ws.er_diagram_ui.export_task = Some(cx.spawn(
        async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(path))) = rx.await else {
                return;
            };
            if let Err(err) = std::fs::write(&path, svg.as_bytes()) {
                let _ = this.update(cx, |ws, cx| {
                    ws.notify_toast(ToastSeverity::Error, err.to_string(), None, cx);
                });
                return;
            }
            let _ = this.update(cx, |ws, cx| {
                ws.notify_toast(
                    ToastSeverity::Success,
                    ws.controller.strings.export_complete_toast,
                    Some(SharedString::from(path.display().to_string())),
                    cx,
                );
            });
        },
    ));
}

fn build_er_svg_xml(nodes: &[ErTableNode], edges: &[ErRelationEdge]) -> String {
    let min_x = nodes.iter().map(|n| n.x).fold(f32::INFINITY, f32::min) - 40.0;
    let min_y = nodes.iter().map(|n| n.y).fold(f32::INFINITY, f32::min) - 40.0;
    let max_x = nodes
        .iter()
        .map(|n| n.x + n.width)
        .fold(f32::NEG_INFINITY, f32::max)
        + 40.0;
    let max_y = nodes
        .iter()
        .map(|n| n.y + n.height)
        .fold(f32::NEG_INFINITY, f32::max)
        + 40.0;
    let width = (max_x - min_x).max(400.0);
    let height = (max_y - min_y).max(300.0);

    let node_map: HashMap<String, ErTableNode> =
        nodes.iter().map(|n| (n.key.clone(), n.clone())).collect();
    let specs = compute_connector_specs(edges, &node_map, 1.0, -min_x, -min_y);

    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width:.0}\" height=\"{height:.0}\" viewBox=\"0 0 {width:.0} {height:.0}\">\n\
         <defs><marker id=\"arrow\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"7\" markerHeight=\"7\" orient=\"auto-start-reverse\">\
         <path d=\"M 0 1 L 10 5 L 0 9 z\" fill=\"#64748b\"/></marker></defs>\n\
         <rect width=\"100%\" height=\"100%\" fill=\"#f8fafc\"/>\n"
    );

    for spec in &specs {
        out.push_str(&format!(
            "<path d=\"M {:.1} {:.1} C {:.1} {:.1}, {:.1} {:.1}, {:.1} {:.1}\" fill=\"none\" stroke=\"#64748b\" stroke-width=\"1.5\" marker-end=\"url(#arrow)\"/>\n",
            spec.p0.0, spec.p0.1, spec.c1.0, spec.c1.1, spec.c2.0, spec.c2.1, spec.p3.0, spec.p3.1
        ));
        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"sans-serif\" font-size=\"10\" fill=\"#475569\">*</text>\n\
             <text x=\"{:.1}\" y=\"{:.1}\" font-family=\"sans-serif\" font-size=\"10\" fill=\"#475569\">1</text>\n",
            spec.start_badge_pos.0, spec.start_badge_pos.1 + 10.0,
            spec.end_badge_pos.0, spec.end_badge_pos.1 + 10.0
        ));
    }

    for node in nodes {
        append_svg_table_card(&mut out, node, node.x - min_x, node.y - min_y);
    }
    out.push_str("</svg>\n");
    out
}

fn append_svg_table_card(out: &mut String, node: &ErTableNode, x: f32, y: f32) {
    let w = node.width;
    let h = node.height;
    let name_esc = escape_xml(&node.name);
    out.push_str(&format!(
        "<g transform=\"translate({x:.1},{y:.1})\">\n\
         <rect width=\"{w:.1}\" height=\"{h:.1}\" rx=\"10\" fill=\"#ffffff\" stroke=\"#cbd5e1\" stroke-width=\"1\"/>\n\
         <rect width=\"{w:.1}\" height=\"{CARD_HEADER_HEIGHT_PX:.1}\" rx=\"10\" fill=\"#eff6ff\"/>\n\
         <text x=\"12\" y=\"23\" font-family=\"sans-serif\" font-size=\"12.5\" font-weight=\"600\" fill=\"#0f172a\">{name_esc}</text>\n"
    ));
    for (idx, col) in node.columns.iter().take(MAX_VISIBLE_COLS).enumerate() {
        let ry = CARD_HEADER_HEIGHT_PX + (idx as f32 + 0.72) * CARD_ROW_HEIGHT_PX;
        let prefix = if col.is_pk {
            "PK "
        } else if col.is_fk {
            "FK "
        } else {
            ""
        };
        let col_esc = escape_xml(&format!("{prefix}{}", col.name));
        let typ_esc = escape_xml(&col.data_type);
        out.push_str(&format!(
            "<text x=\"12\" y=\"{ry:.1}\" font-family=\"sans-serif\" font-size=\"11\" fill=\"#1e293b\">{col_esc}</text>\n\
             <text x=\"{:.1}\" y=\"{ry:.1}\" text-anchor=\"end\" font-family=\"monospace\" font-size=\"10.5\" fill=\"#64748b\">{typ_esc}</text>\n",
            w - 12.0
        ));
    }
    out.push_str("</g>\n");
}

fn escape_xml(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tablepro_core::column::{ColumnInfo, ColumnKind, ColumnType, ReadForm};
    use tablepro_core::ddl::{ColumnAttributes, ColumnDefinition, PrimaryKeyConstraint};
    use tablepro_core::foreign_key_info::ForeignKeyInfo;
    use tablepro_core::meta::TableRef;

    fn make_col(name: &str, data_type: &str, pk: bool) -> ColumnDefinition {
        ColumnDefinition::new(
            ColumnInfo {
                name: name.to_owned(),
                column_type: ColumnType {
                    name: data_type.to_owned(),
                    kind: ColumnKind::Text {
                        length: tablepro_core::column::TextLength::Unbounded,
                    },
                    catalog: None,
                    dynamic_storage: false,
                    read_form: ReadForm::Native,
                },
                nullable: !pk,
                primary_key: pk,
                is_auto_increment: false,
                is_generated: false,
                partition_scoped: false,
                default: tablepro_core::column::ColumnDefault::None,
                comment: None,
            },
            ColumnAttributes::default(),
        )
    }

    #[test]
    fn er_graph_layout_has_no_overlaps_and_exports_valid_svg() {
        let users = TableDefinition {
            table: TableRef::new(Some("public".to_owned()), "users"),
            columns: vec![
                make_col("id", "bigint", true),
                make_col("email", "text", false),
            ],
            primary_key: Some(PrimaryKeyConstraint {
                name: None,
                columns: vec!["id".to_owned()],
            }),
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            inbound_foreign_keys: Vec::new(),
            source: None,
            options: None,
        };
        let orders = TableDefinition {
            table: TableRef::new(Some("public".to_owned()), "orders"),
            columns: vec![
                make_col("id", "bigint", true),
                make_col("user_id", "bigint", false),
            ],
            primary_key: None,
            indexes: Vec::new(),
            foreign_keys: vec![ForeignKeyInfo {
                name: "orders_user_id_fkey".to_owned(),
                columns: vec!["user_id".to_owned()],
                ref_schema: Some("public".to_owned()),
                ref_table: "users".to_owned(),
                ref_columns: vec!["id".to_owned()],
                on_delete: None,
                on_update: None,
            }],
            inbound_foreign_keys: Vec::new(),
            source: None,
            options: None,
        };
        let settings = TableDefinition {
            table: TableRef::new(Some("audit".to_owned()), "settings&logs"),
            columns: vec![make_col("k", "text", true)],
            primary_key: None,
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            inbound_foreign_keys: Vec::new(),
            source: None,
            options: None,
        };

        let (mut nodes, edges, schemas) = build_er_graph(&[users, orders, settings]);
        assert_eq!(nodes.len(), 3);
        assert_eq!(edges.len(), 1);
        assert_eq!(schemas, vec!["audit".to_owned(), "public".to_owned()]);
        assert!(nodes[0].has_inbound_fk);
        assert!(nodes[1].columns[1].is_fk);

        apply_auto_layout(&mut nodes, &edges);
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                let a = &nodes[i];
                let b = &nodes[j];
                let overlap_x = a.x < b.x + b.width && a.x + a.width > b.x;
                let overlap_y = a.y < b.y + b.height && a.y + a.height > b.y;
                assert!(
                    !(overlap_x && overlap_y),
                    "cards overlapped: {} & {}",
                    a.key,
                    b.key
                );
            }
        }

        let svg = build_er_svg_xml(&nodes, &edges);
        assert!(svg.starts_with("<svg "));
        assert!(svg.contains("settings&amp;logs"));
        assert!(svg.contains("<path "));
    }
}
