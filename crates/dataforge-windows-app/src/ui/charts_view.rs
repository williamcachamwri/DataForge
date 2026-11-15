//! Interactive result-set chart visualization pane (Bar, Line, Donut).

use std::collections::HashMap;
use std::f32::consts::TAU;
use std::sync::Arc;

use herogpui::components::{Button, Icon, IconName, IconSize, PickerItem, Select};
use herogpui::core::{Size as HeroSize, Variant, with_alpha};
use herogpui::gpui::{
    AnyElement, App, Bounds, Context, FontFeatures, FontWeight, Hsla, InteractiveElement,
    IntoElement, ParentElement, Path, PathBuilder, Pixels, Rgba, SharedString,
    StatefulInteractiveElement, Styled, Window, canvas, div, point, px,
};
use herogpui::theme::ActiveTheme;
use tablepro_core::column::ResultColumn;
use tablepro_core::value::Value;

use super::results_pane::format_core_value;
use super::{DataForgeWorkspace, theme};

const MAX_BAR_LINE_POINTS: usize = 24;
const MAX_DONUT_SLICES: usize = 7;

/// Supported chart visualization modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChartKind {
    #[default]
    Bar,
    Line,
    Donut,
}

/// Supported numeric aggregation modes over grouped X-axis categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChartAggregation {
    Count,
    #[default]
    Sum,
    Avg,
    Min,
    Max,
}

impl ChartAggregation {
    const ALL: [Self; 5] = [Self::Count, Self::Sum, Self::Avg, Self::Min, Self::Max];

    const fn key(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Sum => "sum",
            Self::Avg => "avg",
            Self::Min => "min",
            Self::Max => "max",
        }
    }

    fn from_key(key: &str) -> Self {
        match key {
            "count" => Self::Count,
            "avg" => Self::Avg,
            "min" => Self::Min,
            "max" => Self::Max,
            _ => Self::Sum,
        }
    }

    fn label(self, s: &crate::i18n::Strings) -> &'static str {
        match self {
            Self::Count => s.grid_agg_count,
            Self::Sum => s.grid_agg_sum,
            Self::Avg => s.grid_agg_avg,
            Self::Min => s.grid_agg_min,
            Self::Max => s.grid_agg_max,
        }
    }
}

/// Persistent UI state for the Result Charts sub-view.
#[derive(Debug, Default)]
pub struct ChartsViewState {
    pub x_column: Option<usize>,
    pub y_column: Option<usize>,
    pub chart_kind: ChartKind,
    pub agg_mode: ChartAggregation,
}

impl ChartsViewState {
    /// Creates initial Result Charts state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, _cx: &mut Context<DataForgeWorkspace>) -> Self {
        Self::default()
    }
}

struct ChartPoint {
    label: String,
    value: f64,
}

/// Renders the Result Charts view inside the Results Pane.
#[must_use]
pub fn render_charts_view(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let (columns, rows) = active_chart_dataset(ws);
    let numeric_cols = detect_numeric_columns(&columns, &rows);
    let (x_idx, y_idx) = resolve_axis_indices(ws, &columns, &numeric_cols);
    let agg_mode = ws.charts_ui.agg_mode;
    let chart_kind = ws.charts_ui.chart_kind;

    let controls = render_charts_controls_bar(
        ws,
        &columns,
        &numeric_cols,
        x_idx,
        y_idx,
        chart_kind,
        agg_mode,
        cx,
    );

    let can_chart = !columns.is_empty()
        && !rows.is_empty()
        && (!numeric_cols.is_empty() || agg_mode == ChartAggregation::Count);

    let body = if !can_chart {
        render_charts_empty_state(ws, cx)
    } else {
        let series = aggregate_chart_series(&rows, x_idx, y_idx, agg_mode, chart_kind);
        if series.is_empty() {
            render_charts_empty_state(ws, cx)
        } else {
            render_chart_canvas_area(ws, chart_kind, agg_mode, &series, cx)
        }
    };

    div()
        .id("charts-view-root")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme::panel_surface_bg(cx))
        .child(controls)
        .child(div().flex_1().min_h_0().w_full().child(body))
        .into_any_element()
}

fn active_chart_dataset(ws: &DataForgeWorkspace) -> (Vec<ResultColumn>, Vec<Vec<Value>>) {
    if let Some(tab_id) = ws.controller.tabs.active_tab()
        && let Some(res) = ws.tab_results.get(&tab_id)
        && let Some(set) = res.accumulator.result_sets.get(res.active_result_set)
        && !set.columns.is_empty()
    {
        return (set.columns.to_vec(), set.rows.clone());
    }
    (
        ws.controller.grid_raw_columns.clone(),
        ws.controller.grid_raw_rows.clone(),
    )
}

fn detect_numeric_columns(columns: &[ResultColumn], rows: &[Vec<Value>]) -> Vec<usize> {
    let mut numeric = Vec::new();
    let sample_len = rows.len().min(200);
    for (col_idx, _col) in columns.iter().enumerate() {
        let mut saw_non_null = false;
        let mut all_numeric = true;
        for row in rows.iter().take(sample_len) {
            if let Some(val) = row.get(col_idx)
                && !matches!(val, Value::Null)
            {
                saw_non_null = true;
                if value_to_f64(val).is_none() {
                    all_numeric = false;
                    break;
                }
            }
        }
        if saw_non_null && all_numeric {
            numeric.push(col_idx);
        }
    }
    numeric
}

fn value_to_f64(val: &Value) -> Option<f64> {
    let num = match val {
        Value::Int32(v) => f64::from(*v),
        Value::Int(v) => *v as f64,
        Value::UInt(v) => *v as f64,
        Value::WideInt(v) => v.to_string().parse::<f64>().ok()?,
        Value::Float32(v) => f64::from(*v),
        Value::Float64(v) => *v,
        Value::Decimal(d) => d.to_string().parse::<f64>().ok()?,
        Value::Text(s) => s.trim().parse::<f64>().ok()?,
        Value::Null
        | Value::Bool(_)
        | Value::Bytes(_)
        | Value::Uuid(_)
        | Value::Json(_)
        | Value::Date(_)
        | Value::Time(_)
        | Value::TimeTz(_)
        | Value::Timestamp(_)
        | Value::TimestampTz(_)
        | Value::Interval(_)
        | Value::Bits(_)
        | Value::Array(_)
        | Value::Other(_)
        | Value::Undecodable(_) => return None,
    };
    num.is_finite().then_some(num)
}

fn resolve_axis_indices(
    ws: &DataForgeWorkspace,
    columns: &[ResultColumn],
    numeric_cols: &[usize],
) -> (usize, Option<usize>) {
    if columns.is_empty() {
        return (0, None);
    }
    let x_idx = ws
        .charts_ui
        .x_column
        .filter(|&i| i < columns.len())
        .unwrap_or_else(|| {
            (0..columns.len())
                .find(|i| !numeric_cols.contains(i))
                .unwrap_or(0)
        });
    let y_idx = ws
        .charts_ui
        .y_column
        .filter(|&i| {
            i < columns.len()
                && (ws.charts_ui.agg_mode == ChartAggregation::Count || numeric_cols.contains(&i))
        })
        .or_else(|| numeric_cols.iter().copied().find(|&i| i != x_idx))
        .or_else(|| numeric_cols.first().copied())
        .or(Some(0));
    (x_idx, y_idx)
}

#[expect(
    clippy::too_many_arguments,
    reason = "charts toolbar needs axis options, numeric column flags, and active mode selections"
)]
fn render_charts_controls_bar(
    ws: &DataForgeWorkspace,
    columns: &[ResultColumn],
    numeric_cols: &[usize],
    x_idx: usize,
    y_idx: Option<usize>,
    chart_kind: ChartKind,
    agg_mode: ChartAggregation,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();

    let kind_pills = div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(render_chart_type_btn(
            "charts-type-bar",
            IconName::ChartBar,
            s.charts_bar,
            chart_kind == ChartKind::Bar,
            ChartKind::Bar,
            cx,
        ))
        .child(render_chart_type_btn(
            "charts-type-line",
            IconName::ChartLine,
            s.charts_line,
            chart_kind == ChartKind::Line,
            ChartKind::Line,
            cx,
        ))
        .child(render_chart_type_btn(
            "charts-type-donut",
            IconName::ChartPie,
            s.charts_donut,
            chart_kind == ChartKind::Donut,
            ChartKind::Donut,
            cx,
        ));

    let x_items: Vec<PickerItem> = columns
        .iter()
        .enumerate()
        .map(|(i, c)| PickerItem::new(i.to_string(), c.name.clone()))
        .collect();
    let y_items: Vec<PickerItem> = columns
        .iter()
        .enumerate()
        .filter(|(i, _)| agg_mode == ChartAggregation::Count || numeric_cols.contains(i))
        .map(|(i, c)| PickerItem::new(i.to_string(), c.name.clone()))
        .collect();
    let agg_items: Vec<PickerItem> = ChartAggregation::ALL
        .iter()
        .map(|a| PickerItem::new(a.key(), a.label(s)))
        .collect();

    div()
        .min_h(px(38.0))
        .w_full()
        .px(px(12.0))
        .py(px(5.0))
        .border_b_1()
        .border_color(theme::hairline_border_color(cx))
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .child(kind_pills)
        .child(render_charts_axis_controls(
            &colors,
            &ws.controller.strings,
            (x_items, y_items, agg_items),
            (x_idx, y_idx, agg_mode),
            cx,
        ))
        .into_any_element()
}

fn render_charts_axis_controls(
    colors: &herogpui::theme::ThemeColors,
    s: &crate::i18n::Strings,
    items: (Vec<PickerItem>, Vec<PickerItem>, Vec<PickerItem>),
    selected: (usize, Option<usize>, ChartAggregation),
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let (x_items, y_items, agg_items) = items;
    let (x_idx, y_idx, agg_mode) = selected;
    let has_cols = !x_items.is_empty();
    let has_y = !y_items.is_empty();

    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.0))
        .child(render_axis_select(
            "charts-x-select",
            s.charts_x_axis,
            x_items,
            has_cols.then(|| SharedString::from(x_idx.to_string())),
            !has_cols,
            colors,
            cx.listener(|ws, key: &Option<SharedString>, _, cx| {
                if let Some(idx) = key.as_ref().and_then(|k| k.parse::<usize>().ok()) {
                    ws.charts_ui.x_column = Some(idx);
                    cx.notify();
                }
            }),
        ))
        .child(render_axis_select(
            "charts-y-select",
            s.charts_y_axis,
            y_items,
            y_idx
                .filter(|_| has_y)
                .map(|i| SharedString::from(i.to_string())),
            !has_y,
            colors,
            cx.listener(|ws, key: &Option<SharedString>, _, cx| {
                if let Some(idx) = key.as_ref().and_then(|k| k.parse::<usize>().ok()) {
                    ws.charts_ui.y_column = Some(idx);
                    cx.notify();
                }
            }),
        ))
        .child(render_axis_select(
            "charts-agg-select",
            s.charts_agg,
            agg_items,
            Some(SharedString::from(agg_mode.key())),
            !has_cols,
            colors,
            cx.listener(|ws, key: &Option<SharedString>, _, cx| {
                if let Some(k) = key.as_ref() {
                    ws.charts_ui.agg_mode = ChartAggregation::from_key(k.as_ref());
                    cx.notify();
                }
            }),
        ))
        .into_any_element()
}

fn render_chart_type_btn(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    active: bool,
    kind: ChartKind,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    Button::new(id)
        .variant(if active {
            Variant::Primary
        } else {
            Variant::Tertiary
        })
        .size(HeroSize::Sm)
        .child(Icon::new(icon).size(IconSize::Xs))
        .label(label)
        .on_press(cx.listener(move |ws, _, _, cx| {
            ws.charts_ui.chart_kind = kind;
            cx.notify();
        }))
        .into_any_element()
}

fn render_axis_select(
    id: &'static str,
    label: &'static str,
    items: Vec<PickerItem>,
    value: Option<SharedString>,
    disabled: bool,
    colors: &herogpui::theme::ThemeColors,
    on_change: impl Fn(&Option<SharedString>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(colors.muted)
                .child(label),
        )
        .child(
            div().w(px(124.0)).child(
                Select::new(id, items)
                    .aria_label(label)
                    .value(value)
                    .is_disabled(disabled)
                    .full_width(true)
                    .height(px(26.0))
                    .trigger_text_size(px(11.5))
                    .row_text_size(px(11.5))
                    .on_selection_change(on_change),
            ),
        )
        .into_any_element()
}

fn render_charts_empty_state(ws: &DataForgeWorkspace, cx: &App) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();

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
                    Icon::new(IconName::ChartBar)
                        .size(IconSize::Md)
                        .color(colors.accent.color),
                ),
        )
        .child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.foreground)
                .child(s.charts_tab),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(colors.muted)
                .child(s.charts_no_numeric),
        )
        .into_any_element()
}

struct GroupAcc {
    count: usize,
    sum: f64,
    num_count: usize,
    min: Option<f64>,
    max: Option<f64>,
}

fn aggregate_chart_series(
    rows: &[Vec<Value>],
    x_idx: usize,
    y_idx: Option<usize>,
    agg: ChartAggregation,
    kind: ChartKind,
) -> Vec<ChartPoint> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, GroupAcc> = HashMap::new();

    for row in rows {
        let raw_x = row.get(x_idx).map_or_else(
            || "NULL".to_owned(),
            |v| {
                if v.is_null() {
                    "NULL".to_owned()
                } else {
                    format_core_value(v)
                }
            },
        );
        let acc = groups.entry(raw_x.clone()).or_insert_with(|| {
            order.push(raw_x);
            GroupAcc {
                count: 0,
                sum: 0.0,
                num_count: 0,
                min: None,
                max: None,
            }
        });
        acc.count = acc.count.saturating_add(1);
        if let Some(yi) = y_idx
            && let Some(val) = row.get(yi)
            && let Some(num) = value_to_f64(val)
        {
            acc.sum += num;
            acc.num_count = acc.num_count.saturating_add(1);
            acc.min = Some(acc.min.map_or(num, |m| m.min(num)));
            acc.max = Some(acc.max.map_or(num, |m| m.max(num)));
        }
    }

    let mut pts: Vec<ChartPoint> = order
        .into_iter()
        .filter_map(|label| {
            let acc = groups.get(&label)?;
            let val = match agg {
                ChartAggregation::Count => Some(acc.count as f64),
                ChartAggregation::Sum => (acc.num_count > 0).then_some(acc.sum),
                ChartAggregation::Avg => {
                    (acc.num_count > 0).then(|| acc.sum / acc.num_count as f64)
                }
                ChartAggregation::Min => acc.min,
                ChartAggregation::Max => acc.max,
            }?;
            Some(ChartPoint { label, value: val })
        })
        .collect();

    if kind == ChartKind::Donut {
        pts.retain(|p| p.value > 0.0);
        pts.sort_by(|a, b| b.value.total_cmp(&a.value));
        if pts.len() > MAX_DONUT_SLICES {
            let tail_sum: f64 = pts.drain(MAX_DONUT_SLICES..).map(|p| p.value).sum();
            if tail_sum > 0.0 {
                pts.push(ChartPoint {
                    label: "Other".to_owned(),
                    value: tail_sum,
                });
            }
        }
    } else if pts.len() > MAX_BAR_LINE_POINTS {
        pts.truncate(MAX_BAR_LINE_POINTS);
    }
    pts
}

fn render_chart_canvas_area(
    ws: &DataForgeWorkspace,
    kind: ChartKind,
    agg: ChartAggregation,
    series: &[ChartPoint],
    cx: &App,
) -> AnyElement {
    let s = &ws.controller.strings;
    let tnum = FontFeatures(Arc::new(vec![("tnum".to_owned(), 1)]));
    let inner = match kind {
        ChartKind::Bar => render_bar_or_line_chart(series, true, cx),
        ChartKind::Line => render_bar_or_line_chart(series, false, cx),
        ChartKind::Donut => render_donut_chart(series, agg.label(s), cx),
    };

    div()
        .size_full()
        .p(px(12.0))
        .font_features(tnum)
        .child(inner)
        .into_any_element()
}

fn chart_palette(accent: Hsla) -> [Hsla; 8] {
    [
        accent,
        rgb_hsla(0x10, 0xB9, 0x81),
        rgb_hsla(0xF5, 0x9E, 0x0B),
        rgb_hsla(0x8B, 0x5C, 0xF6),
        rgb_hsla(0xEC, 0x48, 0x99),
        rgb_hsla(0x06, 0xB6, 0xD4),
        rgb_hsla(0xF9, 0x73, 0x16),
        rgb_hsla(0x63, 0x66, 0xF1),
    ]
}

fn rgb_hsla(r: u8, g: u8, b: u8) -> Hsla {
    Hsla::from(Rgba {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        a: 1.0,
    })
}

fn render_bar_or_line_chart(series: &[ChartPoint], is_bar: bool, cx: &App) -> AnyElement {
    let colors = cx.colors();
    let accent = colors.accent.color;
    let guide_color = with_alpha(colors.muted, 0.22);
    let fill_soft = with_alpha(accent, 0.14);

    let raw_min = series.iter().map(|p| p.value).fold(f64::INFINITY, f64::min);
    let raw_max = series
        .iter()
        .map(|p| p.value)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_v = raw_min.min(0.0);
    let mut max_v = raw_max.max(0.0);
    if (max_v - min_v).abs() < f64::EPSILON {
        max_v = min_v + 1.0;
    }

    let values: Vec<f64> = series.iter().map(|p| p.value).collect();
    let y_ticks = [
        max_v,
        min_v + (max_v - min_v) * 0.75,
        min_v + (max_v - min_v) * 0.50,
        min_v + (max_v - min_v) * 0.25,
        min_v,
    ];

    let y_col = div()
        .w(px(58.0))
        .h_full()
        .pb(px(22.0))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .justify_between()
        .items_end()
        .pr(px(8.0))
        .children(y_ticks.iter().map(|t| {
            div()
                .text_size(px(10.5))
                .text_color(colors.muted)
                .child(format_compact_number(*t))
        }));

    let plot_canvas = canvas(
        move |bounds, _w, _cx| {
            if is_bar {
                build_bar_geometry(bounds, &values, min_v, max_v)
            } else {
                build_line_geometry(bounds, &values, min_v, max_v)
            }
        },
        move |_bounds, geom, window, _cx| {
            for g in &geom.guides {
                window.paint_path(g.clone(), guide_color);
            }
            for f in &geom.fills {
                window.paint_path(f.clone(), if is_bar { accent } else { fill_soft });
            }
            for s in &geom.strokes {
                window.paint_path(s.clone(), accent);
            }
            for d in &geom.dots {
                window.paint_path(d.clone(), accent);
            }
        },
    )
    .flex_1()
    .min_h_0()
    .w_full();

    let x_labels = render_x_axis_labels(series, colors);

    div()
        .size_full()
        .flex()
        .flex_row()
        .child(y_col)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .child(plot_canvas)
                .child(x_labels),
        )
        .into_any_element()
}

fn render_x_axis_labels(
    series: &[ChartPoint],
    colors: &herogpui::theme::ThemeColors,
) -> AnyElement {
    let step = (series.len() / 10).max(1);
    let mut row = div()
        .h(px(22.0))
        .w_full()
        .pt(px(4.0))
        .flex()
        .flex_row()
        .items_center();
    for (idx, pt) in series.iter().enumerate() {
        let show = idx % step == 0 || idx + 1 == series.len();
        row = row.child(
            div()
                .flex_1()
                .min_w_0()
                .px(px(2.0))
                .flex()
                .justify_center()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(10.5))
                        .text_color(colors.muted)
                        .child(if show {
                            pt.label.clone()
                        } else {
                            String::new()
                        }),
                ),
        );
    }
    row.into_any_element()
}

struct PlotGeometry {
    guides: Vec<Path<Pixels>>,
    fills: Vec<Path<Pixels>>,
    strokes: Vec<Path<Pixels>>,
    dots: Vec<Path<Pixels>>,
}

fn build_guide_lines(ox: f32, oy: f32, bw: f32, bh: f32) -> Vec<Path<Pixels>> {
    let mut out = Vec::with_capacity(5);
    for i in 0..=4 {
        let y = oy + (bh * (i as f32 / 4.0));
        let mut pb = PathBuilder::stroke(px(1.0));
        pb.move_to(point(px(ox), px(y)));
        pb.line_to(point(px(ox + bw), px(y)));
        if let Ok(path) = pb.build() {
            out.push(path);
        }
    }
    out
}

fn build_bar_geometry(
    bounds: Bounds<Pixels>,
    values: &[f64],
    min_v: f64,
    max_v: f64,
) -> PlotGeometry {
    let ox = f32::from(bounds.origin.x);
    let oy = f32::from(bounds.origin.y) + 6.0;
    let bw = f32::from(bounds.size.width).max(1.0);
    let bh = (f32::from(bounds.size.height) - 10.0).max(1.0);
    let guides = build_guide_lines(ox, oy, bw, bh);
    let mut fills = Vec::with_capacity(values.len());

    let span = (max_v - min_v).max(1e-9);
    let zero_y = oy + bh * (1.0 - ((0.0 - min_v) / span) as f32).clamp(0.0, 1.0);
    let slot_w = bw / (values.len().max(1) as f32);
    let bar_w = (slot_w * 0.62).clamp(6.0, 46.0);

    for (idx, &val) in values.iter().enumerate() {
        let cx = ox + slot_w * (idx as f32 + 0.5);
        let x0 = cx - bar_w * 0.5;
        let x1 = cx + bar_w * 0.5;
        let val_y = oy + bh * (1.0 - ((val - min_v) / span) as f32).clamp(0.0, 1.0);
        let (top_y, bot_y) = if val_y <= zero_y {
            (val_y, zero_y)
        } else {
            (zero_y, val_y)
        };
        let h = (bot_y - top_y).max(2.0);
        let r = (bar_w * 0.25).min(5.0).min(h * 0.5);

        let mut pb = PathBuilder::fill();
        pb.move_to(point(px(x0), px(bot_y)));
        pb.line_to(point(px(x0), px(top_y + r)));
        pb.curve_to(point(px(x0 + r), px(top_y)), point(px(x0), px(top_y)));
        pb.line_to(point(px(x1 - r), px(top_y)));
        pb.curve_to(point(px(x1), px(top_y + r)), point(px(x1), px(top_y)));
        pb.line_to(point(px(x1), px(bot_y)));
        pb.close();
        if let Ok(path) = pb.build() {
            fills.push(path);
        }
    }

    PlotGeometry {
        guides,
        fills,
        strokes: Vec::new(),
        dots: Vec::new(),
    }
}

fn build_line_geometry(
    bounds: Bounds<Pixels>,
    values: &[f64],
    min_v: f64,
    max_v: f64,
) -> PlotGeometry {
    let ox = f32::from(bounds.origin.x);
    let oy = f32::from(bounds.origin.y) + 6.0;
    let bw = f32::from(bounds.size.width).max(1.0);
    let bh = (f32::from(bounds.size.height) - 10.0).max(1.0);
    let guides = build_guide_lines(ox, oy, bw, bh);

    if values.is_empty() {
        return PlotGeometry {
            guides,
            fills: Vec::new(),
            strokes: Vec::new(),
            dots: Vec::new(),
        };
    }

    let span = (max_v - min_v).max(1e-9);
    let slot_w = bw / (values.len().max(1) as f32);
    let coords: Vec<(f32, f32)> = values
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let x = ox + slot_w * (i as f32 + 0.5);
            let y = oy + bh * (1.0 - ((v - min_v) / span) as f32).clamp(0.0, 1.0);
            (x, y)
        })
        .collect();

    let mut strokes = Vec::new();
    let mut fills = Vec::new();
    if coords.len() >= 2 {
        let mut sb = PathBuilder::stroke(px(2.2));
        let mut ab = PathBuilder::fill();
        sb.move_to(point(px(coords[0].0), px(coords[0].1)));
        ab.move_to(point(px(coords[0].0), px(oy + bh)));
        ab.line_to(point(px(coords[0].0), px(coords[0].1)));
        for &(x, y) in coords.iter().skip(1) {
            sb.line_to(point(px(x), px(y)));
            ab.line_to(point(px(x), px(y)));
        }
        if let Some(&(lx, _)) = coords.last() {
            ab.line_to(point(px(lx), px(oy + bh)));
        }
        ab.close();
        if let Ok(p) = sb.build() {
            strokes.push(p);
        }
        if let Ok(p) = ab.build() {
            fills.push(p);
        }
    }

    let mut dots = Vec::with_capacity(coords.len());
    for &(cx, cy) in &coords {
        if let Some(dot) = build_circle_path(cx, cy, 3.8) {
            dots.push(dot);
        }
    }

    PlotGeometry {
        guides,
        fills,
        strokes,
        dots,
    }
}

fn build_circle_path(cx: f32, cy: f32, radius: f32) -> Option<Path<Pixels>> {
    let mut pb = PathBuilder::fill();
    let segs = 14_usize;
    for i in 0..segs {
        let ang = (i as f32 / segs as f32) * TAU;
        let pt = point(px(cx + radius * ang.cos()), px(cy + radius * ang.sin()));
        if i == 0 {
            pb.move_to(pt);
        } else {
            pb.line_to(pt);
        }
    }
    pb.close();
    pb.build().ok()
}

fn render_donut_chart(series: &[ChartPoint], agg_label: &str, cx: &App) -> AnyElement {
    let colors = cx.colors();
    let palette = chart_palette(colors.accent.color);
    let total: f64 = series.iter().map(|p| p.value).sum();
    let total_safe = if total > 0.0 { total } else { 1.0 };

    let slices: Vec<(f32, Hsla)> = series
        .iter()
        .enumerate()
        .map(|(i, pt)| {
            let frac = (pt.value / total_safe) as f32;
            (frac, palette[i % palette.len()])
        })
        .collect();
    let slices_for_canvas = slices.clone();

    let donut_ring = div()
        .relative()
        .w(px(190.0))
        .h_full()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            canvas(
                move |bounds, _w, _cx| build_donut_geometry(bounds, &slices_for_canvas),
                move |_bounds, paths, window, _cx| {
                    for (path, color) in paths {
                        window.paint_path(path, color);
                    }
                },
            )
            .size_full(),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(px(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(format_compact_number(total)),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(colors.muted)
                        .child(agg_label.to_owned()),
                ),
        );

    let legend = render_donut_legend(series, &palette, total_safe, cx);

    div()
        .size_full()
        .flex()
        .flex_row()
        .items_center()
        .min_h_0()
        .child(donut_ring)
        .child(legend)
        .into_any_element()
}

fn render_donut_legend(
    series: &[ChartPoint],
    palette: &[Hsla; 8],
    total_safe: f64,
    cx: &App,
) -> AnyElement {
    let colors = cx.colors();
    let mut legend = div()
        .id("charts-donut-legend")
        .flex_1()
        .min_w_0()
        .min_h_0()
        .overflow_y_scroll()
        .pl(px(14.0))
        .flex()
        .flex_col()
        .gap(px(6.0));

    for (idx, pt) in series.iter().enumerate() {
        let swatch = palette[idx % palette.len()];
        let pct = (pt.value / total_safe) * 100.0;
        legend = legend.child(
            div()
                .w_full()
                .px(px(8.0))
                .py(px(5.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(theme::hairline_border_color(cx))
                .bg(theme::elevated_overlay_bg(cx))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(7.0))
                        .min_w_0()
                        .child(div().w(px(9.0)).h(px(9.0)).rounded(px(999.0)).bg(swatch))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(px(11.5))
                                .text_color(colors.foreground)
                                .child(pt.label.clone()),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(11.0))
                        .text_color(colors.muted)
                        .child(format!("{} ({pct:.1}%)", format_compact_number(pt.value))),
                ),
        );
    }

    legend.into_any_element()
}

fn build_donut_geometry(
    bounds: Bounds<Pixels>,
    slices: &[(f32, Hsla)],
) -> Vec<(Path<Pixels>, Hsla)> {
    let w = f32::from(bounds.size.width);
    let h = f32::from(bounds.size.height);
    let cx = f32::from(bounds.origin.x) + w * 0.5;
    let cy = f32::from(bounds.origin.y) + h * 0.5;
    let outer_r = (w.min(h) * 0.44).max(16.0);
    let inner_r = outer_r * 0.58;

    let gap = if slices.len() > 1 { 0.025_f32 } else { 0.0 };
    let mut start_angle = -TAU * 0.25;
    let mut out = Vec::with_capacity(slices.len());

    for &(frac, color) in slices {
        let sweep = (frac * TAU - gap).max(0.02);
        let end_angle = start_angle + sweep;
        let steps = ((sweep / TAU) * 48.0).ceil().max(6.0) as usize;

        let mut pb = PathBuilder::fill();
        for i in 0..=steps {
            let a = start_angle + sweep * (i as f32 / steps as f32);
            let pt = point(px(cx + outer_r * a.cos()), px(cy + outer_r * a.sin()));
            if i == 0 {
                pb.move_to(pt);
            } else {
                pb.line_to(pt);
            }
        }
        for i in (0..=steps).rev() {
            let a = start_angle + sweep * (i as f32 / steps as f32);
            pb.line_to(point(
                px(cx + inner_r * a.cos()),
                px(cy + inner_r * a.sin()),
            ));
        }
        pb.close();
        if let Ok(path) = pb.build() {
            out.push((path, color));
        }
        start_angle = end_angle + gap;
    }
    out
}

fn format_compact_number(v: f64) -> String {
    let abs = v.abs();
    if abs >= 1_000_000_000.0 {
        format!("{:.2}B", v / 1_000_000_000.0)
    } else if abs >= 1_000_000.0 {
        format!("{:.2}M", v / 1_000_000.0)
    } else if abs >= 10_000.0 {
        format!("{:.1}K", v / 1_000.0)
    } else if (v - v.round()).abs() < 1e-6 {
        format!("{:.0}", v)
    } else {
        format!("{v:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tablepro_core::column::ColumnType;

    #[test]
    fn detects_numeric_columns_and_aggregates_cleanly() {
        let cols = vec![
            ResultColumn::new("region", ColumnType::untyped_text("text")),
            ResultColumn::new("revenue", ColumnType::untyped_text("numeric")),
        ];
        let rows = vec![
            vec![Value::Text("US".to_owned()), Value::Int(100)],
            vec![Value::Text("US".to_owned()), Value::Int(50)],
            vec![Value::Text("EU".to_owned()), Value::Float64(75.5)],
        ];

        let num_cols = detect_numeric_columns(&cols, &rows);
        assert_eq!(num_cols, vec![1]);

        let sum_pts =
            aggregate_chart_series(&rows, 0, Some(1), ChartAggregation::Sum, ChartKind::Bar);
        assert_eq!(sum_pts.len(), 2);
        assert_eq!(sum_pts[0].label, "US");
        assert!((sum_pts[0].value - 150.0).abs() < 1e-6);
        assert!((sum_pts[1].value - 75.5).abs() < 1e-6);

        let avg_pts =
            aggregate_chart_series(&rows, 0, Some(1), ChartAggregation::Avg, ChartKind::Line);
        assert!((avg_pts[0].value - 75.0).abs() < 1e-6);

        let cnt_pts =
            aggregate_chart_series(&rows, 0, Some(1), ChartAggregation::Count, ChartKind::Donut);
        assert!((cnt_pts[0].value - 2.0).abs() < 1e-6);
    }
}
