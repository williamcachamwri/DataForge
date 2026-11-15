//! Visual EXPLAIN query execution plan tree view inside the Results Pane.

use std::sync::Arc;

use herogpui::components::{Button, Chip, ChipLabel, ChipVariant, Icon, IconName, IconSize};
use herogpui::core::{Color as HeroColor, Size as HeroSize, Variant, with_alpha};
use herogpui::gpui::{
    AnyElement, App, Context, FontFeatures, FontWeight, Hsla, InteractiveElement, IntoElement,
    MouseButton, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
    relative,
};
use herogpui::prelude::FluentBuilder;
use herogpui::theme::ActiveTheme;
use tableminus_services::ExplainNode;
use tableminus_state::connection_window::ConnectionWindowPhase;

use super::shell_state::ToastSeverity;
use super::{TableMinusWorkspace, theme};

/// Persistent UI state for the Visual EXPLAIN pane.
#[derive(Debug, Default)]
pub struct ExplainViewState {
    pub selected_node_id: Option<String>,
    pub last_error: Option<String>,
}

impl ExplainViewState {
    /// Creates initial Visual EXPLAIN state owned by [`TableMinusWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, _cx: &mut Context<TableMinusWorkspace>) -> Self {
        Self::default()
    }
}

struct PlanStats {
    total_nodes: usize,
    warning_count: usize,
    max_cost: f64,
    max_actual_ms: f64,
}

struct FlatExplainRow<'a> {
    depth: usize,
    node: &'a ExplainNode,
}

/// Renders the Visual EXPLAIN execution plan view inside the Results Pane.
#[must_use]
pub fn render_explain_view(
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
    let nodes = ws.controller.last_explain_nodes.clone();
    let stats = compute_plan_stats(&nodes);
    let header = render_explain_header(ws, is_connected, &stats, cx);
    let body = if nodes.is_empty() {
        render_explain_empty_state(ws, is_connected, cx)
    } else {
        render_explain_split_body(ws, &nodes, &stats, cx)
    };

    div()
        .id("explain-view-root")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme::panel_surface_bg(cx))
        .child(header)
        .child(div().flex_1().min_h_0().w_full().child(body))
        .into_any_element()
}

fn compute_plan_stats(nodes: &[ExplainNode]) -> PlanStats {
    let mut stats = PlanStats {
        total_nodes: 0,
        warning_count: 0,
        max_cost: 0.0,
        max_actual_ms: 0.0,
    };
    accumulate_stats(nodes, &mut stats);
    stats
}

fn accumulate_stats(nodes: &[ExplainNode], stats: &mut PlanStats) {
    for node in nodes {
        stats.total_nodes = stats.total_nodes.saturating_add(1);
        if node.is_seq_scan_warning {
            stats.warning_count = stats.warning_count.saturating_add(1);
        }
        if let Some(cost) = node.cost
            && cost.is_finite()
        {
            stats.max_cost = stats.max_cost.max(cost);
        }
        if let Some(ms) = node.actual_time_ms
            && ms.is_finite()
        {
            stats.max_actual_ms = stats.max_actual_ms.max(ms);
        }
        accumulate_stats(&node.children, stats);
    }
}

fn flatten_nodes<'a>(nodes: &'a [ExplainNode], depth: usize, out: &mut Vec<FlatExplainRow<'a>>) {
    for node in nodes {
        out.push(FlatExplainRow { depth, node });
        flatten_nodes(&node.children, depth + 1, out);
    }
}

fn find_node_by_id<'a>(nodes: &'a [ExplainNode], id_str: &str) -> Option<&'a ExplainNode> {
    let target_id = id_str.parse::<usize>().ok()?;
    find_node_recursive(nodes, target_id)
}

fn find_node_recursive(nodes: &[ExplainNode], target_id: usize) -> Option<&ExplainNode> {
    for node in nodes {
        if node.id == target_id {
            return Some(node);
        }
        if let Some(found) = find_node_recursive(&node.children, target_id) {
            return Some(found);
        }
    }
    None
}

fn render_explain_header(
    ws: &TableMinusWorkspace,
    is_connected: bool,
    stats: &PlanStats,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors();

    div()
        .min_h(px(38.0))
        .w_full()
        .px(px(12.0))
        .py(px(4.0))
        .border_b_1()
        .border_color(theme::hairline_border_color(cx))
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .min_w_0()
                .child(
                    Icon::new(IconName::Activity)
                        .size(IconSize::Sm)
                        .color(colors.accent.color),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .truncate()
                        .child(s.explain_visual_tab),
                )
                .when(stats.total_nodes > 0, |el| {
                    el.child(
                        Chip::new()
                            .variant(ChipVariant::Secondary)
                            .size(HeroSize::Sm)
                            .child(ChipLabel::new().child(format!(
                                "{} {}",
                                stats.total_nodes, s.explain_nodes_suffix
                            ))),
                    )
                })
                .when(stats.warning_count > 0, |el| {
                    el.child(
                        Chip::new()
                            .variant(ChipVariant::Soft)
                            .size(HeroSize::Sm)
                            .color(HeroColor::Warning)
                            .child(Icon::new(IconName::TriangleAlert).size(IconSize::Xs))
                            .child(ChipLabel::new().child(format!(
                                "{} {}",
                                stats.warning_count, s.explain_seq_scan_warn
                            ))),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .flex_shrink_0()
                .child(
                    Button::new("explain-run-plan-btn")
                        .variant(Variant::Secondary)
                        .size(HeroSize::Sm)
                        .is_disabled(!is_connected)
                        .child(Icon::new(IconName::Activity).size(IconSize::Xs))
                        .label(s.explain_plan)
                        .on_press(cx.listener(|ws, _, _, cx| {
                            trigger_visual_explain(ws, false, cx);
                        })),
                )
                .child(
                    Button::new("explain-run-analyze-btn")
                        .variant(Variant::Primary)
                        .size(HeroSize::Sm)
                        .is_disabled(!is_connected)
                        .child(Icon::new(IconName::Play).size(IconSize::Xs))
                        .label(s.explain_analyze)
                        .on_press(cx.listener(|ws, _, _, cx| {
                            trigger_visual_explain(ws, true, cx);
                        })),
                ),
        )
        .into_any_element()
}

fn trigger_visual_explain(
    ws: &mut TableMinusWorkspace,
    analyze: bool,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let sql = ws.controller.editor_sql_text();
    let label = if analyze {
        ws.controller.strings.explain_analyze
    } else {
        ws.controller.strings.explain_plan
    };
    let suffix = ws.controller.strings.explain_nodes_suffix;
    match ws.controller.run_visual_explain_blocking(&sql, analyze) {
        Ok(nodes) => {
            ws.explain_ui.last_error = None;
            ws.explain_ui.selected_node_id = nodes.first().map(|n| n.id.to_string());
            ws.notify_toast(
                ToastSeverity::Success,
                format!("{label}: {} {suffix}", nodes.len()),
                None,
                cx,
            );
            cx.notify();
        }
        Err(err) => {
            let msg = err.to_string();
            ws.explain_ui.last_error = Some(msg.clone());
            ws.notify_toast(ToastSeverity::Error, msg, None, cx);
            cx.notify();
        }
    }
}

fn render_explain_empty_state(
    ws: &TableMinusWorkspace,
    is_connected: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors();
    let sub = if !is_connected {
        s.no_active_connection_desc.to_owned()
    } else if let Some(err) = &ws.explain_ui.last_error {
        err.clone()
    } else {
        format!("{} / {}", s.explain_plan, s.explain_analyze)
    };

    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .p(px(20.0))
        .child(
            div()
                .w(px(38.0))
                .h(px(38.0))
                .rounded(px(10.0))
                .bg(with_alpha(colors.accent.color, 0.12))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(IconName::Activity)
                        .size(IconSize::Md)
                        .color(colors.accent.color),
                ),
        )
        .child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.foreground)
                .child(s.explain_visual_tab),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(if ws.explain_ui.last_error.is_some() {
                    colors.danger.color
                } else {
                    colors.muted
                })
                .child(sub),
        )
        .into_any_element()
}

fn render_explain_split_body(
    ws: &mut TableMinusWorkspace,
    nodes: &[ExplainNode],
    stats: &PlanStats,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let mut flat = Vec::new();
    flatten_nodes(nodes, 0, &mut flat);

    let selected_node = ws
        .explain_ui
        .selected_node_id
        .as_deref()
        .and_then(|id| find_node_by_id(nodes, id))
        .or_else(|| nodes.first());
    let selected_id = selected_node.map(|n| n.id);

    let mut tree_list = div()
        .id("explain-tree-scroll")
        .flex_1()
        .min_w_0()
        .min_h_0()
        .overflow_y_scroll()
        .p(px(10.0))
        .flex()
        .flex_col()
        .gap(px(6.0));

    for row in flat {
        let is_selected = selected_id == Some(row.node.id);
        tree_list = tree_list.child(render_explain_node_row(
            ws,
            row.depth,
            row.node,
            stats,
            is_selected,
            cx,
        ));
    }

    let inspector = selected_node
        .map(|node| render_explain_inspector(ws, node, stats, cx))
        .unwrap_or_else(|| div().into_any_element());

    div()
        .size_full()
        .flex()
        .flex_row()
        .min_h_0()
        .child(tree_list)
        .when(selected_node.is_some(), |el| el.child(inspector))
        .into_any_element()
}

fn render_explain_node_row(
    ws: &TableMinusWorkspace,
    depth: usize,
    node: &ExplainNode,
    stats: &PlanStats,
    is_selected: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &ws.controller.strings;
    let border = if is_selected {
        colors.accent.color
    } else if node.is_seq_scan_warning {
        with_alpha(colors.warning.color, 0.55)
    } else {
        theme::hairline_border_color(cx)
    };
    let card_bg = if is_selected {
        with_alpha(colors.accent.color, 0.10)
    } else {
        theme::elevated_overlay_bg(cx)
    };

    let node_id_str = node.id.to_string();
    let row_el_id = SharedString::from(format!("explain-node-{}", node.id));
    let indent_guides = render_indent_guides(depth, &colors);

    let card = div()
        .id(row_el_id)
        .flex_1()
        .min_w_0()
        .rounded(px(theme::PANEL_RADIUS_PX))
        .border_1()
        .border_color(border)
        .bg(card_bg)
        .px(px(10.0))
        .py(px(8.0))
        .cursor_pointer()
        .hover(|st| st.bg(with_alpha(colors.accent.color, 0.06)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, _ev, _w, cx| {
                ws.explain_ui.selected_node_id = Some(node_id_str.clone());
                cx.notify();
            }),
        )
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(render_node_top_row(node, s.explain_seq_scan_warn, &colors))
        .child(render_node_metrics_row(node, stats, s, &colors));

    div()
        .w_full()
        .flex()
        .flex_row()
        .items_stretch()
        .child(indent_guides)
        .child(card)
        .into_any_element()
}

fn render_indent_guides(depth: usize, colors: &herogpui::theme::ThemeColors) -> AnyElement {
    if depth == 0 {
        return div().into_any_element();
    }
    let guide_col = with_alpha(colors.muted, 0.28);
    let mut row = div().flex().flex_row().flex_shrink_0();
    for lvl in 0..depth {
        let is_last = lvl + 1 == depth;
        row = row.child(
            div()
                .w(px(18.0))
                .relative()
                .flex()
                .items_center()
                .child(div().ml(px(7.0)).w(px(1.0)).h_full().bg(guide_col))
                .when(is_last, |el| {
                    el.child(div().w(px(9.0)).h(px(1.0)).bg(guide_col))
                }),
        );
    }
    row.into_any_element()
}

fn render_node_top_row(
    node: &ExplainNode,
    warn_label: &'static str,
    colors: &herogpui::theme::ThemeColors,
) -> AnyElement {
    let op_upper = node.operation.to_ascii_uppercase();
    let op_color = if node.is_seq_scan_warning {
        HeroColor::Warning
    } else if op_upper.contains("INDEX") || op_upper.contains("SEARCH") {
        HeroColor::Success
    } else {
        HeroColor::Accent
    };

    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.0))
        .min_w_0()
        .child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .size(HeroSize::Sm)
                .color(op_color)
                .child(ChipLabel::new().child(node.operation.clone())),
        )
        .when_some(node.relation.clone(), |el, rel| {
            el.child(
                div()
                    .px(px(6.0))
                    .py(px(2.0))
                    .rounded(px(4.0))
                    .bg(with_alpha(colors.foreground, 0.06))
                    .text_size(px(11.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.foreground)
                    .truncate()
                    .child(rel),
            )
        })
        .when(node.is_seq_scan_warning, |el| {
            el.child(
                Chip::new()
                    .variant(ChipVariant::Soft)
                    .size(HeroSize::Sm)
                    .color(HeroColor::Danger)
                    .child(Icon::new(IconName::TriangleAlert).size(IconSize::Xs))
                    .child(ChipLabel::new().child(warn_label)),
            )
        })
        .into_any_element()
}

fn render_node_metrics_row(
    node: &ExplainNode,
    stats: &PlanStats,
    s: &crate::i18n::Strings,
    colors: &herogpui::theme::ThemeColors,
) -> AnyElement {
    let tnum = FontFeatures(Arc::new(vec![("tnum".to_owned(), 1)]));
    let cost_ratio = match node.cost {
        Some(c) if stats.max_cost > 0.0 => (c / stats.max_cost).clamp(0.04, 1.0) as f32,
        _ => 0.0,
    };
    let bar_color = cost_bar_color(cost_ratio, node.is_seq_scan_warning, colors);

    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(12.0))
        .font_features(tnum)
        .when_some(node.cost, |el, cost| {
            el.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(colors.muted)
                            .child(format!("{}: {cost:.2}", s.explain_cost)),
                    )
                    .child(
                        div()
                            .w(px(64.0))
                            .h(px(5.0))
                            .rounded(px(999.0))
                            .bg(with_alpha(colors.foreground, 0.08))
                            .overflow_hidden()
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(cost_ratio))
                                    .rounded(px(999.0))
                                    .bg(bar_color),
                            ),
                    ),
            )
        })
        .when_some(node.estimated_rows, |el, rows| {
            let txt = match node.actual_rows {
                Some(act) => format!("{}: {rows:.0} (act {act})", s.explain_est_rows),
                None => format!("{}: {rows:.0}", s.explain_est_rows),
            };
            el.child(
                div()
                    .text_size(px(11.0))
                    .text_color(colors.muted)
                    .child(txt),
            )
        })
        .when_some(node.actual_time_ms, |el, ms| {
            el.child(
                div()
                    .text_size(px(11.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.accent.color)
                    .child(format!("{}: {ms:.2} ms", s.explain_actual_time)),
            )
        })
        .when(
            node.cost.is_none() && node.estimated_rows.is_none() && node.actual_time_ms.is_none(),
            |el| {
                el.child(
                    div()
                        .text_size(px(11.0))
                        .text_color(colors.muted)
                        .truncate()
                        .child(node.label.clone()),
                )
            },
        )
        .into_any_element()
}

fn cost_bar_color(
    ratio: f32,
    is_seq_scan_warning: bool,
    colors: &herogpui::theme::ThemeColors,
) -> Hsla {
    if is_seq_scan_warning || ratio > 0.75 {
        colors.danger.color
    } else if ratio > 0.45 {
        colors.warning.color
    } else {
        colors.accent.color
    }
}

fn render_explain_inspector(
    ws: &TableMinusWorkspace,
    node: &ExplainNode,
    stats: &PlanStats,
    cx: &App,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors();
    let border = theme::hairline_border_color(cx);
    let code_font = theme::editor_gpui_font(&ws.controller.settings);

    let mut pairs: Vec<(&str, String)> = Vec::new();
    pairs.push((s.explain_node_label, node.operation.clone()));
    if let Some(ref rel) = node.relation {
        pairs.push((s.explain_relation_label, rel.clone()));
    }
    if let Some(cost) = node.cost {
        let pct = if stats.max_cost > 0.0 {
            format!(" ({:.0}%)", (cost / stats.max_cost) * 100.0)
        } else {
            String::new()
        };
        pairs.push((s.explain_cost, format!("{cost:.2}{pct}")));
    }
    if let Some(est) = node.estimated_rows {
        pairs.push((s.explain_est_rows, format!("{est:.0}")));
    }
    if let Some(act) = node.actual_rows {
        pairs.push((s.explain_actual_rows_label, act.to_string()));
    }
    if let Some(ms) = node.actual_time_ms {
        pairs.push((s.explain_actual_time, format!("{ms:.3} ms")));
    }

    let detail_text = if node.detail.trim().is_empty() {
        node.label.clone()
    } else {
        node.detail.clone()
    };

    div()
        .w(px(280.0))
        .flex_shrink_0()
        .h_full()
        .border_l_1()
        .border_color(border)
        .bg(theme::muted_surface_bg(cx))
        .flex()
        .flex_col()
        .child(
            div()
                .id("explain-inspector-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(12.0))
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(node.label.clone()),
                )
                .when(node.is_seq_scan_warning, |el| {
                    el.child(
                        div()
                            .px(px(8.0))
                            .py(px(6.0))
                            .rounded(px(6.0))
                            .bg(with_alpha(colors.warning.color, 0.14))
                            .border_1()
                            .border_color(with_alpha(colors.warning.color, 0.45))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                Icon::new(IconName::TriangleAlert)
                                    .size(IconSize::Xs)
                                    .color(colors.warning.color),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(colors.warning.color)
                                    .child(s.explain_seq_scan_warn),
                            ),
                    )
                })
                .child(render_inspector_kv_table(&pairs, border, colors))
                .child(
                    div()
                        .p(px(8.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(border)
                        .bg(theme::elevated_overlay_bg(cx))
                        .font(code_font)
                        .text_size(px(11.0))
                        .text_color(colors.foreground)
                        .child(detail_text),
                ),
        )
        .into_any_element()
}

fn render_inspector_kv_table(
    pairs: &[(&'static str, String)],
    border: Hsla,
    colors: &herogpui::theme::ThemeColors,
) -> AnyElement {
    let mut table = div().flex().flex_col().gap(px(4.0));
    for (k, v) in pairs {
        table = table.child(
            div()
                .pb(px(4.0))
                .border_b_1()
                .border_color(border)
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(div().text_size(px(11.0)).text_color(colors.muted).child(*k))
                .child(
                    div()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.foreground)
                        .truncate()
                        .child(v.clone()),
                ),
        );
    }
    table.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_plan() -> Vec<ExplainNode> {
        vec![ExplainNode {
            id: 1,
            label: "Hash Join".to_owned(),
            detail: "Hash Cond: (o.user_id = u.id)".to_owned(),
            operation: "Hash Join".to_owned(),
            relation: None,
            cost: Some(120.0),
            estimated_rows: Some(250.0),
            actual_time_ms: Some(4.5),
            actual_rows: Some(240),
            is_seq_scan_warning: false,
            children: vec![ExplainNode {
                id: 2,
                label: "Seq Scan on orders".to_owned(),
                detail: "Filter: (total > 10)".to_owned(),
                operation: "Seq Scan".to_owned(),
                relation: Some("orders".to_owned()),
                cost: Some(80.0),
                estimated_rows: Some(1000.0),
                actual_time_ms: Some(2.1),
                actual_rows: Some(980),
                is_seq_scan_warning: true,
                children: Vec::new(),
            }],
        }]
    }

    #[test]
    fn explain_stats_and_tree_flattening_track_depths_and_warnings() {
        let plan = sample_plan();
        let stats = compute_plan_stats(&plan);
        assert_eq!(stats.total_nodes, 2);
        assert_eq!(stats.warning_count, 1);
        assert!((stats.max_cost - 120.0).abs() < f64::EPSILON);
        assert!((stats.max_actual_ms - 4.5).abs() < f64::EPSILON);

        let mut flat = Vec::new();
        flatten_nodes(&plan, 0, &mut flat);
        assert_eq!(flat.len(), 2);
        assert_eq!(flat[0].depth, 0);
        assert_eq!(flat[1].depth, 1);
        assert_eq!(find_node_by_id(&plan, "2").map(|n| n.id), Some(2));
        assert!(find_node_by_id(&plan, "99").is_none());
    }
}
