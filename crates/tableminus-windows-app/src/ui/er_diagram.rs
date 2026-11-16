//! Interactive Entity-Relationship (ER) diagram canvas for tables and foreign keys.

use std::cell::Cell;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;

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
use tableminus_state::connection_window::ConnectionWindowPhase;
use tablepro_core::ddl::TableDefinition;

use super::shell_state::ToastSeverity;
use super::{TableMinusWorkspace, theme};

const CARD_WIDTH_PX: f32 = 264.0;
const CARD_HEADER_HEIGHT_PX: f32 = 38.0;
const CARD_ROW_HEIGHT_PX: f32 = 24.0;
const CARD_PADDING_BOTTOM_PX: f32 = 8.0;
const MAX_VISIBLE_COLS: usize = 12;
const MIN_ZOOM: f32 = 0.15;
const MAX_ZOOM: f32 = 2.5;
const ZOOM_STEP: f32 = 0.15;
const GRID_STEP_PX: f32 = 24.0;
const MAX_NODES_PER_SUBCOL: usize = 6;
const ORTHO_CORNER_RADIUS_PX: f32 = 8.0;
const LANE_STEP_PX: f32 = 10.0;
const MINIMAP_W_PX: f32 = 188.0;
const MINIMAP_H_PX: f32 = 128.0;
const MINIMAP_MARGIN_PX: f32 = 14.0;
const MINIMAP_PAD_PX: f32 = 10.0;

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
    pub hovered_col: Option<(String, String)>,
    pub hovered_fk: Option<(String, String)>,
    pub hovered_edge_key: Option<(String, String, String)>,
    pub dragging_node: Option<ErNodeDrag>,
    pub panning_canvas: Option<ErCanvasPan>,
    pub panning_minimap: bool,
    pub viewport_size: Rc<Cell<(f32, f32)>>,
    pub stage_origin: Rc<Cell<(f32, f32)>>,
    pub minimap_bounds: Rc<Cell<(f32, f32, f32, f32)>>,
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

/// Layer- and cluster-aware layout with sub-column wrapping, 2D shelf packing,
/// and iterative collision avoidance for large schemas (50-100+ tables).
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
            if !adj[u].contains(&v) {
                adj[u].push(v);
            }
            if !adj[v].contains(&u) {
                adj[v].push(u);
            }
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

    connected_comps.sort_by(|a, b| {
        b.len()
            .cmp(&a.len())
            .then_with(|| nodes[a[0]].name.cmp(&nodes[b[0]].name))
    });

    let h_gap = 104.0_f32;
    let v_gap = 52.0_f32;
    let comp_gap_x = 120.0_f32;
    let comp_gap_y = 96.0_f32;

    // Layout each connected component in local coordinates first to measure bounding boxes.
    let mut comp_boxes: Vec<(Vec<usize>, f32, f32)> = Vec::with_capacity(connected_comps.len());
    let mut total_area = 0.0_f32;
    for comp in connected_comps {
        let (cw, ch) = layout_component_local(nodes, &adj, &in_deg, &comp, h_gap, v_gap);
        total_area += (cw + comp_gap_x) * (ch + comp_gap_y);
        comp_boxes.push((comp, cw, ch));
    }

    // 2D shelf-pack components so 50-100 tables form a balanced aspect ratio.
    let target_row_w = (total_area.sqrt() * 1.45).clamp(1600.0, 3600.0);
    let mut cursor_x = 36.0_f32;
    let mut cursor_y = 32.0_f32;
    let mut shelf_max_h = 0.0_f32;

    for (comp, cw, ch) in comp_boxes {
        if cursor_x > 36.0 && cursor_x + cw > target_row_w {
            cursor_x = 36.0;
            cursor_y += shelf_max_h + comp_gap_y;
            shelf_max_h = 0.0;
        }
        for idx in comp {
            nodes[idx].x += cursor_x;
            nodes[idx].y += cursor_y;
        }
        cursor_x += cw + comp_gap_x;
        shelf_max_h = shelf_max_h.max(ch);
    }

    let isolated_start_y = if shelf_max_h > 0.0 {
        cursor_y + shelf_max_h + comp_gap_y
    } else {
        32.0
    };
    if !isolated.is_empty() {
        layout_isolated_grid(nodes, &isolated, isolated_start_y, h_gap, v_gap);
    }

    resolve_node_overlaps(nodes, 48.0, 32.0);
    normalize_node_origin(nodes, 36.0, 32.0);
}

fn layout_component_local(
    nodes: &mut [ErTableNode],
    adj: &[Vec<usize>],
    in_deg: &[usize],
    comp: &[usize],
    h_gap: f32,
    v_gap: f32,
) -> (f32, f32) {
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

    // Barycenter sweeps to reduce edge crossings between adjacent layers.
    for _ in 0..2 {
        for lvl in 1..layers.len() {
            let prev_pos: HashMap<usize, usize> = layers[lvl - 1]
                .iter()
                .enumerate()
                .map(|(p, &idx)| (idx, p))
                .collect();
            layers[lvl].sort_by(|&a, &b| {
                let ba = barycenter_score(a, adj, &prev_pos);
                let bb = barycenter_score(b, adj, &prev_pos);
                ba.partial_cmp(&bb)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| nodes[a].name.cmp(&nodes[b].name))
            });
        }
    }

    let mut col_idx = 0_usize;
    let mut max_w = 0.0_f32;
    let mut max_h = 0.0_f32;
    for layer in &layers {
        for subcol in layer.chunks(MAX_NODES_PER_SUBCOL) {
            let x = (col_idx as f32) * (CARD_WIDTH_PX + h_gap);
            let mut y = 0.0_f32;
            for &node_idx in subcol {
                nodes[node_idx].x = x;
                nodes[node_idx].y = y;
                max_w = max_w.max(x + nodes[node_idx].width);
                max_h = max_h.max(y + nodes[node_idx].height);
                y += nodes[node_idx].height + v_gap;
            }
            col_idx += 1;
        }
    }
    (max_w, max_h)
}

fn barycenter_score(node_idx: usize, adj: &[Vec<usize>], prev_pos: &HashMap<usize, usize>) -> f32 {
    let mut sum = 0.0_f32;
    let mut count = 0_usize;
    for &nbr in &adj[node_idx] {
        if let Some(&pos) = prev_pos.get(&nbr) {
            sum += pos as f32;
            count += 1;
        }
    }
    if count == 0 {
        f32::MAX * 0.5
    } else {
        sum / (count as f32)
    }
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
    let cols = (sorted.len() as f32).sqrt().ceil().clamp(3.0, 8.0) as usize;
    let mut y = start_y;
    for chunk in sorted.chunks(cols) {
        let mut row_max_h = 0.0_f32;
        for (c, &idx) in chunk.iter().enumerate() {
            nodes[idx].x = 36.0 + (c as f32) * (CARD_WIDTH_PX + h_gap);
            nodes[idx].y = y;
            row_max_h = row_max_h.max(nodes[idx].height);
        }
        y += row_max_h + v_gap;
    }
}

/// Iteratively separates any overlapping table cards while preserving minimum spacing.
fn resolve_node_overlaps(nodes: &mut [ErTableNode], min_gap_x: f32, min_gap_y: f32) {
    let n = nodes.len();
    for _pass in 0..32 {
        let mut moved = false;
        for i in 0..n {
            for j in (i + 1)..n {
                let (ax, ay, aw, ah) = (nodes[i].x, nodes[i].y, nodes[i].width, nodes[i].height);
                let (bx, by, bw, bh) = (nodes[j].x, nodes[j].y, nodes[j].width, nodes[j].height);

                let overlap_x = (ax + aw + min_gap_x).min(bx + bw + min_gap_x) - ax.max(bx);
                let overlap_y = (ay + ah + min_gap_y).min(by + bh + min_gap_y) - ay.max(by);
                if overlap_x > 0.0 && overlap_y > 0.0 {
                    moved = true;
                    if overlap_y <= overlap_x {
                        if ay <= by {
                            nodes[j].y += overlap_y;
                        } else {
                            nodes[i].y += overlap_y;
                        }
                    } else if ax <= bx {
                        nodes[j].x += overlap_x;
                    } else {
                        nodes[i].x += overlap_x;
                    }
                }
            }
        }
        if !moved {
            break;
        }
    }
}

fn normalize_node_origin(nodes: &mut [ErTableNode], pad_x: f32, pad_y: f32) {
    let Some(min_x) = nodes.iter().map(|n| n.x).reduce(f32::min) else {
        return;
    };
    let Some(min_y) = nodes.iter().map(|n| n.y).reduce(f32::min) else {
        return;
    };
    let dx = pad_x - min_x;
    let dy = pad_y - min_y;
    if dx.abs() > 0.5 || dy.abs() > 0.5 {
        for node in nodes {
            node.x += dx;
            node.y += dy;
        }
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
        .child(
            Button::new("er-zoom-fit-btn")
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .is_icon_only(true)
                .aria_label("Zoom to Fit")
                .on_press(cx.listener(|ws, _, _, cx| {
                    zoom_to_fit(ws);
                    cx.notify();
                }))
                .child(Icon::new(IconName::Expand).size(IconSize::Xs)),
        )
        .into_any_element()
}

fn compute_nodes_bounds(nodes: &[&ErTableNode]) -> Option<(f32, f32, f32, f32)> {
    if nodes.is_empty() {
        return None;
    }
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for n in nodes {
        min_x = min_x.min(n.x);
        min_y = min_y.min(n.y);
        max_x = max_x.max(n.x + n.width);
        max_y = max_y.max(n.y + n.height);
    }
    Some((min_x, min_y, max_x, max_y))
}

fn zoom_to_fit(ws: &mut TableMinusWorkspace) {
    let vis = visible_nodes(ws);
    let Some((min_x, min_y, max_x, max_y)) = compute_nodes_bounds(&vis) else {
        return;
    };
    let (raw_vw, raw_vh) = ws.er_diagram_ui.viewport_size.get();
    let vw = if raw_vw > 40.0 { raw_vw } else { 960.0 };
    let vh = if raw_vh > 40.0 { raw_vh } else { 640.0 };
    let pad = 44.0_f32;
    let content_w = (max_x - min_x).max(1.0);
    let content_h = (max_y - min_y).max(1.0);
    let fit_zoom = ((vw - pad * 2.0) / content_w)
        .min((vh - pad * 2.0) / content_h)
        .clamp(MIN_ZOOM, 1.25);
    ws.er_diagram_ui.zoom = fit_zoom;
    ws.er_diagram_ui.pan_x = (vw - content_w * fit_zoom) * 0.5 - min_x * fit_zoom;
    ws.er_diagram_ui.pan_y = (vh - content_h * fit_zoom) * 0.5 - min_y * fit_zoom;
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

#[derive(Clone, Default)]
struct RelationshipFocus {
    focus_table: Option<String>,
    hovered_fk: Option<(String, String)>,
    hovered_edge_key: Option<(String, String, String)>,
}

impl RelationshipFocus {
    fn from_state(state: &ErDiagramState) -> Self {
        let focus_table = state
            .hovered_table
            .clone()
            .or_else(|| state.selected_table.clone());
        Self {
            focus_table,
            hovered_fk: state.hovered_fk.clone(),
            hovered_edge_key: state.hovered_edge_key.clone(),
        }
    }

    fn has_any_focus(&self) -> bool {
        self.hovered_fk.is_some() || self.hovered_edge_key.is_some() || self.focus_table.is_some()
    }

    fn is_spec_active(&self, spec: &ConnectorSpec) -> bool {
        if let Some((ref fk_table, ref fk_col)) = self.hovered_fk {
            return &spec.from_key == fk_table && spec.from_cols.iter().any(|c| c == fk_col);
        }
        if let Some((ref ek_from, ref ek_name, ref ek_to)) = self.hovered_edge_key {
            return &spec.from_key == ek_from
                && &spec.edge_name == ek_name
                && &spec.to_key == ek_to;
        }
        self.focus_table
            .as_deref()
            .is_some_and(|ft| ft == spec.from_key || ft == spec.to_key)
    }

    fn is_edge_active(&self, edge: &ErRelationEdge) -> bool {
        if let Some((ref fk_table, ref fk_col)) = self.hovered_fk {
            return &edge.from_key == fk_table && edge.from_cols.iter().any(|c| c == fk_col);
        }
        if let Some((ref ek_from, ref ek_name, ref ek_to)) = self.hovered_edge_key {
            return &edge.from_key == ek_from && &edge.name == ek_name && &edge.to_key == ek_to;
        }
        self.focus_table
            .as_deref()
            .is_some_and(|ft| ft == edge.from_key || ft == edge.to_key)
    }

    fn is_table_highlighted(&self, table_key: &str, vis_edges: &[ErRelationEdge]) -> bool {
        if self.hovered_fk.is_some() || self.hovered_edge_key.is_some() {
            return vis_edges.iter().any(|e| {
                self.is_edge_active(e) && (e.from_key == table_key || e.to_key == table_key)
            });
        }
        self.focus_table.as_deref().is_some_and(|ft| {
            ft == table_key
                || vis_edges.iter().any(|e| {
                    (e.from_key == ft && e.to_key == table_key)
                        || (e.to_key == ft && e.from_key == table_key)
                })
        })
    }

    fn is_col_highlighted(
        &self,
        table_key: &str,
        col_name: &str,
        vis_edges: &[ErRelationEdge],
    ) -> bool {
        if self.hovered_fk.is_none() && self.hovered_edge_key.is_none() {
            return false;
        }
        vis_edges.iter().any(|e| {
            if !self.is_edge_active(e) {
                return false;
            }
            (e.from_key == table_key && e.from_cols.iter().any(|c| c == col_name))
                || (e.to_key == table_key && e.to_cols.iter().any(|c| c == col_name))
        })
    }
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
                    || ws.er_diagram_ui.panning_minimap
                {
                    ws.er_diagram_ui.dragging_node = None;
                    ws.er_diagram_ui.panning_canvas = None;
                    ws.er_diagram_ui.panning_minimap = false;
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
    let rel_focus = RelationshipFocus::from_state(&ws.er_diagram_ui);

    let keys = visible_node_keys(ws);
    let vis_nodes: Vec<ErTableNode> = visible_nodes(ws).into_iter().cloned().collect();
    let vis_edges: Vec<ErRelationEdge> = visible_edges(ws, &keys).into_iter().cloned().collect();
    let node_map: HashMap<String, ErTableNode> = vis_nodes
        .iter()
        .map(|n| (n.key.clone(), n.clone()))
        .collect();

    let colors = cx.colors().clone();
    let dot_color = with_alpha(colors.muted, 0.22);
    let edge_default_color = with_alpha(colors.muted, 0.58);
    let edge_dim_color = with_alpha(colors.muted, 0.18);
    let edge_active_color = colors.accent.color;

    let connector_specs = compute_connector_specs(&vis_edges, &node_map, zoom, pan_x, pan_y);
    let specs_for_canvas = connector_specs.clone();
    let focus_for_canvas = rel_focus.clone();
    let viewport_cell = ws.er_diagram_ui.viewport_size.clone();
    let origin_cell = ws.er_diagram_ui.stage_origin.clone();

    let mut stage = build_er_stage_container(cx).child(
        canvas(
            move |bounds, _window, _cx| {
                viewport_cell.set((f32::from(bounds.size.width), f32::from(bounds.size.height)));
                origin_cell.set((f32::from(bounds.origin.x), f32::from(bounds.origin.y)));
                build_canvas_geometry(
                    bounds,
                    zoom,
                    pan_x,
                    pan_y,
                    &specs_for_canvas,
                    &focus_for_canvas,
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
        &rel_focus,
        (zoom, pan_x, pan_y),
        cx,
    );

    stage = stage.child(render_er_minimap(ws, &vis_nodes, &rel_focus, cx));

    stage.into_any_element()
}

fn append_er_stage_overlays(
    mut stage: gpui::Stateful<gpui::Div>,
    ws: &mut TableMinusWorkspace,
    graph_elements: (&[ConnectorSpec], &[ErTableNode], &[ErRelationEdge]),
    rel_focus: &RelationshipFocus,
    viewport: (f32, f32, f32),
    cx: &mut Context<TableMinusWorkspace>,
) -> gpui::Stateful<gpui::Div> {
    let (connector_specs, vis_nodes, vis_edges) = graph_elements;
    let (zoom, pan_x, pan_y) = viewport;
    let colors = cx.colors().clone();
    let has_focus = rel_focus.has_any_focus();
    for spec in connector_specs {
        let is_highlighted = rel_focus.is_spec_active(spec);
        let badge_color = if is_highlighted {
            colors.accent.color
        } else if has_focus {
            with_alpha(colors.muted, 0.45)
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
        let is_connected_to_focus = rel_focus.is_table_highlighted(&node.key, vis_edges);
        let is_primary_focus = rel_focus.focus_table.as_deref() == Some(node.key.as_str());
        stage = stage.child(render_er_table_card(
            ws,
            node,
            vis_edges,
            rel_focus,
            zoom,
            pan_x,
            pan_y,
            is_connected_to_focus,
            is_primary_focus,
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
    if ws.er_diagram_ui.panning_minimap {
        pan_to_minimap_pos(ws, mx, my);
        cx.notify();
        return;
    }
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
    } else if ws.er_diagram_ui.hovered_table.is_none() && ws.er_diagram_ui.hovered_fk.is_none() {
        let (ox, oy) = ws.er_diagram_ui.stage_origin.get();
        let keys = visible_node_keys(ws);
        let vis_nodes: Vec<ErTableNode> = visible_nodes(ws).into_iter().cloned().collect();
        let vis_edges: Vec<ErRelationEdge> =
            visible_edges(ws, &keys).into_iter().cloned().collect();
        let node_map: HashMap<String, ErTableNode> =
            vis_nodes.into_iter().map(|n| (n.key.clone(), n)).collect();
        let specs = compute_connector_specs(
            &vis_edges,
            &node_map,
            ws.er_diagram_ui.zoom,
            ws.er_diagram_ui.pan_x,
            ws.er_diagram_ui.pan_y,
        );
        let hit = hit_test_connector(&specs, mx - ox, my - oy, 7.0);
        if ws.er_diagram_ui.hovered_edge_key != hit {
            ws.er_diagram_ui.hovered_edge_key = hit;
            cx.notify();
        }
    } else if ws.er_diagram_ui.hovered_edge_key.is_some() {
        ws.er_diagram_ui.hovered_edge_key = None;
        cx.notify();
    }
}

fn pan_to_minimap_pos(ws: &mut TableMinusWorkspace, mouse_x: f32, mouse_y: f32) {
    let (mx, my, mw, mh) = ws.er_diagram_ui.minimap_bounds.get();
    if mw <= 1.0 || mh <= 1.0 {
        return;
    }
    let vis = visible_nodes(ws);
    let Some((min_x, min_y, max_x, max_y)) = compute_nodes_bounds(&vis) else {
        return;
    };
    let pad = 60.0_f32;
    let world_min_x = min_x - pad;
    let world_min_y = min_y - pad;
    let world_w = (max_x - min_x + pad * 2.0).max(1.0);
    let world_h = (max_y - min_y + pad * 2.0).max(1.0);

    let inner_w = (mw - MINIMAP_PAD_PX * 2.0).max(1.0);
    let inner_h = (mh - MINIMAP_PAD_PX * 2.0).max(1.0);
    let scale = (inner_w / world_w).min(inner_h / world_h);
    let offset_x = mx + MINIMAP_PAD_PX + (inner_w - world_w * scale) * 0.5;
    let offset_y = my + MINIMAP_PAD_PX + (inner_h - world_h * scale) * 0.5;

    let world_x = world_min_x + ((mouse_x - offset_x) / scale).clamp(0.0, world_w);
    let world_y = world_min_y + ((mouse_y - offset_y) / scale).clamp(0.0, world_h);

    let (raw_vw, raw_vh) = ws.er_diagram_ui.viewport_size.get();
    let vw = if raw_vw > 40.0 { raw_vw } else { 960.0 };
    let vh = if raw_vh > 40.0 { raw_vh } else { 640.0 };
    let zoom = ws.er_diagram_ui.zoom;

    ws.er_diagram_ui.pan_x = vw * 0.5 - world_x * zoom;
    ws.er_diagram_ui.pan_y = vh * 0.5 - world_y * zoom;
}

#[derive(Clone)]
struct ConnectorSpec {
    from_key: String,
    to_key: String,
    edge_name: String,
    from_cols: Vec<String>,
    points: Vec<(f32, f32)>,
    arrow_tip: (f32, f32),
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

#[derive(Clone, Copy)]
struct EdgeEndpoints {
    fx: f32,
    fy: f32,
    fw: f32,
    tx: f32,
    ty: f32,
    tw: f32,
}

fn compute_connector_specs(
    edges: &[ErRelationEdge],
    node_map: &HashMap<String, ErTableNode>,
    zoom: f32,
    pan_x: f32,
    pan_y: f32,
) -> Vec<ConnectorSpec> {
    // Group edges by corridor bucket so parallel lines get staggered lane offsets.
    let mut corridor_edges: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    let mut raw_endpoints: Vec<Option<EdgeEndpoints>> = Vec::with_capacity(edges.len());

    for (idx, edge) in edges.iter().enumerate() {
        let (Some(from_node), Some(to_node)) =
            (node_map.get(&edge.from_key), node_map.get(&edge.to_key))
        else {
            raw_endpoints.push(None);
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

        raw_endpoints.push(Some(EdgeEndpoints {
            fx,
            fy,
            fw,
            tx,
            ty,
            tw,
        }));
        if edge.from_key != edge.to_key {
            let b1 = ((fx + fw * 0.5) / (CARD_WIDTH_PX * zoom.max(0.1))).round() as i32;
            let b2 = ((tx + tw * 0.5) / (CARD_WIDTH_PX * zoom.max(0.1))).round() as i32;
            let key = if b1 <= b2 { (b1, b2) } else { (b2, b1) };
            corridor_edges.entry(key).or_default().push(idx);
        }
    }

    let mut lane_offsets = vec![0.0_f32; edges.len()];
    for indices in corridor_edges.values_mut() {
        indices.sort_by(|&a, &b| {
            let ya = raw_endpoints[a].map_or(0.0, |e| e.fy.min(e.ty));
            let yb = raw_endpoints[b].map_or(0.0, |e| e.fy.min(e.ty));
            ya.partial_cmp(&yb)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| edges[a].name.cmp(&edges[b].name))
        });
        let count = indices.len();
        for (pos, &edge_idx) in indices.iter().enumerate() {
            lane_offsets[edge_idx] =
                ((pos as f32) - (count as f32 - 1.0) * 0.5) * LANE_STEP_PX * zoom;
        }
    }

    let mut specs = Vec::with_capacity(edges.len());
    for (idx, edge) in edges.iter().enumerate() {
        let Some(ep) = raw_endpoints[idx] else {
            continue;
        };
        let to_node = &node_map[&edge.to_key];
        let spec = if edge.from_key == edge.to_key {
            build_self_loop_spec(
                edge,
                ep.fx + ep.fw,
                ep.fy,
                to_node.y * zoom + pan_y,
                zoom,
                idx as f32,
            )
        } else {
            build_orthogonal_spec(
                edge,
                ep.fx,
                ep.fy,
                ep.fw,
                ep.tx,
                ep.ty,
                ep.tw,
                zoom,
                lane_offsets[idx],
            )
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
    reason = "orthogonal connector geometry helper needs source/target card screen coordinates, zoom, and lane offset"
)]
fn build_orthogonal_spec(
    edge: &ErRelationEdge,
    fx: f32,
    fy: f32,
    fw: f32,
    tx: f32,
    ty: f32,
    tw: f32,
    zoom: f32,
    lane_offset: f32,
) -> ConnectorSpec {
    let min_gap = 24.0 * zoom;
    let (start_x, end_x, out_dir, in_dir, same_side) = if tx >= fx + fw + min_gap {
        (fx + fw, tx, 1.0_f32, -1.0_f32, false)
    } else if fx >= tx + tw + min_gap {
        (fx, tx + tw, -1.0_f32, 1.0_f32, false)
    } else {
        (fx + fw, tx + tw, 1.0_f32, 1.0_f32, true)
    };

    let points = if same_side {
        let channel_x = start_x.max(end_x) + 32.0 * zoom + lane_offset.abs();
        if (fy - ty).abs() < 1.0 {
            vec![(start_x, fy), (end_x, ty)]
        } else {
            vec![(start_x, fy), (channel_x, fy), (channel_x, ty), (end_x, ty)]
        }
    } else if (fy - ty).abs() < 1.0 {
        vec![(start_x, fy), (end_x, ty)]
    } else {
        let lo = start_x.min(end_x) + 14.0 * zoom;
        let hi = start_x.max(end_x) - 14.0 * zoom;
        let raw_mid = (start_x + end_x) * 0.5 + lane_offset;
        let mid_x = if lo <= hi {
            raw_mid.clamp(lo, hi)
        } else {
            (start_x + end_x) * 0.5
        };
        vec![(start_x, fy), (mid_x, fy), (mid_x, ty), (end_x, ty)]
    };

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
        edge_name: edge.name.clone(),
        from_cols: edge.from_cols.clone(),
        points,
        arrow_tip: tip,
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
    loop_index: f32,
) -> ConnectorSpec {
    let loop_w = (44.0 + (loop_index % 3.0) * 12.0) * zoom;
    let end_y = (top_y + 18.0 * zoom).min(fy - 10.0 * zoom);
    let arrow_len = 7.0 * zoom.clamp(0.7, 1.3);
    let arrow_half = 4.0 * zoom.clamp(0.7, 1.3);
    let points = vec![
        (right_x, fy),
        (right_x + loop_w, fy),
        (right_x + loop_w, end_y),
        (right_x, end_y),
    ];
    ConnectorSpec {
        from_key: edge.from_key.clone(),
        to_key: edge.to_key.clone(),
        edge_name: edge.name.clone(),
        from_cols: edge.from_cols.clone(),
        points,
        arrow_tip: (right_x, end_y),
        arrow_a: (right_x + arrow_len, end_y - arrow_half),
        arrow_b: (right_x + arrow_len, end_y + arrow_half),
        start_badge_pos: (right_x + 8.0 * zoom, fy - 14.0 * zoom),
        end_badge_pos: (right_x + 12.0 * zoom, end_y - 14.0 * zoom),
    }
}

fn build_orthogonal_rounded_path(
    pts: &[(f32, f32)],
    ox: f32,
    oy: f32,
    corner_radius: f32,
    stroke_w: f32,
) -> Option<Path<Pixels>> {
    if pts.len() < 2 {
        return None;
    }
    const KAPPA: f32 = 0.552_284_8;
    let mut sb = PathBuilder::stroke(px(stroke_w));
    sb.move_to(point(px(ox + pts[0].0), px(oy + pts[0].1)));

    for i in 1..pts.len() - 1 {
        let prev = pts[i - 1];
        let cur = pts[i];
        let next = pts[i + 1];

        let dx1 = cur.0 - prev.0;
        let dy1 = cur.1 - prev.1;
        let len1 = (dx1 * dx1 + dy1 * dy1).sqrt();

        let dx2 = next.0 - cur.0;
        let dy2 = next.1 - cur.1;
        let len2 = (dx2 * dx2 + dy2 * dy2).sqrt();

        let r = corner_radius.min(len1 * 0.45).min(len2 * 0.45);
        if r < 1.0 || len1 < 0.001 || len2 < 0.001 {
            sb.line_to(point(px(ox + cur.0), px(oy + cur.1)));
            continue;
        }
        let ux1 = dx1 / len1;
        let uy1 = dy1 / len1;
        let ux2 = dx2 / len2;
        let uy2 = dy2 / len2;

        let ax = cur.0 - ux1 * r;
        let ay = cur.1 - uy1 * r;
        let bx = cur.0 + ux2 * r;
        let by = cur.1 + uy2 * r;

        let c1x = ax + (cur.0 - ax) * KAPPA;
        let c1y = ay + (cur.1 - ay) * KAPPA;
        let c2x = bx + (cur.0 - bx) * KAPPA;
        let c2y = by + (cur.1 - by) * KAPPA;

        sb.line_to(point(px(ox + ax), px(oy + ay)));
        sb.cubic_bezier_to(
            point(px(ox + bx), px(oy + by)),
            point(px(ox + c1x), px(oy + c1y)),
            point(px(ox + c2x), px(oy + c2y)),
        );
    }

    let last = pts[pts.len() - 1];
    sb.line_to(point(px(ox + last.0), px(oy + last.1)));
    sb.build().ok()
}

fn hit_test_connector(
    specs: &[ConnectorSpec],
    local_x: f32,
    local_y: f32,
    tolerance: f32,
) -> Option<(String, String, String)> {
    let tol_sq = tolerance * tolerance;
    for spec in specs {
        for win in spec.points.windows(2) {
            let (x1, y1) = win[0];
            let (x2, y2) = win[1];
            let dx = x2 - x1;
            let dy = y2 - y1;
            let len_sq = dx * dx + dy * dy;
            let t = if len_sq > 0.001 {
                (((local_x - x1) * dx + (local_y - y1) * dy) / len_sq).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let px_x = x1 + t * dx;
            let px_y = y1 + t * dy;
            let dist_sq = (local_x - px_x).powi(2) + (local_y - px_y).powi(2);
            if dist_sq <= tol_sq {
                return Some((
                    spec.from_key.clone(),
                    spec.edge_name.clone(),
                    spec.to_key.clone(),
                ));
            }
        }
    }
    None
}

fn build_canvas_geometry(
    bounds: Bounds<Pixels>,
    zoom: f32,
    pan_x: f32,
    pan_y: f32,
    specs: &[ConnectorSpec],
    rel_focus: &RelationshipFocus,
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

    let has_focus = rel_focus.has_any_focus();
    let corner_r = (ORTHO_CORNER_RADIUS_PX * zoom).clamp(3.0, 12.0);

    for spec in specs {
        let is_active = rel_focus.is_spec_active(spec);
        let stroke_w = if is_active { 2.4 } else { 1.4 };

        let Some(sp) = build_orthogonal_rounded_path(&spec.points, ox, oy, corner_r, stroke_w)
        else {
            continue;
        };
        let mut ab = PathBuilder::fill();
        ab.move_to(point(px(ox + spec.arrow_tip.0), px(oy + spec.arrow_tip.1)));
        ab.line_to(point(px(ox + spec.arrow_a.0), px(oy + spec.arrow_a.1)));
        ab.line_to(point(px(ox + spec.arrow_b.0), px(oy + spec.arrow_b.1)));
        ab.close();

        if let Ok(ap) = ab.build() {
            if is_active {
                active_strokes.push(sp);
                active_arrows.push(ap);
            } else if has_focus {
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

fn render_er_minimap(
    ws: &TableMinusWorkspace,
    vis_nodes: &[ErTableNode],
    rel_focus: &RelationshipFocus,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let minimap_bg = theme::elevated_overlay_bg(cx);
    let border_col = theme::hairline_border_color(cx);
    let node_fill = with_alpha(colors.muted, 0.42);
    let node_active_fill = colors.accent.color;
    let vp_stroke = colors.accent.color;
    let vp_fill = with_alpha(colors.accent.color, 0.12);

    let zoom = ws.er_diagram_ui.zoom;
    let pan_x = ws.er_diagram_ui.pan_x;
    let pan_y = ws.er_diagram_ui.pan_y;
    let (raw_vw, raw_vh) = ws.er_diagram_ui.viewport_size.get();
    let vw = if raw_vw > 40.0 { raw_vw } else { 960.0 };
    let vh = if raw_vh > 40.0 { raw_vh } else { 640.0 };

    let nodes_snapshot: Vec<(f32, f32, f32, f32, bool)> = vis_nodes
        .iter()
        .map(|n| {
            let active = rel_focus.focus_table.as_deref() == Some(n.key.as_str());
            (n.x, n.y, n.width, n.height, active)
        })
        .collect();
    let bounds_cell = ws.er_diagram_ui.minimap_bounds.clone();

    div()
        .id("er-minimap")
        .absolute()
        .right(px(MINIMAP_MARGIN_PX))
        .bottom(px(MINIMAP_MARGIN_PX))
        .w(px(MINIMAP_W_PX))
        .h(px(MINIMAP_H_PX))
        .rounded(px(8.0))
        .border_1()
        .border_color(border_col)
        .bg(minimap_bg)
        .shadow_md()
        .overflow_hidden()
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|ws, ev: &gpui::MouseDownEvent, _w, cx| {
                ws.er_diagram_ui.panning_minimap = true;
                pan_to_minimap_pos(ws, f32::from(ev.position.x), f32::from(ev.position.y));
                cx.stop_propagation();
                cx.notify();
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|ws, _ev, _w, cx| {
                if ws.er_diagram_ui.panning_minimap {
                    ws.er_diagram_ui.panning_minimap = false;
                    cx.stop_propagation();
                    cx.notify();
                }
            }),
        )
        .child(
            canvas(
                move |bounds, _w, _cx| {
                    let ox = f32::from(bounds.origin.x);
                    let oy = f32::from(bounds.origin.y);
                    let bw = f32::from(bounds.size.width);
                    let bh = f32::from(bounds.size.height);
                    bounds_cell.set((ox, oy, bw, bh));
                    build_minimap_geometry(
                        (ox, oy, bw, bh),
                        &nodes_snapshot,
                        (zoom, pan_x, pan_y, vw, vh),
                    )
                },
                move |_bounds, geom, window, _cx| {
                    for p in &geom.node_rects {
                        window.paint_path(p.clone(), node_fill);
                    }
                    for p in &geom.active_rects {
                        window.paint_path(p.clone(), node_active_fill);
                    }
                    if let Some(ref vp_fill_path) = geom.viewport_fill {
                        window.paint_path(vp_fill_path.clone(), vp_fill);
                    }
                    if let Some(ref vp_stroke_path) = geom.viewport_stroke {
                        window.paint_path(vp_stroke_path.clone(), vp_stroke);
                    }
                },
            )
            .size_full(),
        )
        .into_any_element()
}

struct MinimapGeometry {
    node_rects: Vec<Path<Pixels>>,
    active_rects: Vec<Path<Pixels>>,
    viewport_fill: Option<Path<Pixels>>,
    viewport_stroke: Option<Path<Pixels>>,
}

fn build_minimap_geometry(
    bounds: (f32, f32, f32, f32),
    nodes: &[(f32, f32, f32, f32, bool)],
    viewport: (f32, f32, f32, f32, f32),
) -> MinimapGeometry {
    let (ox, oy, bw, bh) = bounds;
    let (zoom, pan_x, pan_y, vw, vh) = viewport;
    if nodes.is_empty() {
        return MinimapGeometry {
            node_rects: Vec::new(),
            active_rects: Vec::new(),
            viewport_fill: None,
            viewport_stroke: None,
        };
    }

    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for &(x, y, w, h, _) in nodes {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x + w);
        max_y = max_y.max(y + h);
    }

    let pad = 60.0_f32;
    let world_min_x = min_x - pad;
    let world_min_y = min_y - pad;
    let world_w = (max_x - min_x + pad * 2.0).max(1.0);
    let world_h = (max_y - min_y + pad * 2.0).max(1.0);

    let inner_w = (bw - MINIMAP_PAD_PX * 2.0).max(1.0);
    let inner_h = (bh - MINIMAP_PAD_PX * 2.0).max(1.0);
    let scale = (inner_w / world_w).min(inner_h / world_h);
    let map_ox = ox + MINIMAP_PAD_PX + (inner_w - world_w * scale) * 0.5;
    let map_oy = oy + MINIMAP_PAD_PX + (inner_h - world_h * scale) * 0.5;

    let mut node_builder = PathBuilder::fill();
    let mut active_builder = PathBuilder::fill();
    let mut has_normal = false;
    let mut has_active = false;

    for &(x, y, w, h, active) in nodes {
        let rx = map_ox + (x - world_min_x) * scale;
        let ry = map_oy + (y - world_min_y) * scale;
        let rw = (w * scale).max(3.0);
        let rh = (h * scale).max(2.5);
        let b = if active {
            has_active = true;
            &mut active_builder
        } else {
            has_normal = true;
            &mut node_builder
        };
        b.move_to(point(px(rx), px(ry)));
        b.line_to(point(px(rx + rw), px(ry)));
        b.line_to(point(px(rx + rw), px(ry + rh)));
        b.line_to(point(px(rx), px(ry + rh)));
        b.close();
    }

    let vp_world_x = -pan_x / zoom.max(0.05);
    let vp_world_y = -pan_y / zoom.max(0.05);
    let vp_world_w = vw / zoom.max(0.05);
    let vp_world_h = vh / zoom.max(0.05);

    let vx = (map_ox + (vp_world_x - world_min_x) * scale).clamp(ox + 2.0, ox + bw - 4.0);
    let vy = (map_oy + (vp_world_y - world_min_y) * scale).clamp(oy + 2.0, oy + bh - 4.0);
    let vx2 =
        (map_ox + (vp_world_x + vp_world_w - world_min_x) * scale).clamp(vx + 4.0, ox + bw - 2.0);
    let vy2 =
        (map_oy + (vp_world_y + vp_world_h - world_min_y) * scale).clamp(vy + 4.0, oy + bh - 2.0);

    let mut vp_fill_b = PathBuilder::fill();
    vp_fill_b.move_to(point(px(vx), px(vy)));
    vp_fill_b.line_to(point(px(vx2), px(vy)));
    vp_fill_b.line_to(point(px(vx2), px(vy2)));
    vp_fill_b.line_to(point(px(vx), px(vy2)));
    vp_fill_b.close();

    let mut vp_stroke_b = PathBuilder::stroke(px(1.25));
    vp_stroke_b.move_to(point(px(vx), px(vy)));
    vp_stroke_b.line_to(point(px(vx2), px(vy)));
    vp_stroke_b.line_to(point(px(vx2), px(vy2)));
    vp_stroke_b.line_to(point(px(vx), px(vy2)));
    vp_stroke_b.close();

    MinimapGeometry {
        node_rects: if has_normal {
            node_builder.build().into_iter().collect()
        } else {
            Vec::new()
        },
        active_rects: if has_active {
            active_builder.build().into_iter().collect()
        } else {
            Vec::new()
        },
        viewport_fill: vp_fill_b.build().ok(),
        viewport_stroke: vp_stroke_b.build().ok(),
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
    reason = "ER card renderer consumes positioned node, relationship focus, zoom/pan transform, and focus flags"
)]
fn render_er_table_card(
    ws: &TableMinusWorkspace,
    node: &ErTableNode,
    vis_edges: &[ErRelationEdge],
    rel_focus: &RelationshipFocus,
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
    let is_hovered = ws.er_diagram_ui.hovered_table.as_deref() == Some(node.key.as_str());

    let border_col = if is_primary_focus {
        colors.accent.color
    } else if is_highlighted || is_hovered {
        with_alpha(colors.accent.color, 0.65)
    } else {
        with_alpha(colors.foreground, 0.14)
    };
    let header_bg = if is_primary_focus {
        with_alpha(colors.accent.color, 0.22)
    } else if is_highlighted || is_hovered {
        with_alpha(colors.accent.color, 0.17)
    } else {
        with_alpha(colors.accent.color, 0.12)
    };
    let header_divider = if is_primary_focus || is_highlighted {
        with_alpha(colors.accent.color, 0.35)
    } else {
        colors.border
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
        .when(is_primary_focus || is_hovered, |el| el.shadow_lg())
        .when(!is_primary_focus && !is_hovered, |el| el.shadow_sm())
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
            header_divider,
            s.er_ref_badge,
            &colors,
        ));

    card = card.child(render_er_card_rows(
        ws,
        node,
        vis_edges,
        rel_focus,
        row_h,
        col_sz,
        zoom,
        s.er_fk_badge,
        s.er_more_suffix,
        &colors,
        cx,
    ));
    card.into_any_element()
}

#[expect(
    clippy::too_many_arguments,
    reason = "card row list renderer consumes table node, relationship focus, zoom metrics, and context"
)]
fn render_er_card_rows(
    ws: &TableMinusWorkspace,
    node: &ErTableNode,
    vis_edges: &[ErRelationEdge],
    rel_focus: &RelationshipFocus,
    row_h: f32,
    col_sz: f32,
    zoom: f32,
    fk_badge: &'static str,
    more_suffix: &'static str,
    colors: &herogpui::theme::ThemeColors,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let mut rows_col = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .py(px(3.0 * zoom));
    for col in node.columns.iter().take(MAX_VISIBLE_COLS) {
        let is_col_hovered = ws
            .er_diagram_ui
            .hovered_col
            .as_ref()
            .is_some_and(|(tk, cn)| tk == &node.key && cn == &col.name);
        let is_rel_highlighted = rel_focus.is_col_highlighted(&node.key, &col.name, vis_edges);
        rows_col = rows_col.child(render_er_column_row(
            &node.key,
            col,
            row_h,
            col_sz,
            zoom,
            fk_badge,
            is_col_hovered,
            is_rel_highlighted,
            colors,
            cx,
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
    divider_color: Hsla,
    ref_badge: &'static str,
    colors: &herogpui::theme::ThemeColors,
) -> AnyElement {
    div()
        .h(px(header_h))
        .px(px(10.0))
        .bg(header_bg)
        .border_b_1()
        .border_color(divider_color)
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

#[expect(
    clippy::too_many_arguments,
    reason = "column row renderer consumes column metadata, hover/relationship highlight flags, and context"
)]
fn render_er_column_row(
    table_key: &str,
    col: &ErColumnItem,
    row_h: f32,
    col_sz: f32,
    zoom: f32,
    fk_badge: &'static str,
    is_col_hovered: bool,
    is_rel_highlighted: bool,
    colors: &herogpui::theme::ThemeColors,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let pk_color = colors.warning.color;
    let fk_color = colors.accent.color;

    let row_bg = if is_rel_highlighted {
        Some(with_alpha(fk_color, 0.15))
    } else if is_col_hovered {
        Some(with_alpha(colors.foreground, 0.06))
    } else if col.is_pk {
        Some(with_alpha(pk_color, 0.06))
    } else if col.is_fk {
        Some(with_alpha(fk_color, 0.045))
    } else {
        None
    };

    let row_id = SharedString::from(format!("er-col-{table_key}-{}", col.name));
    let tk_hover = table_key.to_owned();
    let cn_hover = col.name.clone();
    let is_fk = col.is_fk;

    div()
        .id(row_id)
        .h(px(row_h))
        .px(px(10.0 * zoom))
        .when_some(row_bg, |el, bg| el.bg(bg))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(6.0))
        .on_hover(cx.listener(move |ws, hovered: &bool, _w, cx| {
            if *hovered {
                ws.er_diagram_ui.hovered_col = Some((tk_hover.clone(), cn_hover.clone()));
                if is_fk {
                    ws.er_diagram_ui.hovered_fk = Some((tk_hover.clone(), cn_hover.clone()));
                }
            } else {
                if ws
                    .er_diagram_ui
                    .hovered_col
                    .as_ref()
                    .is_some_and(|(t, c)| t == &tk_hover && c == &cn_hover)
                {
                    ws.er_diagram_ui.hovered_col = None;
                }
                if is_fk
                    && ws
                        .er_diagram_ui
                        .hovered_fk
                        .as_ref()
                        .is_some_and(|(t, c)| t == &tk_hover && c == &cn_hover)
                {
                    ws.er_diagram_ui.hovered_fk = None;
                }
            }
            cx.notify();
        }))
        .child(render_er_column_left(
            col,
            col_sz,
            zoom,
            fk_badge,
            is_rel_highlighted,
            colors,
        ))
        .when(zoom >= 0.55, |el| {
            el.child(render_er_column_right(col, col_sz, zoom, colors))
        })
        .into_any_element()
}

fn render_er_column_left(
    col: &ErColumnItem,
    col_sz: f32,
    zoom: f32,
    fk_badge: &'static str,
    is_rel_highlighted: bool,
    colors: &herogpui::theme::ThemeColors,
) -> AnyElement {
    let pk_color = colors.warning.color;
    let fk_color = colors.accent.color;
    div()
        .flex()
        .items_center()
        .gap(px(4.5 * zoom))
        .min_w_0()
        .when(col.is_pk, |el| {
            el.child(Icon::new(IconName::Key).size(IconSize::Xs).color(pk_color))
                .child(
                    div()
                        .px(px(3.5))
                        .rounded(px(3.0))
                        .bg(with_alpha(pk_color, 0.18))
                        .text_size(px((col_sz - 2.5).max(7.0)))
                        .font_weight(FontWeight::BOLD)
                        .text_color(pk_color)
                        .child("PK"),
                )
        })
        .when(col.is_fk, |el| {
            el.child(
                div()
                    .px(px(4.0))
                    .rounded(px(3.0))
                    .border_1()
                    .border_color(with_alpha(fk_color, 0.35))
                    .bg(with_alpha(fk_color, 0.14))
                    .text_size(px((col_sz - 2.5).max(7.0)))
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
                .font_weight(if col.is_pk || is_rel_highlighted {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if is_rel_highlighted {
                    fk_color
                } else {
                    colors.foreground
                })
                .child(col.name.clone()),
        )
        .into_any_element()
}

fn render_er_column_right(
    col: &ErColumnItem,
    col_sz: f32,
    zoom: f32,
    colors: &herogpui::theme::ThemeColors,
) -> AnyElement {
    let fk_color = colors.accent.color;
    let right_label = if zoom >= 0.72
        && let Some(ref target) = col.fk_target
    {
        format!("→ {target}")
    } else if col.nullable {
        format!("{}?", col.data_type)
    } else {
        col.data_type.clone()
    };
    let right_color = if col.fk_target.is_some() && zoom >= 0.72 {
        with_alpha(fk_color, 0.85)
    } else {
        colors.muted
    };
    div()
        .flex_shrink_0()
        .max_w(px(96.0 * zoom))
        .truncate()
        .text_size(px((col_sz - 1.2).max(7.0)))
        .text_color(right_color)
        .child(right_label)
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
        let path_d = build_orthogonal_svg_path(&spec.points, ORTHO_CORNER_RADIUS_PX);
        out.push_str(&format!(
            "<path d=\"{path_d}\" fill=\"none\" stroke=\"#64748b\" stroke-width=\"1.5\" marker-end=\"url(#arrow)\"/>\n",
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

fn build_orthogonal_svg_path(pts: &[(f32, f32)], corner_radius: f32) -> String {
    use std::fmt::Write as _;
    if pts.len() < 2 {
        return String::new();
    }
    let mut d = format!("M {:.1} {:.1}", pts[0].0, pts[0].1);
    for i in 1..pts.len() - 1 {
        let prev = pts[i - 1];
        let cur = pts[i];
        let next = pts[i + 1];
        let dx1 = cur.0 - prev.0;
        let dy1 = cur.1 - prev.1;
        let len1 = (dx1 * dx1 + dy1 * dy1).sqrt();
        let dx2 = next.0 - cur.0;
        let dy2 = next.1 - cur.1;
        let len2 = (dx2 * dx2 + dy2 * dy2).sqrt();
        let r = corner_radius.min(len1 * 0.45).min(len2 * 0.45);
        if r < 1.0 || len1 < 0.001 || len2 < 0.001 {
            let _ = write!(d, " L {:.1} {:.1}", cur.0, cur.1);
            continue;
        }
        let ax = cur.0 - (dx1 / len1) * r;
        let ay = cur.1 - (dy1 / len1) * r;
        let bx = cur.0 + (dx2 / len2) * r;
        let by = cur.1 + (dy2 / len2) * r;
        let _ = write!(
            d,
            " L {ax:.1} {ay:.1} Q {:.1} {:.1} {bx:.1} {by:.1}",
            cur.0, cur.1
        );
    }
    let last = pts[pts.len() - 1];
    let _ = write!(d, " L {:.1} {:.1}", last.0, last.1);
    d
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

    #[test]
    fn large_schema_auto_layout_has_no_overlaps_and_balanced_bounds() {
        let mut defs = Vec::new();
        for i in 0..72 {
            let mut fks = Vec::new();
            if i > 0 && i % 5 != 0 {
                let target = (i / 5) * 5;
                fks.push(ForeignKeyInfo {
                    name: format!("fk_t{i}_t{target}"),
                    columns: vec!["parent_id".to_owned()],
                    ref_schema: Some("public".to_owned()),
                    ref_table: format!("table_{target:02}"),
                    ref_columns: vec!["id".to_owned()],
                    on_delete: None,
                    on_update: None,
                });
            }
            defs.push(TableDefinition {
                table: TableRef::new(Some("public".to_owned()), format!("table_{i:02}")),
                columns: vec![
                    make_col("id", "bigint", true),
                    make_col("parent_id", "bigint", false),
                    make_col("label", "text", false),
                ],
                primary_key: None,
                indexes: Vec::new(),
                foreign_keys: fks,
                inbound_foreign_keys: Vec::new(),
                source: None,
                options: None,
            });
        }

        let (mut nodes, edges, _) = build_er_graph(&defs);
        apply_auto_layout(&mut nodes, &edges);

        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                let a = &nodes[i];
                let b = &nodes[j];
                let overlap_x = a.x < b.x + b.width && a.x + a.width > b.x;
                let overlap_y = a.y < b.y + b.height && a.y + a.height > b.y;
                assert!(
                    !(overlap_x && overlap_y),
                    "large schema cards overlapped: {} & {}",
                    a.key,
                    b.key
                );
            }
        }

        let refs: Vec<&ErTableNode> = nodes.iter().collect();
        let (min_x, min_y, max_x, max_y) = compute_nodes_bounds(&refs).expect("bounds");
        assert!(min_x >= 35.0);
        assert!(min_y >= 31.0);
        assert!(max_x - min_x > 500.0);
        assert!(max_y - min_y > 300.0);
    }

    #[test]
    fn orthogonal_routing_separates_parallel_lanes_and_hits_segments() {
        let n1 = ErTableNode {
            key: "a".to_owned(),
            schema: None,
            name: "a".to_owned(),
            columns: vec![
                ErColumnItem {
                    name: "id".to_owned(),
                    data_type: "int".to_owned(),
                    is_pk: true,
                    is_fk: false,
                    fk_target: None,
                    nullable: false,
                },
                ErColumnItem {
                    name: "b1".to_owned(),
                    data_type: "int".to_owned(),
                    is_pk: false,
                    is_fk: true,
                    fk_target: Some("b.id".to_owned()),
                    nullable: false,
                },
                ErColumnItem {
                    name: "b2".to_owned(),
                    data_type: "int".to_owned(),
                    is_pk: false,
                    is_fk: true,
                    fk_target: Some("b.id".to_owned()),
                    nullable: false,
                },
            ],
            has_inbound_fk: false,
            x: 40.0,
            y: 40.0,
            width: CARD_WIDTH_PX,
            height: 120.0,
        };
        let n2 = ErTableNode {
            key: "b".to_owned(),
            schema: None,
            name: "b".to_owned(),
            columns: vec![ErColumnItem {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                is_pk: true,
                is_fk: false,
                fk_target: None,
                nullable: false,
            }],
            has_inbound_fk: true,
            x: 440.0,
            y: 120.0,
            width: CARD_WIDTH_PX,
            height: 80.0,
        };
        let edges = vec![
            ErRelationEdge {
                name: "fk1".to_owned(),
                from_key: "a".to_owned(),
                from_cols: vec!["b1".to_owned()],
                to_key: "b".to_owned(),
                to_cols: vec!["id".to_owned()],
            },
            ErRelationEdge {
                name: "fk2".to_owned(),
                from_key: "a".to_owned(),
                from_cols: vec!["b2".to_owned()],
                to_key: "b".to_owned(),
                to_cols: vec!["id".to_owned()],
            },
        ];
        let map: HashMap<String, ErTableNode> = [("a".to_owned(), n1), ("b".to_owned(), n2)].into();
        let specs = compute_connector_specs(&edges, &map, 1.0, 0.0, 0.0);
        assert_eq!(specs.len(), 2);
        assert!(specs[0].points.len() >= 2);
        assert!(specs[1].points.len() >= 2);
        if specs[0].points.len() == 4 && specs[1].points.len() == 4 {
            assert!((specs[0].points[1].0 - specs[1].points[1].0).abs() >= 5.0);
        }
        let mid = specs[0].points[0];
        let hit = hit_test_connector(&specs, mid.0 + 4.0, mid.1, 6.0);
        assert_eq!(
            hit,
            Some(("a".to_owned(), "fk1".to_owned(), "b".to_owned()))
        );
    }
}
