//! Deterministic Mock Data Generator workbench view.
//!
//! Inspects table columns & outbound foreign keys, allows configuring per-column
//! [`MockValueKind`] rules and NULL ratios, previews sample rows & dialect-aware
//! `INSERT` SQL, and executes batched generation asynchronously against the active session.

use std::collections::{BTreeSet, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

use dataforge_services::DatabaseEngine;
use dataforge_services::mock_generator::{
    MockColumnRule, MockGenerationPlan, MockGenerationReport, MockValueKind, SeededRng,
    build_mock_insert_batches, execute_mock_generation, infer_plan_from_table,
};
use dataforge_state::connection_window::ConnectionWindowPhase;
use herogpui::components::{
    Alert, Button, Chip, ChipLabel, ChipVariant, FieldVariant, Icon, IconName, IconSize, Input,
    InputState, InputType, PickerItem, Select, Variant,
};
use herogpui::core::{Color as HeroColor, Size as HeroSize};
use herogpui::gpui::prelude::FluentBuilder as _;
use herogpui::gpui::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, FontWeight,
    InteractiveElement, IntoElement, ParentElement, SharedString, StatefulInteractiveElement,
    Styled, Window, div, px,
};
use herogpui::theme::ActiveTheme;
use tablepro_core::ddl::TableDefinition;
use tablepro_core::meta::{ObjectInfo, ObjectKind};
use tablepro_core::value::Value;

use super::shell_state::{push_error_toast, push_success_toast};
use super::{DataForgeWorkspace, syntax_bridge, theme};

const DEFAULT_ROW_COUNT: usize = 100;
const DEFAULT_BATCH_SIZE: usize = 50;
const DEFAULT_SEED: u64 = 42;
const PREVIEW_ROW_LIMIT: usize = 6;
const PREVIEW_FK_POOL_LIMIT: usize = 25;

/// Selectable generator rule kinds mapped onto [`MockValueKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKindChoice {
    AutoIncrement,
    Uuid,
    Email,
    FullName,
    Company,
    Phone,
    IntRange,
    FloatRange,
    Bool,
    TimestampIso,
    JsonObject,
    LoremText,
    EnumValues,
    ForeignKeySample,
}

impl RuleKindChoice {
    const ALL: [Self; 14] = [
        Self::AutoIncrement,
        Self::Uuid,
        Self::Email,
        Self::FullName,
        Self::Company,
        Self::Phone,
        Self::IntRange,
        Self::FloatRange,
        Self::Bool,
        Self::TimestampIso,
        Self::JsonObject,
        Self::LoremText,
        Self::EnumValues,
        Self::ForeignKeySample,
    ];

    const fn key(self) -> &'static str {
        match self {
            Self::AutoIncrement => "auto_inc",
            Self::Uuid => "uuid",
            Self::Email => "email",
            Self::FullName => "full_name",
            Self::Company => "company",
            Self::Phone => "phone",
            Self::IntRange => "int_range",
            Self::FloatRange => "float_range",
            Self::Bool => "bool",
            Self::TimestampIso => "timestamp_iso",
            Self::JsonObject => "json_object",
            Self::LoremText => "lorem_text",
            Self::EnumValues => "enum_values",
            Self::ForeignKeySample => "fk_sample",
        }
    }

    const fn code_label(self) -> &'static str {
        match self {
            Self::AutoIncrement => "AUTO_INC",
            Self::Uuid => "UUID",
            Self::Email => "EMAIL",
            Self::FullName => "FULL_NAME",
            Self::Company => "COMPANY",
            Self::Phone => "PHONE",
            Self::IntRange => "INT_RANGE",
            Self::FloatRange => "FLOAT_RANGE",
            Self::Bool => "BOOL",
            Self::TimestampIso => "TIMESTAMP_ISO",
            Self::JsonObject => "JSON",
            Self::LoremText => "LOREM_TEXT",
            Self::EnumValues => "ENUM",
            Self::ForeignKeySample => "FK_SAMPLE",
        }
    }

    const fn from_kind(kind: &MockValueKind) -> Self {
        match kind {
            MockValueKind::AutoIncrement => Self::AutoIncrement,
            MockValueKind::Uuid => Self::Uuid,
            MockValueKind::Email => Self::Email,
            MockValueKind::FullName => Self::FullName,
            MockValueKind::Company => Self::Company,
            MockValueKind::Phone => Self::Phone,
            MockValueKind::IntRange { .. } => Self::IntRange,
            MockValueKind::FloatRange { .. } => Self::FloatRange,
            MockValueKind::Bool => Self::Bool,
            MockValueKind::TimestampIso => Self::TimestampIso,
            MockValueKind::JsonObject => Self::JsonObject,
            MockValueKind::LoremText => Self::LoremText,
            MockValueKind::EnumValues(_) => Self::EnumValues,
            MockValueKind::ForeignKeySample { .. } => Self::ForeignKeySample,
        }
    }

    fn from_key(key: &str) -> Self {
        Self::ALL
            .iter()
            .copied()
            .find(|choice| choice.key() == key)
            .unwrap_or(Self::LoremText)
    }

    const fn next(self) -> Self {
        let mut idx = 0;
        while idx < Self::ALL.len() {
            if Self::ALL[idx] as u8 == self as u8 {
                return Self::ALL[(idx + 1) % Self::ALL.len()];
            }
            idx += 1;
        }
        Self::LoremText
    }

    const fn has_param(self) -> bool {
        matches!(
            self,
            Self::IntRange | Self::FloatRange | Self::EnumValues | Self::ForeignKeySample
        )
    }

    const fn param_placeholder(self) -> &'static str {
        match self {
            Self::IntRange => "1..1000",
            Self::FloatRange => "1.0..100.0",
            Self::EnumValues => "a, b, c",
            Self::ForeignKeySample => "table.column",
            _ => "",
        }
    }

    fn into_kind(self, prev: &MockValueKind) -> MockValueKind {
        match self {
            Self::AutoIncrement => MockValueKind::AutoIncrement,
            Self::Uuid => MockValueKind::Uuid,
            Self::Email => MockValueKind::Email,
            Self::FullName => MockValueKind::FullName,
            Self::Company => MockValueKind::Company,
            Self::Phone => MockValueKind::Phone,
            Self::IntRange => match prev {
                MockValueKind::IntRange { min, max } => MockValueKind::IntRange {
                    min: *min,
                    max: *max,
                },
                _ => MockValueKind::IntRange { min: 1, max: 1000 },
            },
            Self::FloatRange => match prev {
                MockValueKind::FloatRange { min, max } => MockValueKind::FloatRange {
                    min: *min,
                    max: *max,
                },
                _ => MockValueKind::FloatRange {
                    min: 1.0,
                    max: 500.0,
                },
            },
            Self::Bool => MockValueKind::Bool,
            Self::TimestampIso => MockValueKind::TimestampIso,
            Self::JsonObject => MockValueKind::JsonObject,
            Self::LoremText => MockValueKind::LoremText,
            Self::EnumValues => match prev {
                MockValueKind::EnumValues(vals) if !vals.is_empty() => {
                    MockValueKind::EnumValues(vals.clone())
                }
                _ => MockValueKind::EnumValues(vec![
                    "active".to_owned(),
                    "pending".to_owned(),
                    "archived".to_owned(),
                ]),
            },
            Self::ForeignKeySample => match prev {
                MockValueKind::ForeignKeySample {
                    ref_table,
                    ref_column,
                } => MockValueKind::ForeignKeySample {
                    ref_table: ref_table.clone(),
                    ref_column: ref_column.clone(),
                },
                _ => MockValueKind::ForeignKeySample {
                    ref_table: String::new(),
                    ref_column: "id".to_owned(),
                },
            },
        }
    }
}

#[derive(Debug)]
struct RuleDraft {
    id: u64,
    column_name: String,
    sql_type: String,
    nullable: bool,
    null_ratio: f32,
    null_ratio_input: Entity<InputState>,
    kind: MockValueKind,
    param_input: Entity<InputState>,
}

/// Persistent UI state for the Mock Data Generator workbench view.
#[derive(Debug)]
pub struct MockGeneratorViewState {
    pub selected_table_key: Option<String>,
    schema_name: Option<String>,
    table_name: String,
    loaded_table_key: Option<String>,
    row_count_input: Entity<InputState>,
    batch_size_input: Entity<InputState>,
    seed_input: Entity<InputState>,
    row_count: usize,
    batch_size: usize,
    seed: u64,
    rules: Vec<RuleDraft>,
    preview_columns: Vec<String>,
    preview_rows: Vec<Vec<String>>,
    preview_sql: String,
    running: bool,
    last_report: Option<MockGenerationReport>,
    load_error: Option<String>,
    exec_error: Option<String>,
    next_id: u64,
}

impl MockGeneratorViewState {
    /// Creates initial Mock Data Generator state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, cx: &mut Context<DataForgeWorkspace>) -> Self {
        let row_count_input =
            cx.new(|cx| InputState::with_value(cx, DEFAULT_ROW_COUNT.to_string()));
        let batch_size_input =
            cx.new(|cx| InputState::with_value(cx, DEFAULT_BATCH_SIZE.to_string()));
        let seed_input = cx.new(|cx| InputState::with_value(cx, DEFAULT_SEED.to_string()));
        Self {
            selected_table_key: None,
            schema_name: None,
            table_name: String::new(),
            loaded_table_key: None,
            row_count_input,
            batch_size_input,
            seed_input,
            row_count: DEFAULT_ROW_COUNT,
            batch_size: DEFAULT_BATCH_SIZE,
            seed: DEFAULT_SEED,
            rules: Vec::new(),
            preview_columns: Vec::new(),
            preview_rows: Vec::new(),
            preview_sql: String::new(),
            running: false,
            last_report: None,
            load_error: None,
            exec_error: None,
            next_id: 1,
        }
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    fn make_rule_draft(&mut self, rule: MockColumnRule, cx: &mut App) -> RuleDraft {
        let id = self.alloc_id();
        let ratio_str = format!("{:.2}", rule.null_ratio);
        let param_str = format_kind_param(&rule.kind);
        RuleDraft {
            id,
            column_name: rule.column_name,
            sql_type: rule.sql_type,
            nullable: rule.nullable,
            null_ratio: rule.null_ratio,
            null_ratio_input: cx.new(|cx| InputState::with_value(cx, ratio_str)),
            kind: rule.kind,
            param_input: cx.new(|cx| InputState::with_value(cx, param_str)),
        }
    }
}

/// Renders the Mock Data Generator workbench tool view.
#[must_use]
pub fn render_mock_generator_view(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let is_connected = is_workspace_connected(ws);
    if is_connected {
        ensure_initial_table_loaded(ws, cx);
    }
    let tables = collect_target_tables(&ws.controller.cached_objects);

    div()
        .id("mock-generator-view")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme::panel_surface_bg(cx))
        .child(render_header(ws, cx))
        .child(render_toolbar(ws, &tables, is_connected, cx))
        .children(render_error_alerts(ws))
        .child(render_main_body(ws, is_connected, cx))
        .into_any_element()
}

fn is_workspace_connected(ws: &DataForgeWorkspace) -> bool {
    matches!(
        ws.controller
            .connections
            .phase(ws.controller.active_workspace),
        Some(ConnectionWindowPhase::Connected)
    )
}

fn render_header(ws: &DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let engine = ws.controller.active_database_engine();

    div()
        .min_h(px(42.0))
        .px(px(14.0))
        .py(px(6.0))
        .border_b_1()
        .border_color(theme::hairline_border_color(cx))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    Icon::new(IconName::WandSparkles)
                        .size(IconSize::Sm)
                        .color(colors.accent.color),
                )
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .flex_shrink_0()
                        .child(s.mock_generator_title),
                )
                .child(
                    Chip::new()
                        .variant(ChipVariant::Soft)
                        .size(HeroSize::Sm)
                        .color(HeroColor::Accent)
                        .child(ChipLabel::new().child(engine.as_str().to_ascii_uppercase())),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .text_size(px(11.5))
                        .text_color(colors.muted)
                        .child(s.mock_generator_desc),
                ),
        )
        .child(
            Button::new("mock-gen-close")
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

fn render_toolbar(
    ws: &DataForgeWorkspace,
    tables: &[&ObjectInfo],
    is_connected: bool,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let can_act = is_connected && !ws.mock_generator_ui.rules.is_empty();
    let can_run = can_act && !ws.mock_generator_ui.running;

    div()
        .px(px(14.0))
        .py(px(8.0))
        .border_b_1()
        .border_color(theme::hairline_border_color(cx))
        .bg(theme::muted_surface_bg(cx))
        .flex()
        .flex_wrap()
        .items_end()
        .justify_between()
        .gap(px(10.0))
        .child(render_toolbar_inputs(ws, tables, is_connected, cx))
        .child(render_toolbar_actions(ws, can_act, can_run, s, cx))
        .into_any_element()
}

fn render_toolbar_inputs(
    ws: &DataForgeWorkspace,
    tables: &[&ObjectInfo],
    is_connected: bool,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let items: Vec<PickerItem> = tables
        .iter()
        .map(|obj| {
            let key = encode_table_key(obj.schema.as_deref(), &obj.name);
            PickerItem::new(key.clone(), key)
        })
        .collect();
    let current = ws.mock_generator_ui.selected_table_key.clone();

    div()
        .min_w_0()
        .flex()
        .flex_wrap()
        .items_end()
        .gap(px(8.0))
        .child(render_table_picker_group(
            items,
            current,
            is_connected,
            s,
            cx,
        ))
        .child(render_numeric_field(
            s.mock_row_count,
            &ws.mock_generator_ui.row_count_input,
            px(88.0),
            cx.listener(|ws, val: &str, _, cx| {
                if let Ok(parsed) = val.trim().parse::<usize>() {
                    ws.mock_generator_ui.row_count = parsed;
                }
                cx.notify();
            }),
            cx,
        ))
        .child(render_numeric_field(
            s.mock_batch_size,
            &ws.mock_generator_ui.batch_size_input,
            px(82.0),
            cx.listener(|ws, val: &str, _, cx| {
                if let Ok(parsed) = val.trim().parse::<usize>() {
                    ws.mock_generator_ui.batch_size = parsed.max(1);
                }
                cx.notify();
            }),
            cx,
        ))
        .child(render_seed_field(ws, cx))
        .into_any_element()
}

fn render_table_picker_group(
    items: Vec<PickerItem>,
    current: Option<String>,
    is_connected: bool,
    s: &crate::i18n::Strings,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors();
    let empty_tables = items.is_empty();
    div()
        .flex()
        .items_end()
        .gap(px(4.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(
                    div()
                        .text_size(px(10.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.muted)
                        .child(s.tables),
                )
                .child(
                    div().w(px(205.0)).min_w(px(140.0)).child(
                        Select::new("mock-gen-table-select", items)
                            .variant(FieldVariant::Secondary)
                            .placeholder(s.structure_no_table_title)
                            .is_disabled(!is_connected || empty_tables)
                            .full_width(true)
                            .value(current.map(SharedString::from))
                            .on_selection_change(cx.listener(
                                |ws, key: &Option<SharedString>, _, cx| {
                                    if let Some(k) = key.as_ref() {
                                        let (schema, table) = decode_table_key(k.as_ref());
                                        load_table_rules(ws, schema, table, cx);
                                        cx.notify();
                                    }
                                },
                            )),
                    ),
                ),
        )
        .child(
            Button::new("mock-gen-reload-table")
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .is_icon_only(true)
                .is_disabled(!is_connected)
                .aria_label(s.refresh_label)
                .on_press(cx.listener(|ws, _, _, cx| {
                    if let Some(key) = ws.mock_generator_ui.selected_table_key.clone() {
                        let (schema, table) = decode_table_key(&key);
                        load_table_rules(ws, schema, table, cx);
                        cx.notify();
                    }
                }))
                .child(Icon::new(IconName::RefreshCw).size(IconSize::Xs)),
        )
        .into_any_element()
}

fn render_numeric_field(
    label: &'static str,
    input: &Entity<InputState>,
    width: herogpui::gpui::Pixels,
    on_change: impl Fn(&str, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let colors = cx.colors();
    div()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(
            div()
                .text_size(px(10.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.muted)
                .child(label),
        )
        .child(
            div().w(width).child(
                Input::new(input)
                    .input_type(InputType::Number)
                    .variant(FieldVariant::Secondary)
                    .full_width()
                    .on_change(on_change),
            ),
        )
        .into_any_element()
}

fn render_seed_field(ws: &DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors();
    div()
        .flex()
        .items_end()
        .gap(px(4.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(
                    div()
                        .text_size(px(10.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.muted)
                        .child(s.mock_seed),
                )
                .child(
                    div().w(px(102.0)).child(
                        Input::new(&ws.mock_generator_ui.seed_input)
                            .input_type(InputType::Number)
                            .variant(FieldVariant::Secondary)
                            .full_width()
                            .on_change(cx.listener(|ws, val: &str, _, cx| {
                                if let Ok(parsed) = val.trim().parse::<u64>() {
                                    ws.mock_generator_ui.seed = parsed;
                                }
                                cx.notify();
                            })),
                    ),
                ),
        )
        .child(
            Button::new("mock-gen-randomize-seed")
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .is_icon_only(true)
                .aria_label(s.mock_seed)
                .on_press(cx.listener(|ws, _, _, cx| {
                    let next_seed = next_random_seed(ws.mock_generator_ui.seed);
                    ws.mock_generator_ui.seed = next_seed;
                    ws.mock_generator_ui.seed_input.update(cx, |inp, cx| {
                        inp.set_value(next_seed.to_string());
                        cx.notify();
                    });
                    cx.notify();
                }))
                .child(Icon::new(IconName::Sparkles).size(IconSize::Xs)),
        )
        .into_any_element()
}

fn render_toolbar_actions(
    ws: &DataForgeWorkspace,
    can_act: bool,
    can_run: bool,
    s: &crate::i18n::Strings,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let run_label = if ws.mock_generator_ui.running {
        s.status_running_label
    } else {
        s.mock_generate_btn
    };

    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.0))
        .child(
            Button::new("mock-gen-preview-btn")
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .is_disabled(!can_act)
                .child(Icon::new(IconName::Code).size(IconSize::Xs))
                .label(s.mock_preview_btn)
                .on_press(cx.listener(|ws, _, _, cx| {
                    run_preview_generation(ws, cx);
                })),
        )
        .child(
            Button::new("mock-gen-run-btn")
                .variant(Variant::Primary)
                .size(HeroSize::Sm)
                .is_disabled(!can_run)
                .child(Icon::new(IconName::Play).size(IconSize::Xs))
                .label(run_label)
                .on_press(cx.listener(|ws, _, _, cx| {
                    run_execute_generation(ws, cx);
                })),
        )
        .into_any_element()
}

fn render_error_alerts(ws: &DataForgeWorkspace) -> Option<AnyElement> {
    let s = &ws.controller.strings;
    let err = ws
        .mock_generator_ui
        .exec_error
        .as_ref()
        .or(ws.mock_generator_ui.load_error.as_ref())?;
    Some(
        div()
            .px(px(14.0))
            .pt(px(8.0))
            .child(
                Alert::new(s.query_error_title)
                    .status(HeroColor::Danger)
                    .description(err.clone()),
            )
            .into_any_element(),
    )
}

fn render_main_body(
    ws: &DataForgeWorkspace,
    is_connected: bool,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    if !is_connected {
        return render_empty_state(s.no_active_connection, s.no_active_connection_desc, cx);
    }
    if ws.mock_generator_ui.rules.is_empty() {
        return render_empty_state(s.structure_no_table_title, s.structure_no_table_desc, cx);
    }

    div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_row()
        .child(render_rules_pane(ws, cx))
        .child(render_preview_pane(ws, cx))
        .into_any_element()
}

fn render_empty_state(title: &str, subtitle: &str, cx: &App) -> AnyElement {
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
                .max_w(px(440.0))
                .text_size(px(12.0))
                .text_color(colors.muted)
                .child(subtitle.to_owned()),
        )
        .into_any_element()
}

fn render_rules_pane(ws: &DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) -> AnyElement {
    let border = theme::hairline_border_color(cx);
    let mut rows = div()
        .id("mock-gen-rules-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col();

    for (idx, rule) in ws.mock_generator_ui.rules.iter().enumerate() {
        rows = rows.child(render_rule_row(ws, idx, rule, cx));
    }

    div()
        .id("mock-gen-rules-pane")
        .flex_1()
        .min_w(px(320.0))
        .min_h_0()
        .overflow_x_scroll()
        .border_r_1()
        .border_color(border)
        .flex()
        .flex_col()
        .child(render_rules_table_header(ws, cx))
        .child(rows)
        .into_any_element()
}

fn render_rules_table_header(ws: &DataForgeWorkspace, cx: &App) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors();
    let border = theme::hairline_border_color(cx);

    div()
        .px(px(12.0))
        .py(px(6.0))
        .border_b_1()
        .border_color(border)
        .bg(theme::muted_surface_bg(cx))
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            header_cell(s.struct_col_column, colors.muted)
                .flex_1()
                .min_w(px(110.0)),
        )
        .child(header_cell(s.mock_rule_kind, colors.muted).w(px(236.0)))
        .child(header_cell(s.mock_null_ratio, colors.muted).w(px(80.0)))
        .child(header_cell(s.mock_fk_parent, colors.muted).w(px(124.0)))
        .into_any_element()
}

fn header_cell(label: &str, color: herogpui::gpui::Hsla) -> herogpui::gpui::Div {
    div()
        .text_size(px(11.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .truncate()
        .child(label.to_owned())
}

fn render_rule_row(
    ws: &DataForgeWorkspace,
    idx: usize,
    rule: &RuleDraft,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let border = theme::hairline_border_color(cx);
    let choice = RuleKindChoice::from_kind(&rule.kind);
    let value_font = theme::value_gpui_font(&ws.controller.settings);

    let mut row = div()
        .px(px(12.0))
        .py(px(6.0))
        .border_b_1()
        .border_color(border)
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.0));

    if let Some(zebra) = theme::row_zebra_bg(idx, cx) {
        row = row.bg(zebra);
    }

    row.child(render_rule_column_cell(rule, value_font, &colors, s))
        .child(render_rule_kind_cell(rule, choice, s, cx))
        .child(render_null_ratio_cell(rule, &colors, s, cx))
        .child(render_fk_parent_cell(rule, &colors, s))
        .into_any_element()
}

fn render_rule_column_cell(
    rule: &RuleDraft,
    value_font: herogpui::gpui::Font,
    colors: &herogpui::theme::ThemeColors,
    s: &crate::i18n::Strings,
) -> AnyElement {
    let null_label = if rule.nullable {
        s.nullable_badge
    } else {
        s.not_null_badge
    };
    div()
        .flex_1()
        .min_w(px(95.0))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.foreground)
                .truncate()
                .child(rule.column_name.clone()),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .font(value_font)
                        .text_size(px(10.5))
                        .text_color(colors.muted)
                        .truncate()
                        .child(rule.sql_type.clone()),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(colors.muted)
                        .child(null_label),
                ),
        )
        .into_any_element()
}

fn render_rule_kind_cell(
    rule: &RuleDraft,
    choice: RuleKindChoice,
    s: &crate::i18n::Strings,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let rule_id = rule.id;
    let items: Vec<PickerItem> = RuleKindChoice::ALL
        .iter()
        .map(|item| PickerItem::new(item.key(), item.code_label()))
        .collect();

    div()
        .w(px(236.0))
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(
            div().w(px(124.0)).child(
                Select::new(
                    SharedString::from(format!("mock-rule-kind-{rule_id}")),
                    items,
                )
                .variant(FieldVariant::Secondary)
                .full_width(true)
                .value(Some(SharedString::from(choice.key())))
                .on_selection_change(cx.listener(
                    move |ws, key: &Option<SharedString>, _, cx| {
                        if let Some(k) = key.as_ref() {
                            apply_rule_kind_choice(ws, rule_id, RuleKindChoice::from_key(k), cx);
                        }
                    },
                )),
            ),
        )
        .child(
            Button::new(SharedString::from(format!("mock-rule-cycle-{rule_id}")))
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .is_icon_only(true)
                .aria_label(s.mock_rule_kind)
                .on_press(cx.listener(move |ws, _, _, cx| {
                    apply_rule_kind_choice(ws, rule_id, choice.next(), cx);
                }))
                .child(Icon::new(IconName::RefreshCw).size(IconSize::Xs)),
        )
        .when(choice.has_param(), |el| {
            el.child(
                div().flex_1().min_w(px(74.0)).child(
                    Input::new(&rule.param_input)
                        .variant(FieldVariant::Secondary)
                        .placeholder(choice.param_placeholder())
                        .full_width()
                        .on_change(cx.listener(move |ws, val: &str, _, cx| {
                            update_rule_param(ws, rule_id, val);
                            cx.notify();
                        })),
                ),
            )
        })
        .into_any_element()
}

fn render_null_ratio_cell(
    rule: &RuleDraft,
    colors: &herogpui::theme::ThemeColors,
    s: &crate::i18n::Strings,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let rule_id = rule.id;
    div()
        .w(px(80.0))
        .flex()
        .items_center()
        .child(if rule.nullable {
            div()
                .w(px(74.0))
                .child(
                    Input::new(&rule.null_ratio_input)
                        .variant(FieldVariant::Secondary)
                        .placeholder("0.10")
                        .full_width()
                        .on_change(cx.listener(move |ws, val: &str, _, cx| {
                            if let Some(ratio) = parse_null_ratio(val)
                                && let Some(entry) = ws
                                    .mock_generator_ui
                                    .rules
                                    .iter_mut()
                                    .find(|r| r.id == rule_id)
                            {
                                entry.null_ratio = ratio;
                            }
                            cx.notify();
                        })),
                )
                .into_any_element()
        } else {
            div()
                .text_size(px(10.5))
                .text_color(colors.muted)
                .child(s.not_null_badge)
                .into_any_element()
        })
        .into_any_element()
}

fn render_fk_parent_cell(
    rule: &RuleDraft,
    colors: &herogpui::theme::ThemeColors,
    s: &crate::i18n::Strings,
) -> AnyElement {
    let content = match &rule.kind {
        MockValueKind::ForeignKeySample {
            ref_table,
            ref_column,
        } if !ref_table.trim().is_empty() => Chip::new()
            .variant(ChipVariant::Soft)
            .size(HeroSize::Sm)
            .color(HeroColor::Accent)
            .child(ChipLabel::new().child(format!(
                "{}: {}.{}",
                s.mock_fk_parent, ref_table, ref_column
            )))
            .into_any_element(),
        _ => div()
            .text_size(px(11.0))
            .text_color(colors.muted)
            .child("—")
            .into_any_element(),
    };

    div()
        .w(px(124.0))
        .min_w_0()
        .flex()
        .items_center()
        .child(content)
        .into_any_element()
}

fn render_preview_pane(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let border = theme::hairline_border_color(cx);

    div()
        .w(px(360.0))
        .min_w(px(260.0))
        .max_w(px(440.0))
        .min_h_0()
        .flex()
        .flex_col()
        .child(render_execution_status_bar(ws, cx))
        .when(!ws.mock_generator_ui.preview_rows.is_empty(), |el| {
            el.child(render_sample_rows_section(ws, cx))
                .child(div().h(px(1.0)).w_full().bg(border))
        })
        .child(render_sql_preview_section(ws, cx))
        .into_any_element()
}

fn render_execution_status_bar(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors();
    let border = theme::hairline_border_color(cx);
    let value_font = theme::value_gpui_font(&ws.controller.settings);

    let (chip_label, chip_color, inserted_count, batches_count, elapsed_ms) =
        if ws.mock_generator_ui.running {
            (s.status_running_label, HeroColor::Accent, 0, 0, None)
        } else if let Some(report) = &ws.mock_generator_ui.last_report {
            (
                s.status_completed,
                HeroColor::Success,
                report.inserted_rows,
                report.batches_executed,
                Some(report.elapsed_ms),
            )
        } else {
            (s.mock_preview_btn, HeroColor::Default, 0, 0, None)
        };

    div()
        .px(px(12.0))
        .py(px(8.0))
        .border_b_1()
        .border_color(border)
        .bg(theme::muted_surface_bg(cx))
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(6.0))
        .child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .size(HeroSize::Sm)
                .color(chip_color)
                .child(ChipLabel::new().child(chip_label)),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(10.0))
                .font(value_font)
                .text_size(px(11.0))
                .text_color(colors.foreground)
                .child(format!("{}: {inserted_count}", s.mock_inserted_rows))
                .when(batches_count > 0, |el| {
                    el.child(
                        div()
                            .text_color(colors.muted)
                            .child(format!("{}: {batches_count}", s.import_statements_count)),
                    )
                })
                .when_some(elapsed_ms, |el, ms| {
                    el.child(
                        div()
                            .text_color(colors.muted)
                            .child(format!("{}: {ms} ms", s.summary_elapsed)),
                    )
                }),
        )
        .into_any_element()
}

fn render_sample_rows_section(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors();
    let border = theme::hairline_border_color(cx);
    let value_font = theme::value_gpui_font(&ws.controller.settings);
    let cols = &ws.mock_generator_ui.preview_columns;
    let rows = &ws.mock_generator_ui.preview_rows;

    let col_count = u16::try_from(cols.len().max(1)).map_or(1000.0, f32::from);
    let table_min_w = px(col_count * 120.0 + 16.0);

    let mut header = div()
        .min_w(table_min_w)
        .w_full()
        .px(px(8.0))
        .h(px(26.0))
        .border_b_1()
        .border_color(border)
        .bg(theme::muted_surface_bg(cx))
        .flex()
        .items_center()
        .flex_shrink_0();
    for col in cols {
        header = header.child(
            div()
                .w(px(120.0))
                .flex_shrink_0()
                .px(px(6.0))
                .text_size(px(10.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.foreground)
                .truncate()
                .child(col.clone()),
        );
    }

    let mut body = div()
        .id("mock-gen-sample-rows-y")
        .min_w(table_min_w)
        .w_full()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col();

    for (r_idx, row_vals) in rows.iter().enumerate() {
        let mut row_el = div()
            .min_w(table_min_w)
            .w_full()
            .px(px(8.0))
            .min_h(px(24.0))
            .border_b_1()
            .border_color(border)
            .flex()
            .items_center()
            .flex_shrink_0();
        if let Some(zebra) = theme::row_zebra_bg(r_idx, cx) {
            row_el = row_el.bg(zebra);
        }
        for cell in row_vals {
            row_el = row_el.child(
                div()
                    .w(px(120.0))
                    .flex_shrink_0()
                    .px(px(6.0))
                    .font(value_font.clone())
                    .text_size(px(11.0))
                    .text_color(colors.foreground)
                    .truncate()
                    .child(cell.clone()),
            );
        }
        body = body.child(row_el);
    }

    div()
        .h(px(185.0))
        .min_h(px(120.0))
        .flex()
        .flex_col()
        .child(
            div()
                .px(px(12.0))
                .py(px(6.0))
                .border_b_1()
                .border_color(border)
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.foreground)
                .child(format!("{} ({})", s.import_preview_rows, rows.len())),
        )
        .child(
            div()
                .id("mock-gen-sample-rows-x")
                .flex_1()
                .min_h_0()
                .overflow_x_scroll()
                .flex()
                .flex_col()
                .child(header)
                .child(body),
        )
        .into_any_element()
}

fn render_sql_preview_section(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let border = theme::hairline_border_color(cx);
    let editor_font = theme::editor_gpui_font(&ws.controller.settings);
    let grammar = ws.active_grammar();
    let sql = ws.mock_generator_ui.preview_sql.clone();
    let copy_sql = sql.clone();

    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(
            div()
                .px(px(12.0))
                .py(px(6.0))
                .border_b_1()
                .border_color(border)
                .flex()
                .items_center()
                .justify_between()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(s.schema_preview_sql),
                )
                .child(
                    Button::new("mock-gen-copy-sql")
                        .variant(Variant::Tertiary)
                        .size(HeroSize::Sm)
                        .is_disabled(sql.trim().is_empty())
                        .child(Icon::new(IconName::Copy).size(IconSize::Xs))
                        .label(s.copy_label)
                        .on_press(cx.listener(move |ws, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_sql.clone()));
                            let msg = ws.controller.strings.copied;
                            push_success_toast(msg, None, cx);
                        })),
                ),
        )
        .child(
            div()
                .id("mock-gen-sql-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(12.0))
                .bg(ws.content_palette.editor_background)
                .font(editor_font)
                .text_size(px(ws.controller.settings.editor_font.size.max(11.0)))
                .line_height(px(19.0))
                .text_color(ws.content_palette.editor_text)
                .child(if sql.trim().is_empty() {
                    div()
                        .text_color(colors.muted)
                        .child(s.mock_generator_desc)
                        .into_any_element()
                } else {
                    syntax_bridge::styled_sql_block(sql, grammar, &ws.content_palette)
                        .into_any_element()
                }),
        )
        .into_any_element()
}

fn ensure_initial_table_loaded(ws: &mut DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) {
    if ws.mock_generator_ui.loaded_table_key.is_some() {
        return;
    }
    let target = {
        let tables = collect_target_tables(&ws.controller.cached_objects);
        if tables.is_empty() {
            return;
        }
        ws.controller
            .active_table
            .as_ref()
            .filter(|t| {
                tables
                    .iter()
                    .any(|o| o.schema == t.schema && o.name == t.name)
            })
            .map(|t| (t.schema.clone(), t.name.clone()))
            .or_else(|| tables.first().map(|o| (o.schema.clone(), o.name.clone())))
    };

    if let Some((schema, table)) = target {
        load_table_rules(ws, schema, table, cx);
    }
}

fn load_table_rules(
    ws: &mut DataForgeWorkspace,
    schema: Option<String>,
    table: String,
    cx: &mut Context<DataForgeWorkspace>,
) {
    let key = encode_table_key(schema.as_deref(), &table);
    ws.mock_generator_ui.selected_table_key = Some(key.clone());
    ws.mock_generator_ui.loaded_table_key = Some(key);
    ws.mock_generator_ui.schema_name = schema.clone();
    ws.mock_generator_ui.table_name = table.clone();
    ws.mock_generator_ui.exec_error = None;
    ws.mock_generator_ui.preview_columns.clear();
    ws.mock_generator_ui.preview_rows.clear();
    ws.mock_generator_ui.preview_sql.clear();

    match ws
        .controller
        .fetch_table_definition_blocking(schema.as_deref(), &table)
    {
        Ok(def) => {
            ws.mock_generator_ui.load_error = None;
            populate_rules_from_definition(&mut ws.mock_generator_ui, &def, cx);
        }
        Err(err) => {
            ws.mock_generator_ui.rules.clear();
            ws.mock_generator_ui.load_error = Some(err.to_string());
        }
    }
}

fn populate_rules_from_definition(
    state: &mut MockGeneratorViewState,
    def: &TableDefinition,
    cx: &mut App,
) {
    let plan = infer_plan_from_table(def, state.row_count, state.batch_size, state.seed);
    state.rules.clear();
    for rule in plan.rules {
        let draft = state.make_rule_draft(rule, cx);
        state.rules.push(draft);
    }
}

fn apply_rule_kind_choice(
    ws: &mut DataForgeWorkspace,
    rule_id: u64,
    choice: RuleKindChoice,
    cx: &mut Context<DataForgeWorkspace>,
) {
    let Some(rule) = ws
        .mock_generator_ui
        .rules
        .iter_mut()
        .find(|r| r.id == rule_id)
    else {
        return;
    };
    let next_kind = choice.into_kind(&rule.kind);
    let param_str = format_kind_param(&next_kind);
    rule.kind = next_kind;
    rule.param_input.update(cx, |inp, cx| {
        inp.set_value(param_str);
        cx.notify();
    });
    cx.notify();
}

fn update_rule_param(ws: &mut DataForgeWorkspace, rule_id: u64, raw: &str) {
    let Some(rule) = ws
        .mock_generator_ui
        .rules
        .iter_mut()
        .find(|r| r.id == rule_id)
    else {
        return;
    };
    match &mut rule.kind {
        MockValueKind::IntRange { min, max } => {
            if let Some((lo, hi)) = parse_int_range(raw) {
                *min = lo;
                *max = hi;
            }
        }
        MockValueKind::FloatRange { min, max } => {
            if let Some((lo, hi)) = parse_float_range(raw) {
                *min = lo;
                *max = hi;
            }
        }
        MockValueKind::EnumValues(vals) => {
            let parsed = parse_enum_values(raw);
            if !parsed.is_empty() {
                *vals = parsed;
            }
        }
        MockValueKind::ForeignKeySample {
            ref_table,
            ref_column,
        } => {
            if let Some((tbl, col)) = parse_fk_target(raw) {
                *ref_table = tbl;
                *ref_column = col;
            }
        }
        _ => {}
    }
}

fn build_plan_from_state(
    state: &MockGeneratorViewState,
    row_count_override: Option<usize>,
) -> MockGenerationPlan {
    let rules = state
        .rules
        .iter()
        .map(|r| MockColumnRule {
            column_name: r.column_name.clone(),
            sql_type: r.sql_type.clone(),
            nullable: r.nullable,
            null_ratio: r.null_ratio.clamp(0.0, 1.0),
            kind: r.kind.clone(),
        })
        .collect();
    MockGenerationPlan {
        schema: state.schema_name.clone(),
        table: state.table_name.clone(),
        row_count: row_count_override.unwrap_or(state.row_count),
        batch_size: state.batch_size.max(1),
        seed: state.seed,
        rules,
    }
}

fn run_preview_generation(ws: &mut DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) {
    ws.mock_generator_ui.exec_error = None;
    let engine = ws.controller.active_database_engine();
    let preview_count = ws.mock_generator_ui.row_count.clamp(1, PREVIEW_ROW_LIMIT);
    let plan = build_plan_from_state(&ws.mock_generator_ui, Some(preview_count));

    let fk_pools = match fetch_preview_fk_pools(ws, engine, &plan) {
        Ok(pools) => pools,
        Err(err) => {
            ws.mock_generator_ui.exec_error = Some(err);
            cx.notify();
            return;
        }
    };

    match build_mock_insert_batches(engine, &plan, &fk_pools) {
        Ok(batches) => {
            let joined = batches.join("\n\n");
            let active_cols: Vec<String> = plan
                .rules
                .iter()
                .filter(|r| !matches!(r.kind, MockValueKind::AutoIncrement))
                .map(|r| r.column_name.clone())
                .collect();
            let sample_rows = parse_preview_rows_from_sql(&batches, active_cols.len());
            ws.mock_generator_ui.preview_columns = active_cols;
            ws.mock_generator_ui.preview_rows = sample_rows;
            ws.mock_generator_ui.preview_sql = joined;
        }
        Err(err) => {
            ws.mock_generator_ui.exec_error = Some(err.to_string());
        }
    }
    cx.notify();
}

fn fetch_preview_fk_pools(
    ws: &DataForgeWorkspace,
    engine: DatabaseEngine,
    plan: &MockGenerationPlan,
) -> Result<HashMap<(String, String), Vec<Value>>, String> {
    let mut needed = BTreeSet::new();
    for rule in &plan.rules {
        if let MockValueKind::ForeignKeySample {
            ref_table,
            ref_column,
        } = &rule.kind
        {
            let always_null = rule.nullable && rule.null_ratio >= 1.0;
            if !always_null {
                needed.insert((ref_table.clone(), ref_column.clone()));
            }
        }
    }

    let mut pools = HashMap::with_capacity(needed.len());
    for (ref_table, ref_column) in needed {
        let qualified = match ref_table.split_once('.') {
            Some((s, t)) => engine.qualify_table(Some(s), t),
            None => engine.qualify_table(plan.schema.as_deref(), &ref_table),
        };
        let qcol = engine.quote_identifier(&ref_column);
        let sql = match engine {
            DatabaseEngine::Mssql => format!(
                "SELECT DISTINCT TOP {PREVIEW_FK_POOL_LIMIT} {qcol} FROM {qualified} WHERE {qcol} IS NOT NULL;"
            ),
            _ => format!(
                "SELECT DISTINCT {qcol} FROM {qualified} WHERE {qcol} IS NOT NULL LIMIT {PREVIEW_FK_POOL_LIMIT};"
            ),
        };
        let (_, rows) = ws
            .controller
            .execute_query_capture_blocking(&sql)
            .map_err(|e| e.to_string())?;
        let vals: Vec<Value> = rows
            .into_iter()
            .filter_map(|mut r| (!r.is_empty()).then(|| r.remove(0)))
            .filter(|v| !v.is_null())
            .collect();
        pools.insert((ref_table, ref_column), vals);
    }
    Ok(pools)
}

fn run_execute_generation(ws: &mut DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) {
    if ws.mock_generator_ui.running {
        return;
    }
    let Some(services) = ws.controller.services.clone() else {
        ws.mock_generator_ui.exec_error =
            Some(ws.controller.strings.no_active_connection.to_owned());
        cx.notify();
        return;
    };

    let plan = build_plan_from_state(&ws.mock_generator_ui, None);
    let workspace_id = ws.controller.active_workspace;
    let no_session_msg = ws.controller.strings.no_active_session_err;
    let toast_title = ws.controller.strings.mock_generator_title;
    let toast_rows_label = ws.controller.strings.mock_inserted_rows;
    let toast_err_title = ws.controller.strings.failed;

    ws.mock_generator_ui.running = true;
    ws.mock_generator_ui.exec_error = None;
    cx.notify();

    let (tx, rx) = async_channel::bounded::<Result<MockGenerationReport, String>>(1);
    let runtime = services.runtime.clone();
    runtime.spawn(async move {
        let outcome = async {
            let session = {
                let guard = services.sessions.read().await;
                guard
                    .session(workspace_id)
                    .ok_or_else(|| no_session_msg.to_owned())?
            };
            execute_mock_generation(&session, &plan)
                .await
                .map_err(|err| err.to_string())
        }
        .await;
        let _ = tx.send(outcome).await;
    });

    cx.spawn(async move |this, cx| {
        if let Ok(result) = rx.recv().await {
            let _ = this.update(cx, |ws, cx| {
                ws.mock_generator_ui.running = false;
                match result {
                    Ok(report) => {
                        let summary = format!("{toast_rows_label}: {}", report.inserted_rows);
                        if !report.sample_sql.is_empty() {
                            ws.mock_generator_ui.preview_sql = report.sample_sql.join("\n\n");
                        }
                        ws.mock_generator_ui.last_report = Some(report);
                        push_success_toast(toast_title, Some(summary.into()), cx);
                    }
                    Err(err) => {
                        ws.mock_generator_ui.exec_error = Some(err.clone());
                        push_error_toast(toast_err_title, Some(err.into()), cx);
                    }
                }
                cx.notify();
            });
        }
    })
    .detach();
}

fn collect_target_tables(objects: &[ObjectInfo]) -> Vec<&ObjectInfo> {
    objects
        .iter()
        .filter(|obj| matches!(obj.kind, ObjectKind::Table | ObjectKind::PartitionedTable))
        .collect()
}

fn encode_table_key(schema: Option<&str>, table: &str) -> String {
    match schema.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => format!("{s}.{table}"),
        None => table.to_owned(),
    }
}

fn decode_table_key(key: &str) -> (Option<String>, String) {
    match key.split_once('.') {
        Some((s, t)) => (Some(s.to_owned()), t.to_owned()),
        None => (None, key.to_owned()),
    }
}

fn format_kind_param(kind: &MockValueKind) -> String {
    match kind {
        MockValueKind::IntRange { min, max } => format!("{min}..{max}"),
        MockValueKind::FloatRange { min, max } => format!("{min:.2}..{max:.2}"),
        MockValueKind::EnumValues(vals) => vals.join(", "),
        MockValueKind::ForeignKeySample {
            ref_table,
            ref_column,
        } => {
            if ref_table.is_empty() {
                String::new()
            } else {
                format!("{ref_table}.{ref_column}")
            }
        }
        _ => String::new(),
    }
}

fn parse_null_ratio(raw: &str) -> Option<f32> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(pct) = trimmed.strip_suffix('%') {
        let val = pct.trim().parse::<f32>().ok()?;
        return Some((val / 100.0).clamp(0.0, 1.0));
    }
    let val = trimmed.parse::<f32>().ok()?;
    if val > 1.0 && val <= 100.0 {
        Some((val / 100.0).clamp(0.0, 1.0))
    } else {
        Some(val.clamp(0.0, 1.0))
    }
}

fn parse_int_range(raw: &str) -> Option<(i64, i64)> {
    let (a, b) = raw.split_once("..").or_else(|| raw.split_once(','))?;
    let min = a.trim().parse::<i64>().ok()?;
    let max = b.trim().parse::<i64>().ok()?;
    Some((min.min(max), min.max(max)))
}

fn parse_float_range(raw: &str) -> Option<(f64, f64)> {
    let (a, b) = raw.split_once("..").or_else(|| raw.split_once(','))?;
    let min = a.trim().parse::<f64>().ok()?;
    let max = b.trim().parse::<f64>().ok()?;
    if !min.is_finite() || !max.is_finite() {
        return None;
    }
    Some((min.min(max), min.max(max)))
}

fn parse_enum_values(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_fk_target(raw: &str) -> Option<(String, String)> {
    let trimmed = raw.trim();
    let (tbl, col) = trimmed.rsplit_once('.')?;
    let t = tbl.trim();
    let c = col.trim();
    if t.is_empty() || c.is_empty() {
        return None;
    }
    Some((t.to_owned(), c.to_owned()))
}

fn next_random_seed(current: u64) -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0_u64, |d| u64::try_from(d.as_nanos()).unwrap_or(0));
    let mut rng = SeededRng::new(current ^ nanos);
    rng.next_u64() % 1_000_000
}

fn parse_preview_rows_from_sql(batches: &[String], col_count: usize) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    if col_count == 0 {
        return out;
    }
    for stmt in batches {
        for line in stmt.lines() {
            let trimmed = line.trim().trim_end_matches([',', ';']);
            if !trimmed.starts_with('(') || !trimmed.ends_with(')') {
                continue;
            }
            let inner = &trimmed[1..trimmed.len() - 1];
            let cells = split_sql_tuple_cells(inner);
            if cells.len() == col_count {
                out.push(cells);
                if out.len() >= PREVIEW_ROW_LIMIT {
                    return out;
                }
            }
        }
    }
    out
}

fn split_sql_tuple_cells(tuple_inner: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut chars = tuple_inner.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_quote {
            current.push(ch);
            if ch == '\'' {
                if chars.peek() == Some(&'\'') {
                    if let Some(escaped) = chars.next() {
                        current.push(escaped);
                    }
                } else {
                    in_quote = false;
                }
            }
        } else if ch == '\'' {
            in_quote = true;
            current.push(ch);
        } else if ch == ',' {
            cells.push(current.trim().to_owned());
            current.clear();
        } else {
            current.push(ch);
        }
    }
    if !current.trim().is_empty() || !cells.is_empty() {
        cells.push(current.trim().to_owned());
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ranges_enums_and_null_ratios() {
        assert_eq!(parse_int_range("10..50"), Some((10, 50)));
        assert_eq!(parse_int_range("90..10"), Some((10, 90)));
        assert_eq!(parse_float_range("0.5..9.5"), Some((0.5, 9.5)));
        assert_eq!(
            parse_enum_values("admin, editor , viewer "),
            vec!["admin", "editor", "viewer"]
        );
        assert_eq!(
            parse_fk_target("public.users.id"),
            Some(("public.users".to_owned(), "id".to_owned()))
        );
        assert_eq!(parse_null_ratio("0.25"), Some(0.25));
        assert_eq!(parse_null_ratio("15%"), Some(0.15));
        assert_eq!(parse_null_ratio("25"), Some(0.25));
    }

    #[test]
    fn parses_preview_rows_respecting_quoted_commas() {
        let sql = vec![
            "INSERT INTO \"users\" (\"name\", \"bio\") VALUES\n  ('O''Connor, Liam', 'Hello, world'),\n  ('Maya Patel', NULL);".to_owned(),
        ];
        let rows = parse_preview_rows_from_sql(&sql, 2);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["'O''Connor, Liam'", "'Hello, world'"]);
        assert_eq!(rows[1], vec!["'Maya Patel'", "NULL"]);
    }

    #[test]
    fn rule_kind_choice_round_trips_and_cycles() {
        for choice in RuleKindChoice::ALL {
            let kind = choice.into_kind(&MockValueKind::LoremText);
            assert_eq!(RuleKindChoice::from_kind(&kind), choice);
            assert_eq!(RuleKindChoice::from_key(choice.key()), choice);
        }
        assert_eq!(RuleKindChoice::AutoIncrement.next(), RuleKindChoice::Uuid);
    }
}
