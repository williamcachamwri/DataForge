//! Data Import Wizard & SQL Script Runner workbench view.

use std::path::Path;

use herogpui::components::{
    Alert, Button, Checkbox, Chip, ChipLabel, ChipVariant, FieldVariant, Icon, IconName, IconSize,
    Input, InputState, Switch, TabItem, Tabs, TabsSize, TabsVariant, Variant,
};
use herogpui::core::{Color as HeroColor, Size as HeroSize};
use herogpui::gpui::{
    AnyElement, App, AppContext as _, AsyncApp, Context, Entity, FontWeight, InteractiveElement,
    IntoElement, ParentElement, PathPromptOptions, SharedString, StatefulInteractiveElement,
    Styled, Task, WeakEntity, Window, div, px,
};
use herogpui::theme::ActiveTheme;
use tableminus_services::{
    ColumnMapping, ImportFormat, ParsedImportPreview, detect_import_format,
    generate_import_sql_batches, preview_import_file, read_all_import_rows,
    split_sql_script_statements,
};

use super::shell_state::{push_error_toast, push_success_toast};
use super::{TableMinusWorkspace, syntax_bridge, theme};

const PREVIEW_ROW_LIMIT: usize = 25;
const PREVIEW_STMT_LIMIT: usize = 25;
const IMPORT_BATCH_SIZE: usize = 500;

/// Active mode inside the Import Wizard tool view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImportWizardMode {
    #[default]
    TabularData,
    SqlScript,
}

impl ImportWizardMode {
    const fn key(self) -> &'static str {
        match self {
            Self::TabularData => "data",
            Self::SqlScript => "sql",
        }
    }

    fn from_key(key: &str) -> Self {
        match key {
            "sql" => Self::SqlScript,
            _ => Self::TabularData,
        }
    }
}

#[derive(Debug)]
struct MappingDraft {
    source_index: usize,
    source_name: String,
    include: bool,
    target_col_input: Entity<InputState>,
    target_type_input: Entity<InputState>,
}

/// Persistent UI state for the Data Import Wizard & SQL Script Runner.
#[derive(Debug)]
pub struct ImportWizardState {
    pub file_path: Option<String>,
    mode: ImportWizardMode,
    detected_format: Option<ImportFormat>,
    preview: Option<ParsedImportPreview>,
    mappings: Vec<MappingDraft>,
    target_table_input: Entity<InputState>,
    create_new_table: bool,
    script_text: String,
    script_statements: Vec<String>,
    last_error: Option<String>,
    last_success: Option<String>,
    picker_task: Option<Task<()>>,
}

impl ImportWizardState {
    /// Creates initial Import Wizard state owned by [`TableMinusWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, cx: &mut Context<TableMinusWorkspace>) -> Self {
        let target_table_input = cx.new(|cx| InputState::with_value(cx, "imported_data"));
        Self {
            file_path: None,
            mode: ImportWizardMode::TabularData,
            detected_format: None,
            preview: None,
            mappings: Vec::new(),
            target_table_input,
            create_new_table: true,
            script_text: String::new(),
            script_statements: Vec::new(),
            last_error: None,
            last_success: None,
            picker_task: None,
        }
    }
}

/// Renders the Data Import Wizard & SQL Script Runner view.
#[must_use]
pub fn render_import_wizard(
    ws: &mut TableMinusWorkspace,
    _window: &mut Window,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let body = match ws.import_wizard_ui.mode {
        ImportWizardMode::TabularData => render_tabular_import_body(ws, cx),
        ImportWizardMode::SqlScript => render_sql_script_body(ws, cx),
    };

    div()
        .id("import-wizard-view")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme::panel_surface_bg(cx))
        .child(render_header(ws, cx))
        .child(render_toolbar(ws, cx))
        .children(render_feedback_alerts(ws))
        .child(div().flex_1().min_h_0().flex().flex_col().child(body))
        .into_any_element()
}

fn render_header(ws: &TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
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
                    Icon::new(IconName::Upload)
                        .size(IconSize::Sm)
                        .color(colors.accent.color),
                )
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(s.import_wizard),
                ),
        )
        .child(
            Button::new("import-wizard-close")
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
    let selected_key = SharedString::from(ws.import_wizard_ui.mode.key());
    let mode_items = vec![
        TabItem::new("data", format!("{} (CSV / JSON / XLSX)", s.import_wizard)),
        TabItem::new("sql", s.import_sql_script),
    ];
    let entity = cx.entity().clone();
    let mode_tabs = Tabs::new("import-wizard-mode", mode_items, selected_key.clone())
        .selected_key(selected_key)
        .variant(TabsVariant::Secondary)
        .size(TabsSize::Sm)
        .on_selection_change(move |key, _w, cx| {
            let next = ImportWizardMode::from_key(key.as_ref());
            entity.update(cx, |ws, cx| {
                ws.import_wizard_ui.mode = next;
                ws.import_wizard_ui.last_error = None;
                ws.import_wizard_ui.last_success = None;
                cx.notify();
            });
        });

    let file_label = ws
        .import_wizard_ui
        .file_path
        .as_deref()
        .unwrap_or(s.none_label)
        .to_owned();

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
        .child(mode_tabs)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .max_w(px(320.0))
                        .text_size(px(11.0))
                        .text_color(cx.colors().muted)
                        .truncate()
                        .child(file_label),
                )
                .child(
                    Button::new("import-select-file-btn")
                        .variant(Variant::Secondary)
                        .size(HeroSize::Sm)
                        .child(Icon::new(IconName::Folder).size(IconSize::Xs))
                        .label(s.import_select_file)
                        .on_press(cx.listener(|ws, _, _, cx| {
                            trigger_select_import_file(ws, cx);
                        })),
                ),
        )
        .into_any_element()
}

fn render_feedback_alerts(ws: &TableMinusWorkspace) -> Option<AnyElement> {
    if let Some(err) = &ws.import_wizard_ui.last_error {
        return Some(
            div()
                .px(px(14.0))
                .pt(px(8.0))
                .child(Alert::new(err.clone()).status(HeroColor::Danger))
                .into_any_element(),
        );
    }
    if let Some(msg) = &ws.import_wizard_ui.last_success {
        return Some(
            div()
                .px(px(14.0))
                .pt(px(8.0))
                .child(Alert::new(msg.clone()).status(HeroColor::Success))
                .into_any_element(),
        );
    }
    None
}

fn render_tabular_import_body(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let Some(preview) = ws.import_wizard_ui.preview.as_ref() else {
        return render_empty_drop_prompt(
            s.import_wizard,
            s.import_select_file,
            "CSV / TSV / JSON / NDJSON / XLSX",
            cx,
        );
    };

    let format_name = ws
        .import_wizard_ui
        .detected_format
        .map_or(s.import_unknown_format, format_display_name);

    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(render_target_bar(ws, format_name, preview.total_rows, cx))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_row()
                .child(render_mapping_pane(ws, cx))
                .child(render_sample_rows_pane(ws, preview, cx)),
        )
        .into_any_element()
}

fn render_target_bar(
    ws: &TableMinusWorkspace,
    format_name: &str,
    total_rows: usize,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let has_included = ws.import_wizard_ui.mappings.iter().any(|m| m.include);
    let table_non_empty = !ws
        .import_wizard_ui
        .target_table_input
        .read(cx)
        .value()
        .trim()
        .is_empty();

    div()
        .px(px(14.0))
        .py(px(10.0))
        .border_b_1()
        .border_color(theme::hairline_border_color(cx))
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(10.0))
                .child(
                    Chip::new()
                        .variant(ChipVariant::Soft)
                        .size(HeroSize::Sm)
                        .color(HeroColor::Accent)
                        .child(
                            ChipLabel::new()
                                .child(format!("{}: {format_name}", s.import_format_label)),
                        ),
                )
                .child(
                    Chip::new()
                        .variant(ChipVariant::Secondary)
                        .size(HeroSize::Sm)
                        .child(ChipLabel::new().child(format!("{total_rows} {}", s.rows_suffix))),
                )
                .child(
                    div().w(px(210.0)).child(
                        Input::new(&ws.import_wizard_ui.target_table_input)
                            .variant(FieldVariant::Secondary)
                            .placeholder(s.import_target_table)
                            .full_width()
                            .on_change(cx.listener(|_, _: &str, _, cx| cx.notify())),
                    ),
                )
                .child(
                    Switch::new("import-create-table-switch")
                        .size(HeroSize::Sm)
                        .is_selected(ws.import_wizard_ui.create_new_table)
                        .label(s.import_create_new_table)
                        .on_change(cx.listener(|ws, &on: &bool, _, cx| {
                            ws.import_wizard_ui.create_new_table = on;
                            cx.notify();
                        })),
                ),
        )
        .child(
            Button::new("import-run-btn")
                .variant(Variant::Primary)
                .size(HeroSize::Sm)
                .is_disabled(!has_included || !table_non_empty)
                .child(Icon::new(IconName::Play).size(IconSize::Xs))
                .label(s.import_run_import)
                .on_press(cx.listener(|ws, _, _, cx| {
                    execute_tabular_import(ws, cx);
                })),
        )
        .into_any_element()
}

fn render_mapping_pane(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let mut list = div()
        .id("import-mapping-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .p(px(12.0))
        .flex()
        .flex_col()
        .gap(px(8.0));

    for (idx, m) in ws.import_wizard_ui.mappings.iter().enumerate() {
        let src_idx = m.source_index;
        list = list.child(
            div()
                .p(px(8.0))
                .rounded(px(theme::CONTROL_RADIUS_PX))
                .border_1()
                .border_color(theme::hairline_border_color(cx))
                .bg(theme::muted_surface_bg(cx))
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    Checkbox::new(SharedString::from(format!("import-map-inc-{src_idx}")))
                        .is_selected(m.include)
                        .label(m.source_name.clone())
                        .on_change(cx.listener(move |ws, &on: &bool, _, cx| {
                            if let Some(entry) = ws.import_wizard_ui.mappings.get_mut(idx) {
                                entry.include = on;
                            }
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            div().flex_1().child(
                                Input::new(&m.target_col_input)
                                    .variant(FieldVariant::Secondary)
                                    .placeholder(s.schema_column_name)
                                    .full_width(),
                            ),
                        )
                        .child(
                            div().w(px(105.0)).child(
                                Input::new(&m.target_type_input)
                                    .variant(FieldVariant::Secondary)
                                    .placeholder(s.schema_column_type)
                                    .full_width(),
                            ),
                        ),
                ),
        );
    }

    div()
        .w(px(310.0))
        .min_w(px(230.0))
        .max_w(px(340.0))
        .min_h_0()
        .border_r_1()
        .border_color(theme::hairline_border_color(cx))
        .flex()
        .flex_col()
        .child(
            div()
                .px(px(12.0))
                .py(px(8.0))
                .border_b_1()
                .border_color(theme::hairline_border_color(cx))
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.foreground)
                .child(format!(
                    "{} ({})",
                    s.import_column_mapping,
                    ws.import_wizard_ui.mappings.len()
                )),
        )
        .child(list)
        .into_any_element()
}

fn render_sample_rows_pane(
    ws: &TableMinusWorkspace,
    preview: &ParsedImportPreview,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let colors = cx.colors().clone();
    let border = theme::hairline_border_color(cx);
    let value_font = theme::value_gpui_font(&ws.controller.settings);

    div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .child(
            div()
                .px(px(12.0))
                .py(px(8.0))
                .border_b_1()
                .border_color(border)
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.foreground)
                .child(format!(
                    "{} ({}/{})",
                    s.import_preview_rows,
                    preview.sample_rows.len(),
                    preview.total_rows
                )),
        )
        .child(render_sample_table_grid(preview, value_font, border, cx))
        .into_any_element()
}

fn render_sample_table_grid(
    preview: &ParsedImportPreview,
    value_font: herogpui::Font,
    border: herogpui::Hsla,
    cx: &App,
) -> AnyElement {
    let colors = cx.colors().clone();
    let col_count = u16::try_from(preview.columns.len().max(1)).map_or(1000.0, f32::from);
    let table_min_w = px(col_count * 130.0 + 16.0);

    let mut header_row = div()
        .min_w(table_min_w)
        .w_full()
        .flex()
        .items_center()
        .border_b_1()
        .border_color(border)
        .bg(theme::muted_surface_bg(cx))
        .px(px(8.0))
        .h(px(30.0))
        .flex_shrink_0();
    for col in &preview.columns {
        header_row = header_row.child(
            div()
                .w(px(130.0))
                .flex_shrink_0()
                .px(px(6.0))
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.foreground)
                .truncate()
                .child(col.clone()),
        );
    }

    let mut rows_scroll = div()
        .id("import-preview-rows-scroll")
        .min_w(table_min_w)
        .w_full()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col();

    for (r_idx, row) in preview.sample_rows.iter().enumerate() {
        let mut row_el = div()
            .min_w(table_min_w)
            .w_full()
            .flex()
            .items_center()
            .px(px(8.0))
            .min_h(px(26.0))
            .border_b_1()
            .border_color(border);
        if let Some(zebra) = theme::row_zebra_bg(r_idx, cx) {
            row_el = row_el.bg(zebra);
        }
        for cell in row {
            row_el = row_el.child(
                div()
                    .w(px(130.0))
                    .flex_shrink_0()
                    .px(px(6.0))
                    .font(value_font.clone())
                    .text_size(px(11.0))
                    .text_color(colors.foreground)
                    .truncate()
                    .child(cell.clone()),
            );
        }
        rows_scroll = rows_scroll.child(row_el);
    }

    div()
        .id("import-preview-hscroll")
        .flex_1()
        .min_w_0()
        .min_h_0()
        .overflow_x_scroll()
        .flex()
        .flex_col()
        .child(
            div()
                .min_w(table_min_w)
                .w_full()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(header_row)
                .child(rows_scroll),
        )
        .into_any_element()
}

fn render_sql_script_body(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    if ws.import_wizard_ui.script_statements.is_empty() {
        return render_empty_drop_prompt(s.import_sql_script, s.import_select_file, ".sql", cx);
    }

    let stmt_count = ws.import_wizard_ui.script_statements.len();
    let editor_font = theme::editor_gpui_font(&ws.controller.settings);
    let border = theme::hairline_border_color(cx);

    let mut list = div()
        .id("import-sql-statements-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .p(px(14.0))
        .flex()
        .flex_col()
        .gap(px(8.0));

    for (idx, stmt) in ws
        .import_wizard_ui
        .script_statements
        .iter()
        .take(PREVIEW_STMT_LIMIT)
        .enumerate()
    {
        list = list.child(render_sql_statement_card(
            ws,
            idx,
            stmt,
            &editor_font,
            border,
            cx,
        ));
    }

    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(
            div()
                .px(px(14.0))
                .py(px(10.0))
                .border_b_1()
                .border_color(border)
                .flex()
                .items_center()
                .justify_between()
                .child(
                    Chip::new()
                        .variant(ChipVariant::Soft)
                        .size(HeroSize::Sm)
                        .color(HeroColor::Accent)
                        .child(
                            ChipLabel::new()
                                .child(format!("{}: {stmt_count}", s.import_statements_count)),
                        ),
                )
                .child(
                    Button::new("import-exec-script-btn")
                        .variant(Variant::Primary)
                        .size(HeroSize::Sm)
                        .child(Icon::new(IconName::Play).size(IconSize::Xs))
                        .label(s.import_sql_script)
                        .on_press(cx.listener(|ws, _, _, cx| {
                            execute_sql_script_import(ws, cx);
                        })),
                ),
        )
        .child(list)
        .into_any_element()
}

fn render_sql_statement_card(
    ws: &TableMinusWorkspace,
    idx: usize,
    stmt: &str,
    editor_font: &herogpui::Font,
    border: herogpui::Hsla,
    cx: &App,
) -> AnyElement {
    let s = &ws.controller.strings;
    let grammar = ws.active_grammar();
    div()
        .p(px(10.0))
        .rounded(px(theme::CONTROL_RADIUS_PX))
        .border_1()
        .border_color(border)
        .bg(ws.content_palette.editor_background)
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(cx.colors().muted)
                .child(format!("{} #{}", s.statement_prefix, idx + 1)),
        )
        .child(
            div()
                .font(editor_font.clone())
                .text_size(px(12.0))
                .text_color(ws.content_palette.editor_text)
                .child(syntax_bridge::styled_sql_block(
                    stmt.to_owned(),
                    grammar,
                    &ws.content_palette,
                )),
        )
        .into_any_element()
}

fn render_empty_drop_prompt(
    title: &'static str,
    cta: &'static str,
    hint: &'static str,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.0))
        .p(px(24.0))
        .child(
            Icon::new(IconName::Upload)
                .size(IconSize::Lg)
                .color(colors.muted),
        )
        .child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.foreground)
                .child(title),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(colors.muted)
                .child(hint),
        )
        .child(
            Button::new("import-empty-select-btn")
                .variant(Variant::Primary)
                .size(HeroSize::Sm)
                .child(Icon::new(IconName::Folder).size(IconSize::Xs))
                .label(cta)
                .on_press(cx.listener(|ws, _, _, cx| {
                    trigger_select_import_file(ws, cx);
                })),
        )
        .into_any_element()
}

fn trigger_select_import_file(ws: &mut TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) {
    let rx = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: None,
    });
    ws.import_wizard_ui.picker_task = Some(cx.spawn(
        async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |ws, cx| {
                load_selected_file(ws, &path, cx);
                cx.notify();
            });
        },
    ));
}

fn load_selected_file(
    ws: &mut TableMinusWorkspace,
    path: &Path,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let path_str = path.display().to_string();
    ws.import_wizard_ui.file_path = Some(path_str);
    ws.import_wizard_ui.last_error = None;
    ws.import_wizard_ui.last_success = None;

    let is_sql = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("sql"));
    if is_sql || ws.import_wizard_ui.mode == ImportWizardMode::SqlScript {
        ws.import_wizard_ui.mode = ImportWizardMode::SqlScript;
        load_sql_script_file(ws, path, cx);
        return;
    }

    match detect_import_format(path)
        .and_then(|fmt| preview_import_file(path, Some(fmt), PREVIEW_ROW_LIMIT).map(|p| (fmt, p)))
    {
        Ok((fmt, preview)) => {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(sanitize_identifier)
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "imported_data".to_owned());
            ws.import_wizard_ui
                .target_table_input
                .update(cx, |inp, cx| {
                    inp.set_value(stem);
                    cx.notify();
                });
            ws.import_wizard_ui.detected_format = Some(fmt);
            ws.import_wizard_ui.mappings = build_mapping_drafts(&preview, cx);
            ws.import_wizard_ui.preview = Some(preview);
        }
        Err(err) => {
            let msg = err.to_string();
            ws.import_wizard_ui.last_error = Some(msg.clone());
            ws.import_wizard_ui.preview = None;
            ws.import_wizard_ui.mappings.clear();
            push_error_toast(ws.controller.strings.failed, Some(msg.into()), cx);
        }
    }
}

fn load_sql_script_file(
    ws: &mut TableMinusWorkspace,
    path: &Path,
    cx: &mut Context<TableMinusWorkspace>,
) {
    match std::fs::read_to_string(path) {
        Ok(script) => {
            let stmts = split_sql_script_statements(&script);
            ws.import_wizard_ui.script_text = script;
            ws.import_wizard_ui.script_statements = stmts;
        }
        Err(err) => {
            let msg = err.to_string();
            ws.import_wizard_ui.last_error = Some(msg.clone());
            ws.import_wizard_ui.script_text.clear();
            ws.import_wizard_ui.script_statements.clear();
            push_error_toast(ws.controller.strings.failed, Some(msg.into()), cx);
        }
    }
}

fn build_mapping_drafts(preview: &ParsedImportPreview, cx: &mut App) -> Vec<MappingDraft> {
    preview
        .columns
        .iter()
        .enumerate()
        .map(|(idx, col)| {
            let inferred = preview
                .inferred_types
                .get(idx)
                .cloned()
                .unwrap_or_else(|| "TEXT".to_owned());
            let clean_col = sanitize_identifier(col);
            let target_col = if clean_col.is_empty() {
                format!("col_{}", idx + 1)
            } else {
                clean_col
            };
            MappingDraft {
                source_index: idx,
                source_name: col.clone(),
                include: true,
                target_col_input: cx.new(|cx| InputState::with_value(cx, target_col)),
                target_type_input: cx.new(|cx| InputState::with_value(cx, inferred)),
            }
        })
        .collect()
}

fn execute_tabular_import(ws: &mut TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) {
    let Some(path_str) = ws.import_wizard_ui.file_path.clone() else {
        return;
    };
    let target_table = ws
        .import_wizard_ui
        .target_table_input
        .read(cx)
        .value()
        .trim()
        .to_owned();
    if target_table.is_empty() {
        return;
    }
    let mappings: Vec<ColumnMapping> = ws
        .import_wizard_ui
        .mappings
        .iter()
        .map(|m| ColumnMapping {
            source_index: m.source_index,
            target_column: m.target_col_input.read(cx).value().trim().to_owned(),
            target_type: m.target_type_input.read(cx).value().trim().to_owned(),
            include: m.include,
        })
        .collect();
    let format = ws.import_wizard_ui.detected_format;
    let create_new = ws.import_wizard_ui.create_new_table;
    let engine = ws.controller.active_database_engine();

    let rows = match read_all_import_rows(Path::new(&path_str), format) {
        Ok((_, rows)) => rows,
        Err(err) => {
            let msg = err.to_string();
            ws.import_wizard_ui.last_error = Some(msg.clone());
            push_error_toast(ws.controller.strings.failed, Some(msg.into()), cx);
            cx.notify();
            return;
        }
    };
    let row_count = rows.len();
    let batches = generate_import_sql_batches(
        engine,
        None,
        &target_table,
        create_new,
        &mappings,
        &rows,
        IMPORT_BATCH_SIZE,
    );
    match ws.controller.execute_ddl_statements_blocking(&batches) {
        Ok(batch_count) => {
            let summary = format!(
                "{} ({row_count} {}, {batch_count} {})",
                ws.controller.strings.import_success_summary,
                ws.controller.strings.rows_suffix,
                ws.controller.strings.statements_unit
            );
            ws.import_wizard_ui.last_error = None;
            ws.import_wizard_ui.last_success = Some(summary.clone());
            push_success_toast(
                ws.controller.strings.import_success_summary,
                Some(summary.into()),
                cx,
            );
        }
        Err(err) => {
            let msg = err.to_string();
            ws.import_wizard_ui.last_error = Some(msg.clone());
            push_error_toast(ws.controller.strings.failed, Some(msg.into()), cx);
        }
    }
    cx.notify();
}

fn execute_sql_script_import(ws: &mut TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) {
    let script = ws.import_wizard_ui.script_text.clone();
    if script.trim().is_empty() {
        return;
    }
    match ws.controller.execute_sql_script_blocking(&script) {
        Ok((stmts, rows)) => {
            let summary = format!(
                "{}: {stmts} {}, {rows} {}",
                ws.controller.strings.import_success_summary,
                ws.controller.strings.statements_unit,
                ws.controller.strings.rows_suffix
            );
            ws.import_wizard_ui.last_error = None;
            ws.import_wizard_ui.last_success = Some(summary.clone());
            push_success_toast(
                ws.controller.strings.import_success_summary,
                Some(summary.into()),
                cx,
            );
        }
        Err(err) => {
            let msg = err.to_string();
            ws.import_wizard_ui.last_error = Some(msg.clone());
            push_error_toast(ws.controller.strings.failed, Some(msg.into()), cx);
        }
    }
    cx.notify();
}

fn format_display_name(fmt: ImportFormat) -> &'static str {
    match fmt {
        ImportFormat::Csv => "CSV / TSV",
        ImportFormat::JsonArray => "JSON Array",
        ImportFormat::JsonLines => "NDJSON",
        ImportFormat::Xlsx => "XLSX",
    }
}

fn sanitize_identifier(raw: &str) -> String {
    raw.trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
