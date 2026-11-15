//! Visual Schema & DDL Designer for creating and altering tables, indexes, and foreign keys.

use herogpui::components::{
    Alert, Button, Checkbox, Chip, ChipLabel, ChipVariant, FieldVariant, Icon, IconName, IconSize,
    Input, InputState, PickerItem, Select, Separator, Switch, TabItem, Tabs, TabsSize, TabsVariant,
    Variant,
};
use herogpui::core::{Color as HeroColor, Size as HeroSize};
use herogpui::gpui::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, FontWeight,
    InteractiveElement, IntoElement, ParentElement, SharedString, StatefulInteractiveElement,
    Styled, Window, div, px,
};
use herogpui::theme::ActiveTheme;
use tableminus_services::{
    AlterTableOp, ColumnSpec, CreateTableSpec, DatabaseEngine, ForeignKeySpec, IndexSpec,
    build_alter_table_sql, build_create_table_sql, split_sql_script_statements,
};
use tablepro_core::column::ColumnDefault;
use tablepro_core::ddl::TableDefinition;
use tablepro_core::meta::{ObjectInfo, ObjectKind};

use super::results_pane::format_core_value;
use super::shell_state::{push_error_toast, push_success_toast};
use super::{TableMinusWorkspace, syntax_bridge, theme};

/// Active designer mode (new table creation vs altering an existing table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SchemaDesignerMode {
    #[default]
    CreateTable,
    AlterTable,
}

impl SchemaDesignerMode {
    const fn key(self) -> &'static str {
        match self {
            Self::CreateTable => "create",
            Self::AlterTable => "alter",
        }
    }

    fn from_key(key: &str) -> Self {
        match key {
            "alter" => Self::AlterTable,
            _ => Self::CreateTable,
        }
    }
}

/// Guarded destructive action waiting for explicit second confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestructiveConfirm {
    Truncate,
    Drop,
}

#[derive(Debug)]
struct ColumnDraft {
    id: u64,
    original_name: Option<String>,
    name_input: Entity<InputState>,
    type_input: Entity<InputState>,
    default_input: Entity<InputState>,
    nullable: bool,
    is_primary_key: bool,
    is_unique: bool,
    auto_increment: bool,
    marked_drop: bool,
}

#[derive(Debug)]
struct IndexDraft {
    id: u64,
    from_existing: bool,
    marked_drop: bool,
    name_input: Entity<InputState>,
    columns_input: Entity<InputState>,
    unique: bool,
}

#[derive(Debug)]
struct FkDraft {
    id: u64,
    from_existing: bool,
    marked_drop: bool,
    name_input: Entity<InputState>,
    column_input: Entity<InputState>,
    ref_table_input: Entity<InputState>,
    ref_column_input: Entity<InputState>,
    on_delete: Option<String>,
}

/// Persistent UI state for the Visual Schema Designer workbench tool.
#[derive(Debug)]
pub struct SchemaDesignerState {
    pub table_name: String,
    schema_name: Option<String>,
    mode: SchemaDesignerMode,
    table_name_input: Entity<InputState>,
    if_not_exists: bool,
    columns: Vec<ColumnDraft>,
    original_columns: Vec<ColumnSpec>,
    indexes: Vec<IndexDraft>,
    foreign_keys: Vec<FkDraft>,
    loaded_alter_key: Option<String>,
    alter_load_error: Option<String>,
    confirm_action: Option<DestructiveConfirm>,
    last_feedback: Option<(bool, String)>,
    next_id: u64,
}

impl SchemaDesignerState {
    /// Creates initial Schema Designer state owned by [`TableMinusWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, cx: &mut Context<TableMinusWorkspace>) -> Self {
        let table_name = "new_table".to_owned();
        let table_name_input = cx.new(|cx| InputState::with_value(cx, table_name.clone()));
        let mut state = Self {
            table_name,
            schema_name: None,
            mode: SchemaDesignerMode::CreateTable,
            table_name_input,
            if_not_exists: true,
            columns: Vec::new(),
            original_columns: Vec::new(),
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            loaded_alter_key: None,
            alter_load_error: None,
            confirm_action: None,
            last_feedback: None,
            next_id: 1,
        };
        let pk_col = state.make_column_draft(
            ColumnSpec {
                name: "id".to_owned(),
                data_type: "INTEGER".to_owned(),
                nullable: false,
                default_expr: None,
                is_primary_key: true,
                is_unique: false,
                auto_increment: true,
            },
            None,
            cx,
        );
        state.columns.push(pk_col);
        state
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    fn make_column_draft(
        &mut self,
        spec: ColumnSpec,
        original_name: Option<String>,
        cx: &mut App,
    ) -> ColumnDraft {
        let id = self.alloc_id();
        let name_val = spec.name;
        let type_val = spec.data_type;
        let def_val = spec.default_expr.unwrap_or_default();
        ColumnDraft {
            id,
            original_name,
            name_input: cx.new(|cx| InputState::with_value(cx, name_val)),
            type_input: cx.new(|cx| InputState::with_value(cx, type_val)),
            default_input: cx.new(|cx| InputState::with_value(cx, def_val)),
            nullable: spec.nullable,
            is_primary_key: spec.is_primary_key,
            is_unique: spec.is_unique,
            auto_increment: spec.auto_increment,
            marked_drop: false,
        }
    }

    fn make_index_draft(
        &mut self,
        spec: IndexSpec,
        from_existing: bool,
        cx: &mut App,
    ) -> IndexDraft {
        let id = self.alloc_id();
        let name_val = spec.name;
        let cols_val = spec.columns.join(", ");
        IndexDraft {
            id,
            from_existing,
            marked_drop: false,
            name_input: cx.new(|cx| InputState::with_value(cx, name_val)),
            columns_input: cx.new(|cx| InputState::with_value(cx, cols_val)),
            unique: spec.unique,
        }
    }

    fn make_fk_draft(
        &mut self,
        spec: ForeignKeySpec,
        from_existing: bool,
        cx: &mut App,
    ) -> FkDraft {
        let id = self.alloc_id();
        let name_val = spec.name;
        let col_val = spec.column;
        let ref_t = spec.ref_table;
        let ref_c = spec.ref_column;
        FkDraft {
            id,
            from_existing,
            marked_drop: false,
            name_input: cx.new(|cx| InputState::with_value(cx, name_val)),
            column_input: cx.new(|cx| InputState::with_value(cx, col_val)),
            ref_table_input: cx.new(|cx| InputState::with_value(cx, ref_t)),
            ref_column_input: cx.new(|cx| InputState::with_value(cx, ref_c)),
            on_delete: spec.on_delete,
        }
    }
}

/// Renders the Visual Schema Designer workbench tool view.
#[must_use]
pub fn render_schema_designer(
    ws: &mut TableMinusWorkspace,
    _window: &mut Window,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    maybe_auto_load_alter_target(ws, cx);
    let engine = ws.controller.active_database_engine();
    let preview_stmts = compute_preview_statements(ws, engine, cx);
    let preview_sql = preview_stmts.join("\n");

    div()
        .id("schema-designer-view")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme::panel_surface_bg(cx))
        .child(render_header(ws, cx))
        .child(render_toolbar(ws, cx))
        .children(render_confirm_banner(ws, cx))
        .children(render_status_alert(ws, cx))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_row()
                .child(render_editor_column(ws, cx))
                .child(render_preview_column(ws, preview_sql, preview_stmts, cx)),
        )
        .into_any_element()
}

fn render_header(ws: &TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let engine = ws.controller.active_database_engine();

    div()
        .h(px(42.0))
        .px(px(14.0))
        .border_b_1()
        .border_color(theme::hairline_border_color(cx))
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    Icon::new(IconName::Layers)
                        .size(IconSize::Sm)
                        .color(colors.accent.color),
                )
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(s.schema_designer),
                )
                .child(
                    Chip::new()
                        .variant(ChipVariant::Soft)
                        .size(HeroSize::Sm)
                        .color(HeroColor::Accent)
                        .child(ChipLabel::new().child(engine.as_str().to_ascii_uppercase())),
                ),
        )
        .child(
            Button::new("schema-designer-close")
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

fn render_toolbar(ws: &TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) -> AnyElement {
    let s = &ws.controller.strings;
    let mode = ws.schema_designer_ui.mode;
    let selected_key = SharedString::from(mode.key());
    let mode_items = vec![
        TabItem::new("create", s.schema_create_table),
        TabItem::new("alter", s.schema_alter_table),
    ];
    let entity = cx.entity().clone();
    let mode_tabs = Tabs::new("schema-designer-mode", mode_items, selected_key.clone())
        .selected_key(selected_key)
        .variant(TabsVariant::Secondary)
        .size(TabsSize::Sm)
        .on_selection_change(move |key, _w, cx| {
            let next = SchemaDesignerMode::from_key(key.as_ref());
            entity.update(cx, |ws, cx| {
                switch_designer_mode(ws, next, cx);
                cx.notify();
            });
        });

    let target_control = if mode == SchemaDesignerMode::CreateTable {
        render_create_table_controls(ws, cx)
    } else {
        render_alter_table_controls(ws, cx)
    };

    div()
        .px(px(14.0))
        .py(px(8.0))
        .border_b_1()
        .border_color(theme::hairline_border_color(cx))
        .bg(theme::muted_surface_bg(cx))
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(10.0))
                .child(mode_tabs)
                .child(target_control),
        )
        .child(render_destructive_buttons(ws, cx))
        .into_any_element()
}

fn render_create_table_controls(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    div()
        .min_w_0()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(10.0))
        .child(
            div().w(px(190.0)).min_w(px(130.0)).child(
                Input::new(&ws.schema_designer_ui.table_name_input)
                    .variant(FieldVariant::Secondary)
                    .placeholder("table_name")
                    .full_width()
                    .on_change(cx.listener(|ws, val: &str, _, cx| {
                        ws.schema_designer_ui.table_name = val.to_owned();
                        cx.notify();
                    })),
            ),
        )
        .child(
            Checkbox::new("schema-if-not-exists")
                .is_selected(ws.schema_designer_ui.if_not_exists)
                .label("IF NOT EXISTS")
                .on_change(cx.listener(|ws, &checked: &bool, _, cx| {
                    ws.schema_designer_ui.if_not_exists = checked;
                    cx.notify();
                })),
        )
        .into_any_element()
}

fn render_alter_table_controls(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let tables = editable_tables(&ws.controller.cached_objects);
    let items: Vec<PickerItem> = tables
        .iter()
        .map(|obj| {
            let key = encode_table_key(obj.schema.as_deref(), &obj.name);
            PickerItem::new(key.clone(), key)
        })
        .collect();
    let current_key = ws.schema_designer_ui.loaded_alter_key.clone().or_else(|| {
        tables
            .first()
            .map(|obj| encode_table_key(obj.schema.as_deref(), &obj.name))
    });
    let s = &ws.controller.strings;

    div()
        .min_w_0()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.0))
        .child(
            div().w(px(200.0)).min_w(px(130.0)).child(
                Select::new("schema-alter-table-select", items)
                    .variant(FieldVariant::Secondary)
                    .placeholder(s.structure_no_table_title)
                    .full_width(true)
                    .value(current_key.map(SharedString::from))
                    .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _, cx| {
                        if let Some(k) = key.as_ref() {
                            let (schema, table) = decode_table_key(k.as_ref());
                            load_table_into_alter_mode(ws, schema, table, cx);
                            cx.notify();
                        }
                    })),
            ),
        )
        .child(
            Button::new("schema-alter-reload")
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .label(s.refresh_label)
                .on_press(cx.listener(|ws, _, _, cx| {
                    let schema = ws.schema_designer_ui.schema_name.clone();
                    let table = ws.schema_designer_ui.table_name.clone();
                    if !table.trim().is_empty() {
                        load_table_into_alter_mode(ws, schema, table, cx);
                        cx.notify();
                    }
                })),
        )
        .into_any_element()
}

fn render_destructive_buttons(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    if ws.schema_designer_ui.mode != SchemaDesignerMode::AlterTable {
        return div().into_any_element();
    }
    let has_table = !ws.schema_designer_ui.table_name.trim().is_empty();
    let armed = ws.schema_designer_ui.confirm_action;

    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.0))
        .child(
            Button::new("schema-truncate-btn")
                .variant(if armed == Some(DestructiveConfirm::Truncate) {
                    Variant::Danger
                } else {
                    Variant::DangerSoft
                })
                .size(HeroSize::Sm)
                .is_disabled(!has_table)
                .label(s.schema_truncate_table)
                .on_press(cx.listener(|ws, _, _, cx| {
                    handle_destructive_button(ws, DestructiveConfirm::Truncate, cx);
                })),
        )
        .child(
            Button::new("schema-drop-btn")
                .variant(if armed == Some(DestructiveConfirm::Drop) {
                    Variant::Danger
                } else {
                    Variant::DangerSoft
                })
                .size(HeroSize::Sm)
                .is_disabled(!has_table)
                .label(s.schema_drop_table)
                .on_press(cx.listener(|ws, _, _, cx| {
                    handle_destructive_button(ws, DestructiveConfirm::Drop, cx);
                })),
        )
        .into_any_element()
}

fn render_confirm_banner(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> Option<AnyElement> {
    let action = ws.schema_designer_ui.confirm_action?;
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let label = match action {
        DestructiveConfirm::Truncate => {
            format!(
                "{}: {}",
                s.schema_truncate_table, ws.schema_designer_ui.table_name
            )
        }
        DestructiveConfirm::Drop => {
            format!(
                "{}: {}",
                s.schema_drop_table, ws.schema_designer_ui.table_name
            )
        }
    };

    Some(
        div()
            .px(px(14.0))
            .py(px(8.0))
            .border_b_1()
            .border_color(theme::hairline_border_color(cx))
            .bg(colors.danger.color.opacity(0.12))
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(10.0))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .size(IconSize::Sm)
                            .color(colors.danger.color),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.foreground)
                            .child(label),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        Button::new("schema-confirm-cancel")
                            .variant(Variant::Ghost)
                            .size(HeroSize::Sm)
                            .label(s.cancel_label)
                            .on_press(cx.listener(|ws, _, _, cx| {
                                ws.schema_designer_ui.confirm_action = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("schema-confirm-exec")
                            .variant(Variant::Danger)
                            .size(HeroSize::Sm)
                            .label(s.confirm_label)
                            .on_press(cx.listener(move |ws, _, _, cx| {
                                execute_destructive_action(ws, action, cx);
                            })),
                    ),
            )
            .into_any_element(),
    )
}

fn render_status_alert(
    ws: &TableMinusWorkspace,
    _cx: &mut Context<TableMinusWorkspace>,
) -> Option<AnyElement> {
    if let Some(err) = &ws.schema_designer_ui.alter_load_error {
        return Some(
            div()
                .px(px(14.0))
                .pt(px(8.0))
                .child(Alert::new(err.clone()).status(HeroColor::Danger))
                .into_any_element(),
        );
    }
    if let Some((ok, msg)) = &ws.schema_designer_ui.last_feedback {
        let color = if *ok {
            HeroColor::Success
        } else {
            HeroColor::Danger
        };
        return Some(
            div()
                .px(px(14.0))
                .pt(px(8.0))
                .child(Alert::new(msg.clone()).status(color))
                .into_any_element(),
        );
    }
    None
}

fn render_editor_column(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .border_r_1()
        .border_color(theme::hairline_border_color(cx))
        .flex()
        .flex_col()
        .child(
            div()
                .id("schema-designer-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(14.0))
                .flex()
                .flex_col()
                .gap(px(16.0))
                .child(render_columns_section(ws, cx))
                .child(Separator::new())
                .child(render_indexes_section(ws, cx))
                .child(Separator::new())
                .child(render_fks_section(ws, cx)),
        )
        .into_any_element()
}

fn render_columns_section(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let mut section = div().flex().flex_col().gap(px(8.0)).child(
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.foreground)
                    .child(format!(
                        "{} ({})",
                        s.columns_button,
                        ws.schema_designer_ui.columns.len()
                    )),
            )
            .child(
                Button::new("schema-add-col")
                    .variant(Variant::Secondary)
                    .size(HeroSize::Sm)
                    .child(Icon::new(IconName::Plus).size(IconSize::Xs))
                    .label(s.schema_add_column)
                    .on_press(cx.listener(|ws, _, _, cx| {
                        add_empty_column(ws, cx);
                        cx.notify();
                    })),
            ),
    );

    for (idx, col) in ws.schema_designer_ui.columns.iter().enumerate() {
        section = section.child(render_column_row(ws, idx, col, cx));
    }
    section.into_any_element()
}

fn render_column_row(
    ws: &TableMinusWorkspace,
    idx: usize,
    col: &ColumnDraft,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let col_id = col.id;
    let is_alter = ws.schema_designer_ui.mode == SchemaDesignerMode::AlterTable;
    let renamed = is_alter
        && col
            .original_name
            .as_deref()
            .is_some_and(|orig| orig != col.name_input.read(cx).value().trim());

    let mut row = div()
        .p(px(10.0))
        .rounded(px(theme::CONTROL_RADIUS_PX))
        .border_1()
        .border_color(if col.marked_drop {
            colors.danger.color
        } else {
            theme::hairline_border_color(cx)
        })
        .bg(theme::muted_surface_bg(cx))
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.0))
                .child(
                    div().flex_1().min_w(px(120.0)).child(
                        Input::new(&col.name_input)
                            .variant(FieldVariant::Secondary)
                            .placeholder(s.schema_column_name)
                            .full_width()
                            .on_change(cx.listener(|_, _: &str, _, cx| cx.notify())),
                    ),
                )
                .child(
                    div().flex_1().min_w(px(100.0)).max_w(px(150.0)).child(
                        Input::new(&col.type_input)
                            .variant(FieldVariant::Secondary)
                            .placeholder(s.schema_column_type)
                            .full_width()
                            .on_change(cx.listener(|_, _: &str, _, cx| cx.notify())),
                    ),
                )
                .child(
                    div().flex_1().min_w(px(100.0)).max_w(px(150.0)).child(
                        Input::new(&col.default_input)
                            .variant(FieldVariant::Secondary)
                            .placeholder(s.schema_default_value)
                            .full_width()
                            .on_change(cx.listener(|_, _: &str, _, cx| cx.notify())),
                    ),
                )
                .child(render_column_remove_button(idx, col, s, cx)),
        )
        .child(render_column_flags_row(col_id, col, renamed, s, cx));

    if col.marked_drop {
        row = row.opacity(0.65);
    }
    row.into_any_element()
}

fn render_column_remove_button(
    idx: usize,
    col: &ColumnDraft,
    s: &crate::i18n::Strings,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let col_id = col.id;
    let is_existing = col.original_name.is_some();
    Button::new(SharedString::from(format!("schema-col-drop-{col_id}")))
        .variant(if col.marked_drop {
            Variant::Danger
        } else {
            Variant::Ghost
        })
        .size(HeroSize::Sm)
        .aria_label(s.schema_drop_column)
        .on_press(cx.listener(move |ws, _, _, cx| {
            if is_existing {
                if let Some(c) = ws
                    .schema_designer_ui
                    .columns
                    .iter_mut()
                    .find(|c| c.id == col_id)
                {
                    c.marked_drop = !c.marked_drop;
                }
            } else if idx < ws.schema_designer_ui.columns.len() {
                ws.schema_designer_ui.columns.remove(idx);
            }
            cx.notify();
        }))
        .child(Icon::new(IconName::Trash2).size(IconSize::Xs))
        .into_any_element()
}

fn render_column_flags_row(
    col_id: u64,
    col: &ColumnDraft,
    renamed: bool,
    s: &crate::i18n::Strings,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let mut row = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(14.0))
        .child(
            Switch::new(SharedString::from(format!("schema-col-null-{col_id}")))
                .size(HeroSize::Sm)
                .is_selected(col.nullable)
                .label(s.schema_nullable)
                .on_change(cx.listener(move |ws, &on: &bool, _, cx| {
                    if let Some(c) = ws
                        .schema_designer_ui
                        .columns
                        .iter_mut()
                        .find(|c| c.id == col_id)
                    {
                        c.nullable = on;
                    }
                    cx.notify();
                })),
        )
        .child(
            Checkbox::new(SharedString::from(format!("schema-col-pk-{col_id}")))
                .is_selected(col.is_primary_key)
                .label(s.schema_primary_key)
                .on_change(cx.listener(move |ws, &on: &bool, _, cx| {
                    if let Some(c) = ws
                        .schema_designer_ui
                        .columns
                        .iter_mut()
                        .find(|c| c.id == col_id)
                    {
                        c.is_primary_key = on;
                        if on {
                            c.nullable = false;
                        }
                    }
                    cx.notify();
                })),
        )
        .child(
            Checkbox::new(SharedString::from(format!("schema-col-uniq-{col_id}")))
                .is_selected(col.is_unique)
                .label("UNIQUE")
                .on_change(cx.listener(move |ws, &on: &bool, _, cx| {
                    if let Some(c) = ws
                        .schema_designer_ui
                        .columns
                        .iter_mut()
                        .find(|c| c.id == col_id)
                    {
                        c.is_unique = on;
                    }
                    cx.notify();
                })),
        )
        .child(
            Checkbox::new(SharedString::from(format!("schema-col-auto-{col_id}")))
                .is_selected(col.auto_increment)
                .label("AUTO")
                .on_change(cx.listener(move |ws, &on: &bool, _, cx| {
                    if let Some(c) = ws
                        .schema_designer_ui
                        .columns
                        .iter_mut()
                        .find(|c| c.id == col_id)
                    {
                        c.auto_increment = on;
                    }
                    cx.notify();
                })),
        );

    if renamed {
        row = row.child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .size(HeroSize::Sm)
                .color(HeroColor::Warning)
                .child(ChipLabel::new().child(s.schema_rename_column)),
        );
    }
    row.into_any_element()
}

fn render_indexes_section(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let mut section = div().flex().flex_col().gap(px(8.0)).child(
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.foreground)
                    .child(format!(
                        "{} ({})",
                        s.indexes_label,
                        ws.schema_designer_ui.indexes.len()
                    )),
            )
            .child(
                Button::new("schema-add-index")
                    .variant(Variant::Secondary)
                    .size(HeroSize::Sm)
                    .child(Icon::new(IconName::Plus).size(IconSize::Xs))
                    .label(s.schema_add_index)
                    .on_press(cx.listener(|ws, _, _, cx| {
                        add_empty_index(ws, cx);
                        cx.notify();
                    })),
            ),
    );

    for (idx, item) in ws.schema_designer_ui.indexes.iter().enumerate() {
        section = section.child(render_index_row(idx, item, cx));
    }
    section.into_any_element()
}

fn render_index_row(
    idx: usize,
    item: &IndexDraft,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let item_id = item.id;
    let from_existing = item.from_existing;
    div()
        .p(px(8.0))
        .rounded(px(theme::CONTROL_RADIUS_PX))
        .border_1()
        .border_color(if item.marked_drop {
            colors.danger.color
        } else {
            theme::hairline_border_color(cx)
        })
        .bg(theme::muted_surface_bg(cx))
        .min_w_0()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.0))
        .child(
            div().flex_1().min_w(px(120.0)).max_w(px(180.0)).child(
                Input::new(&item.name_input)
                    .variant(FieldVariant::Secondary)
                    .placeholder("idx_name")
                    .full_width()
                    .on_change(cx.listener(|_, _: &str, _, cx| cx.notify())),
            ),
        )
        .child(
            div().flex_1().min_w(px(130.0)).child(
                Input::new(&item.columns_input)
                    .variant(FieldVariant::Secondary)
                    .placeholder("col1, col2")
                    .full_width()
                    .on_change(cx.listener(|_, _: &str, _, cx| cx.notify())),
            ),
        )
        .child(
            Checkbox::new(SharedString::from(format!("schema-idx-uniq-{item_id}")))
                .is_selected(item.unique)
                .label("UNIQUE")
                .on_change(cx.listener(move |ws, &on: &bool, _, cx| {
                    if let Some(entry) = ws
                        .schema_designer_ui
                        .indexes
                        .iter_mut()
                        .find(|i| i.id == item_id)
                    {
                        entry.unique = on;
                    }
                    cx.notify();
                })),
        )
        .child(
            Button::new(SharedString::from(format!("schema-idx-del-{item_id}")))
                .variant(if item.marked_drop {
                    Variant::Danger
                } else {
                    Variant::Ghost
                })
                .size(HeroSize::Sm)
                .on_press(cx.listener(move |ws, _, _, cx| {
                    if from_existing {
                        if let Some(entry) = ws
                            .schema_designer_ui
                            .indexes
                            .iter_mut()
                            .find(|i| i.id == item_id)
                        {
                            entry.marked_drop = !entry.marked_drop;
                        }
                    } else if idx < ws.schema_designer_ui.indexes.len() {
                        ws.schema_designer_ui.indexes.remove(idx);
                    }
                    cx.notify();
                }))
                .child(Icon::new(IconName::Trash2).size(IconSize::Xs)),
        )
        .into_any_element()
}

fn render_fks_section(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let mut section = div().flex().flex_col().gap(px(8.0)).child(
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.foreground)
                    .child(format!(
                        "{} ({})",
                        s.foreign_keys_label,
                        ws.schema_designer_ui.foreign_keys.len()
                    )),
            )
            .child(
                Button::new("schema-add-fk")
                    .variant(Variant::Secondary)
                    .size(HeroSize::Sm)
                    .child(Icon::new(IconName::Plus).size(IconSize::Xs))
                    .label(s.schema_add_fk)
                    .on_press(cx.listener(|ws, _, _, cx| {
                        add_empty_fk(ws, cx);
                        cx.notify();
                    })),
            ),
    );

    for (idx, fk) in ws.schema_designer_ui.foreign_keys.iter().enumerate() {
        section = section.child(render_fk_row(ws, idx, fk, cx));
    }
    section.into_any_element()
}

fn render_fk_row(
    ws: &TableMinusWorkspace,
    idx: usize,
    fk: &FkDraft,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let fk_id = fk.id;
    let from_existing = fk.from_existing;
    div()
        .p(px(8.0))
        .rounded(px(theme::CONTROL_RADIUS_PX))
        .border_1()
        .border_color(if fk.marked_drop {
            colors.danger.color
        } else {
            theme::hairline_border_color(cx)
        })
        .bg(theme::muted_surface_bg(cx))
        .min_w_0()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.0))
        .child(
            div().flex_1().min_w(px(110.0)).child(
                Input::new(&fk.name_input)
                    .variant(FieldVariant::Secondary)
                    .placeholder("fk_name")
                    .full_width()
                    .on_change(cx.listener(|_, _: &str, _, cx| cx.notify())),
            ),
        )
        .child(
            div().flex_1().min_w(px(100.0)).child(
                Input::new(&fk.column_input)
                    .variant(FieldVariant::Secondary)
                    .placeholder(s.schema_column_name)
                    .full_width()
                    .on_change(cx.listener(|_, _: &str, _, cx| cx.notify())),
            ),
        )
        .child(
            div().flex_1().min_w(px(110.0)).child(
                Input::new(&fk.ref_table_input)
                    .variant(FieldVariant::Secondary)
                    .placeholder(s.schema_fk_ref_table)
                    .full_width()
                    .on_change(cx.listener(|_, _: &str, _, cx| cx.notify())),
            ),
        )
        .child(
            div().flex_1().min_w(px(100.0)).child(
                Input::new(&fk.ref_column_input)
                    .variant(FieldVariant::Secondary)
                    .placeholder(s.schema_fk_ref_column)
                    .full_width()
                    .on_change(cx.listener(|_, _: &str, _, cx| cx.notify())),
            ),
        )
        .child(
            Button::new(SharedString::from(format!("schema-fk-del-{fk_id}")))
                .variant(if fk.marked_drop {
                    Variant::Danger
                } else {
                    Variant::Ghost
                })
                .size(HeroSize::Sm)
                .on_press(cx.listener(move |ws, _, _, cx| {
                    if from_existing {
                        if let Some(entry) = ws
                            .schema_designer_ui
                            .foreign_keys
                            .iter_mut()
                            .find(|f| f.id == fk_id)
                        {
                            entry.marked_drop = !entry.marked_drop;
                        }
                    } else if idx < ws.schema_designer_ui.foreign_keys.len() {
                        ws.schema_designer_ui.foreign_keys.remove(idx);
                    }
                    cx.notify();
                }))
                .child(Icon::new(IconName::Trash2).size(IconSize::Xs)),
        )
        .into_any_element()
}

fn render_preview_column(
    ws: &TableMinusWorkspace,
    preview_sql: String,
    stmts: Vec<String>,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let editor_font = theme::editor_gpui_font(&ws.controller.settings);
    let grammar = ws.active_grammar();
    let copy_sql = preview_sql.clone();
    let can_execute = !stmts.is_empty() && stmts.iter().any(|st| !st.trim().starts_with("--"));

    div()
        .w(px(340.0))
        .min_w(px(240.0))
        .max_w(px(390.0))
        .flex_shrink(1.0)
        .min_h_0()
        .p(px(14.0))
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(s.schema_preview_sql),
                )
                .child(
                    Button::new("schema-copy-sql")
                        .variant(Variant::Tertiary)
                        .size(HeroSize::Sm)
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
                .id("schema-sql-preview-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(12.0))
                .rounded(px(theme::PANEL_RADIUS_PX))
                .border_1()
                .border_color(theme::hairline_border_color(cx))
                .bg(ws.content_palette.editor_background)
                .font(editor_font)
                .text_size(px(ws.controller.settings.editor_font.size.max(11.0)))
                .line_height(px(19.0))
                .text_color(ws.content_palette.editor_text)
                .child(if preview_sql.trim().is_empty() {
                    div()
                        .text_color(cx.colors().muted)
                        .child(s.schema_no_changes)
                        .into_any_element()
                } else {
                    syntax_bridge::styled_sql_block(preview_sql, grammar, &ws.content_palette)
                        .into_any_element()
                }),
        )
        .child(
            Button::new("schema-execute-ddl")
                .variant(Variant::Primary)
                .size(HeroSize::Md)
                .is_disabled(!can_execute)
                .child(Icon::new(IconName::Play).size(IconSize::Xs))
                .label(s.schema_execute_ddl)
                .on_press(cx.listener(move |ws, _, _, cx| {
                    execute_ddl_statements(ws, &stmts, cx);
                })),
        )
        .into_any_element()
}

fn switch_designer_mode(
    ws: &mut TableMinusWorkspace,
    mode: SchemaDesignerMode,
    cx: &mut Context<TableMinusWorkspace>,
) {
    ws.schema_designer_ui.mode = mode;
    ws.schema_designer_ui.confirm_action = None;
    ws.schema_designer_ui.last_feedback = None;
    ws.schema_designer_ui.alter_load_error = None;
    if mode == SchemaDesignerMode::CreateTable {
        ws.schema_designer_ui.loaded_alter_key = None;
        ws.schema_designer_ui.original_columns.clear();
        for col in &mut ws.schema_designer_ui.columns {
            col.original_name = None;
            col.marked_drop = false;
        }
        for idx in &mut ws.schema_designer_ui.indexes {
            idx.from_existing = false;
            idx.marked_drop = false;
        }
        for fk in &mut ws.schema_designer_ui.foreign_keys {
            fk.from_existing = false;
            fk.marked_drop = false;
        }
    } else {
        maybe_auto_load_alter_target(ws, cx);
    }
}

fn maybe_auto_load_alter_target(
    ws: &mut TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) {
    if ws.schema_designer_ui.mode != SchemaDesignerMode::AlterTable {
        return;
    }
    if ws.schema_designer_ui.loaded_alter_key.is_some() {
        return;
    }
    let target = ws
        .controller
        .active_table
        .as_ref()
        .map(|t| (t.schema.clone(), t.name.clone()))
        .or_else(|| {
            editable_tables(&ws.controller.cached_objects)
                .first()
                .map(|o| (o.schema.clone(), o.name.clone()))
        });
    if let Some((schema, table)) = target {
        load_table_into_alter_mode(ws, schema, table, cx);
    }
}

fn load_table_into_alter_mode(
    ws: &mut TableMinusWorkspace,
    schema: Option<String>,
    table: String,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let key = encode_table_key(schema.as_deref(), &table);
    ws.schema_designer_ui.loaded_alter_key = Some(key);
    ws.schema_designer_ui.schema_name = schema.clone();
    ws.schema_designer_ui.table_name = table.clone();
    ws.schema_designer_ui
        .table_name_input
        .update(cx, |inp, cx| {
            inp.set_value(table.clone());
            cx.notify();
        });
    ws.schema_designer_ui.confirm_action = None;

    match ws
        .controller
        .fetch_table_definition_blocking(schema.as_deref(), &table)
    {
        Ok(def) => {
            ws.schema_designer_ui.alter_load_error = None;
            populate_from_definition(&mut ws.schema_designer_ui, &def, cx);
        }
        Err(err) => {
            ws.schema_designer_ui.alter_load_error = Some(err.to_string());
        }
    }
}

fn populate_from_definition(state: &mut SchemaDesignerState, def: &TableDefinition, cx: &mut App) {
    state.columns.clear();
    state.original_columns.clear();
    state.indexes.clear();
    state.foreign_keys.clear();

    for col in &def.columns {
        let default_expr = match &col.info.default {
            ColumnDefault::None => None,
            ColumnDefault::Literal(v) => Some(format_core_value(v)),
            ColumnDefault::Expression(expr) => Some(expr.as_sql().to_owned()),
        };
        let spec = ColumnSpec {
            name: col.info.name.clone(),
            data_type: col.info.column_type.name.clone(),
            nullable: col.info.nullable,
            default_expr,
            is_primary_key: col.info.primary_key,
            is_unique: false,
            auto_increment: col.info.is_auto_increment,
        };
        let orig = spec.name.clone();
        state.original_columns.push(spec.clone());
        let draft = state.make_column_draft(spec, Some(orig), cx);
        state.columns.push(draft);
    }

    for idx in &def.indexes {
        if idx.primary {
            continue;
        }
        let spec = IndexSpec {
            name: idx.name.clone(),
            columns: idx.key_columns(),
            unique: idx.unique,
        };
        let draft = state.make_index_draft(spec, true, cx);
        state.indexes.push(draft);
    }

    for fk in &def.foreign_keys {
        let spec = ForeignKeySpec {
            name: fk.name.clone(),
            column: fk.columns.first().cloned().unwrap_or_default(),
            ref_table: fk.ref_table.clone(),
            ref_column: fk.ref_columns.first().cloned().unwrap_or_default(),
            on_delete: fk.on_delete.map(|a| a.as_sql().to_owned()),
            on_update: fk.on_update.map(|a| a.as_sql().to_owned()),
        };
        let draft = state.make_fk_draft(spec, true, cx);
        state.foreign_keys.push(draft);
    }
}

fn add_empty_column(ws: &mut TableMinusWorkspace, cx: &mut App) {
    let next_num = ws.schema_designer_ui.columns.len() + 1;
    let draft = ws.schema_designer_ui.make_column_draft(
        ColumnSpec {
            name: format!("col_{next_num}"),
            data_type: "TEXT".to_owned(),
            nullable: true,
            default_expr: None,
            is_primary_key: false,
            is_unique: false,
            auto_increment: false,
        },
        None,
        cx,
    );
    ws.schema_designer_ui.columns.push(draft);
}

fn add_empty_index(ws: &mut TableMinusWorkspace, cx: &mut App) {
    let tbl = ws.schema_designer_ui.table_name.trim();
    let idx_num = ws.schema_designer_ui.indexes.len() + 1;
    let first_col = ws
        .schema_designer_ui
        .columns
        .first()
        .map(|c| c.name_input.read(cx).value().trim().to_owned())
        .unwrap_or_default();
    let draft = ws.schema_designer_ui.make_index_draft(
        IndexSpec {
            name: format!(
                "idx_{}_{}",
                if tbl.is_empty() { "tbl" } else { tbl },
                idx_num
            ),
            columns: if first_col.is_empty() {
                Vec::new()
            } else {
                vec![first_col]
            },
            unique: false,
        },
        false,
        cx,
    );
    ws.schema_designer_ui.indexes.push(draft);
}

fn add_empty_fk(ws: &mut TableMinusWorkspace, cx: &mut App) {
    let fk_num = ws.schema_designer_ui.foreign_keys.len() + 1;
    let draft = ws.schema_designer_ui.make_fk_draft(
        ForeignKeySpec {
            name: format!("fk_rel_{fk_num}"),
            column: String::new(),
            ref_table: String::new(),
            ref_column: "id".to_owned(),
            on_delete: None,
            on_update: None,
        },
        false,
        cx,
    );
    ws.schema_designer_ui.foreign_keys.push(draft);
}

fn read_column_spec(col: &ColumnDraft, cx: &App) -> ColumnSpec {
    let def_text = col.default_input.read(cx).value().trim().to_owned();
    ColumnSpec {
        name: col.name_input.read(cx).value().trim().to_owned(),
        data_type: col.type_input.read(cx).value().trim().to_owned(),
        nullable: col.nullable,
        default_expr: (!def_text.is_empty()).then_some(def_text),
        is_primary_key: col.is_primary_key,
        is_unique: col.is_unique,
        auto_increment: col.auto_increment,
    }
}

fn read_index_spec(idx: &IndexDraft, cx: &App) -> IndexSpec {
    let cols = idx
        .columns_input
        .read(cx)
        .value()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    IndexSpec {
        name: idx.name_input.read(cx).value().trim().to_owned(),
        columns: cols,
        unique: idx.unique,
    }
}

fn read_fk_spec(fk: &FkDraft, cx: &App) -> ForeignKeySpec {
    ForeignKeySpec {
        name: fk.name_input.read(cx).value().trim().to_owned(),
        column: fk.column_input.read(cx).value().trim().to_owned(),
        ref_table: fk.ref_table_input.read(cx).value().trim().to_owned(),
        ref_column: fk.ref_column_input.read(cx).value().trim().to_owned(),
        on_delete: fk.on_delete.clone(),
        on_update: None,
    }
}

fn compute_preview_statements(
    ws: &TableMinusWorkspace,
    engine: DatabaseEngine,
    cx: &App,
) -> Vec<String> {
    let state = &ws.schema_designer_ui;
    let table_name = state.table_name_input.read(cx).value().trim().to_owned();
    if table_name.is_empty() {
        return Vec::new();
    }
    let schema = state.schema_name.as_deref();
    if state.mode == SchemaDesignerMode::CreateTable {
        let spec = CreateTableSpec {
            table_name,
            if_not_exists: state.if_not_exists,
            columns: state
                .columns
                .iter()
                .map(|c| read_column_spec(c, cx))
                .filter(|c| !c.name.is_empty())
                .collect(),
            indexes: state
                .indexes
                .iter()
                .map(|i| read_index_spec(i, cx))
                .filter(|i| !i.name.is_empty() && !i.columns.is_empty())
                .collect(),
            foreign_keys: state
                .foreign_keys
                .iter()
                .map(|f| read_fk_spec(f, cx))
                .filter(|f| !f.name.is_empty() && !f.column.is_empty() && !f.ref_table.is_empty())
                .collect(),
        };
        let script = build_create_table_sql(engine, schema, &spec);
        return split_sql_script_statements(&script);
    }

    let ops = collect_alter_ops(state, cx);
    build_alter_table_sql(engine, schema, &table_name, &ops)
}

fn collect_alter_ops(state: &SchemaDesignerState, cx: &App) -> Vec<AlterTableOp> {
    let mut ops = Vec::new();
    for col in &state.columns {
        let spec = read_column_spec(col, cx);
        if col.marked_drop {
            if let Some(orig) = &col.original_name {
                ops.push(AlterTableOp::DropColumn(orig.clone()));
            }
            continue;
        }
        match &col.original_name {
            None if !spec.name.is_empty() => ops.push(AlterTableOp::AddColumn(spec)),
            Some(orig) => {
                if !spec.name.is_empty() && spec.name != *orig {
                    ops.push(AlterTableOp::RenameColumn {
                        from: orig.clone(),
                        to: spec.name.clone(),
                    });
                }
                if let Some(prev) = state.original_columns.iter().find(|c| c.name == *orig)
                    && (prev.data_type != spec.data_type
                        || prev.nullable != spec.nullable
                        || prev.default_expr != spec.default_expr)
                {
                    ops.push(AlterTableOp::ModifyColumn { column: spec });
                }
            }
            None => {}
        }
    }
    for idx in &state.indexes {
        let spec = read_index_spec(idx, cx);
        if idx.from_existing && idx.marked_drop && !spec.name.is_empty() {
            ops.push(AlterTableOp::DropIndex(spec.name));
        } else if !idx.from_existing
            && !idx.marked_drop
            && !spec.name.is_empty()
            && !spec.columns.is_empty()
        {
            ops.push(AlterTableOp::AddIndex(spec));
        }
    }
    for fk in &state.foreign_keys {
        let spec = read_fk_spec(fk, cx);
        if fk.from_existing && fk.marked_drop && !spec.name.is_empty() {
            ops.push(AlterTableOp::DropForeignKey(spec.name));
        } else if !fk.from_existing
            && !fk.marked_drop
            && !spec.name.is_empty()
            && !spec.column.is_empty()
            && !spec.ref_table.is_empty()
        {
            ops.push(AlterTableOp::AddForeignKey(spec));
        }
    }
    ops
}

fn execute_ddl_statements(
    ws: &mut TableMinusWorkspace,
    stmts: &[String],
    cx: &mut Context<TableMinusWorkspace>,
) {
    let runnable: Vec<String> = stmts
        .iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty() && !s.starts_with("--"))
        .collect();
    if runnable.is_empty() {
        return;
    }
    match ws.controller.execute_ddl_statements_blocking(&runnable) {
        Ok(count) => {
            let msg = format!("{count} {}", ws.controller.strings.statements_unit);
            ws.schema_designer_ui.last_feedback = Some((true, msg.clone()));
            push_success_toast(
                ws.controller.strings.schema_execute_ddl,
                Some(msg.into()),
                cx,
            );
            if ws.schema_designer_ui.mode == SchemaDesignerMode::AlterTable {
                let schema = ws.schema_designer_ui.schema_name.clone();
                let table = ws.schema_designer_ui.table_name.clone();
                load_table_into_alter_mode(ws, schema, table, cx);
            }
        }
        Err(err) => {
            let msg = err.to_string();
            ws.schema_designer_ui.last_feedback = Some((false, msg.clone()));
            push_error_toast(ws.controller.strings.failed, Some(msg.into()), cx);
        }
    }
    cx.notify();
}

fn handle_destructive_button(
    ws: &mut TableMinusWorkspace,
    action: DestructiveConfirm,
    cx: &mut Context<TableMinusWorkspace>,
) {
    if ws.schema_designer_ui.confirm_action == Some(action) {
        execute_destructive_action(ws, action, cx);
    } else {
        ws.schema_designer_ui.confirm_action = Some(action);
        cx.notify();
    }
}

fn execute_destructive_action(
    ws: &mut TableMinusWorkspace,
    action: DestructiveConfirm,
    cx: &mut Context<TableMinusWorkspace>,
) {
    ws.schema_designer_ui.confirm_action = None;
    let table = ws.schema_designer_ui.table_name.trim().to_owned();
    if table.is_empty() {
        return;
    }
    let schema = ws.schema_designer_ui.schema_name.as_deref();
    let engine = ws.controller.active_database_engine();
    let op = match action {
        DestructiveConfirm::Truncate => AlterTableOp::TruncateTable,
        DestructiveConfirm::Drop => AlterTableOp::DropTable { if_exists: true },
    };
    let stmts = build_alter_table_sql(engine, schema, &table, &[op]);
    let title = match action {
        DestructiveConfirm::Truncate => ws.controller.strings.schema_truncate_table,
        DestructiveConfirm::Drop => ws.controller.strings.schema_drop_table,
    };
    match ws.controller.execute_ddl_statements_blocking(&stmts) {
        Ok(_) => {
            ws.schema_designer_ui.last_feedback = Some((true, table.clone()));
            push_success_toast(title, Some(SharedString::from(table)), cx);
            if action == DestructiveConfirm::Drop {
                ws.schema_designer_ui.loaded_alter_key = None;
                ws.schema_designer_ui.columns.clear();
                ws.schema_designer_ui.original_columns.clear();
                ws.schema_designer_ui.indexes.clear();
                ws.schema_designer_ui.foreign_keys.clear();
            }
        }
        Err(err) => {
            let msg = err.to_string();
            ws.schema_designer_ui.last_feedback = Some((false, msg.clone()));
            push_error_toast(title, Some(msg.into()), cx);
        }
    }
    cx.notify();
}

fn editable_tables(objects: &[ObjectInfo]) -> Vec<&ObjectInfo> {
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
