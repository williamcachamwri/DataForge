//! Full-screen Apple System Settings-style Preferences screen covering Appearance
//! & VS Code Theme JSON import, separate Editor/Value fonts, Behavior & Safety,
//! live MCP Server lifecycle + IDE configs, and real Diagnostics.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use dataforge_state::theme::{ThemeAppearance, ThemeDefinition, ThemeSlot, VsCodeThemeImporter};
use dataforge_windows_platform::{
    AppSettings, ThemePreference, detect_reduced_motion, storage_paths,
};
use herogpui::components::{
    Button, Card, CardContent, CardDescription, CardHeader, CardTitle, Chip, ChipLabel,
    ChipVariant, Icon, IconName, IconSize, InputState, ListBox, ListBoxItem, NumberField,
    NumberState, PickerItem, Scrollbar, Select, Separator, Slider, Switch, TextArea, Variant,
};
use herogpui::core::{Color as HeroColor, SelectionMode, Size as HeroSize};
use herogpui::gpui::prelude::FluentBuilder as _;
use herogpui::gpui::{
    AnyElement, App, AppContext as _, AsyncApp, ClipboardItem, Context, Entity, InteractiveElement,
    IntoElement, ParentElement, PathPromptOptions, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, Task, WeakEntity, Window, div, px,
};
use herogpui::theme::ActiveTheme;
use secrecy::ExposeSecret;
use tablepro_mcp::{DEFAULT_PORT, MCP_PATH, obtain_token, replace_token};

use super::DataForgeWorkspace;
use super::shell_state::{SettingsSection, push_error_toast, push_success_toast};
use super::syntax_bridge::styled_sql_block;
use super::theme::{
    CONTROL_RADIUS_PX, ContentThemePalette, HAIRLINE_BORDER_PX, MICRO_GAP_PX, editor_gpui_font,
    hairline_border_color, muted_surface_bg, panel_surface_bg, value_gpui_font,
};
use crate::app::ScreenMode;

const SAMPLE_PREVIEW_SQL: &str = "-- DataForge Live Theme Preview\n\
SELECT u.id, u.email, COALESCE(p.plan, 'free') AS plan_name, NULL AS deleted_at\n\
FROM users AS u\n\
LEFT JOIN plans p ON p.user_id = u.id\n\
WHERE u.active = TRUE AND u.score >= 42\n\
ORDER BY u.created_at DESC LIMIT 50;";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SettingsCategory {
    #[default]
    Appearance,
    EditorAndFonts,
    BehaviorAndSafety,
    McpServer,
    Diagnostics,
}

impl SettingsCategory {
    const ALL: [Self; 5] = [
        Self::Appearance,
        Self::EditorAndFonts,
        Self::BehaviorAndSafety,
        Self::McpServer,
        Self::Diagnostics,
    ];

    const fn key(self) -> &'static str {
        match self {
            Self::Appearance => "appearance",
            Self::EditorAndFonts => "editor-fonts",
            Self::BehaviorAndSafety => "behavior-safety",
            Self::McpServer => "mcp-server",
            Self::Diagnostics => "diagnostics",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|cat| cat.key() == key)
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::EditorAndFonts => "Editor & Fonts",
            Self::BehaviorAndSafety => "Behavior & Safety",
            Self::McpServer => "MCP Server",
            Self::Diagnostics => "Diagnostics",
        }
    }

    const fn icon(self) -> IconName {
        match self {
            Self::Appearance => IconName::Palette,
            Self::EditorAndFonts => IconName::Type,
            Self::BehaviorAndSafety => IconName::Shield,
            Self::McpServer => IconName::Server,
            Self::Diagnostics => IconName::Activity,
        }
    }

    const fn from_section(section: SettingsSection) -> Self {
        match section {
            SettingsSection::Appearance => Self::Appearance,
            SettingsSection::Editor | SettingsSection::Fonts => Self::EditorAndFonts,
            SettingsSection::McpServer => Self::McpServer,
            SettingsSection::Shortcuts => Self::BehaviorAndSafety,
        }
    }

    const fn to_section(self) -> SettingsSection {
        match self {
            Self::Appearance => SettingsSection::Appearance,
            Self::EditorAndFonts => SettingsSection::Editor,
            Self::BehaviorAndSafety => SettingsSection::Shortcuts,
            Self::McpServer => SettingsSection::McpServer,
            Self::Diagnostics => SettingsSection::Appearance,
        }
    }
}

/// Persistent UI state for the Settings screen.
pub struct SettingsScreenState {
    pub dirty: bool,
    active_category: SettingsCategory,
    last_external_section: SettingsSection,
    content_scroll: ScrollHandle,
    theme_json_input: Entity<InputState>,
    theme_json_error: Option<String>,
    editor_size_state: Entity<NumberState>,
    value_size_state: Entity<NumberState>,
    fetch_limit_state: Entity<NumberState>,
    query_timeout_state: Entity<NumberState>,
    mcp_port_state: Entity<NumberState>,
    query_timeout_secs: u32,
    mcp_token: Option<String>,
    reveal_mcp_token: bool,
    mcp_loading: bool,
    copied_key: Option<String>,
    copied_gen: u64,
    copy_reset_task: Option<Task<()>>,
    async_task: Option<Task<()>>,
    picker_task: Option<Task<()>>,
}

impl SettingsScreenState {
    /// Initializes Settings screen state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, cx: &mut Context<DataForgeWorkspace>) -> Self {
        let theme_json_input = cx.new(|cx| InputState::new(cx));
        let editor_size_state = cx.new(|cx| NumberState::new(cx, 13.0));
        let value_size_state = cx.new(|cx| NumberState::new(cx, 12.0));
        let fetch_limit_state = cx.new(|cx| NumberState::new(cx, 100.0));
        let query_timeout_state = cx.new(|cx| NumberState::new(cx, 30.0));
        let mcp_port_state = cx.new(|cx| NumberState::new(cx, f64::from(DEFAULT_PORT)));

        let mut state = Self {
            dirty: false,
            active_category: SettingsCategory::Appearance,
            last_external_section: SettingsSection::Appearance,
            content_scroll: ScrollHandle::new(),
            theme_json_input,
            theme_json_error: None,
            editor_size_state,
            value_size_state,
            fetch_limit_state,
            query_timeout_state,
            mcp_port_state,
            query_timeout_secs: 30,
            mcp_token: None,
            reveal_mcp_token: false,
            mcp_loading: false,
            copied_key: None,
            copied_gen: 0,
            copy_reset_task: None,
            async_task: None,
            picker_task: None,
        };
        state.load_mcp_token_async(cx);
        state
    }

    fn load_mcp_token_async(&mut self, cx: &mut Context<DataForgeWorkspace>) {
        self.async_task = Some(cx.spawn(
            async move |this: WeakEntity<DataForgeWorkspace>, cx: &mut AsyncApp| {
                let services = this
                    .read_with(cx, |ws, _| ws.controller.services.clone())
                    .ok()
                    .flatten();
                let Some(services) = services else {
                    return;
                };
                let creds = services.credentials.clone();
                let runtime = services.runtime.clone();
                let token_res = runtime
                    .spawn(async move {
                        obtain_token(&*creds)
                            .await
                            .map(|t| t.secret().expose_secret().to_owned())
                    })
                    .await;
                if let Ok(Ok(token)) = token_res {
                    let _ = this.update(cx, |ws, cx| {
                        ws.settings_ui.mcp_token = Some(token);
                        cx.notify();
                    });
                }
            },
        ));
    }

    fn copy_with_feedback(
        &mut self,
        key: String,
        payload: String,
        cx: &mut Context<DataForgeWorkspace>,
    ) {
        cx.write_to_clipboard(ClipboardItem::new_string(payload));
        self.copied_gen = self.copied_gen.wrapping_add(1);
        let generation = self.copied_gen;
        self.copied_key = Some(key);
        self.copy_reset_task = Some(cx.spawn(
            async move |this: WeakEntity<DataForgeWorkspace>, cx: &mut AsyncApp| {
                cx.background_executor()
                    .timer(Duration::from_millis(1400))
                    .await;
                let _ = this.update(cx, |ws, cx| {
                    if ws.settings_ui.copied_gen == generation {
                        ws.settings_ui.copied_key = None;
                        cx.notify();
                    }
                });
            },
        ));
        cx.notify();
    }
}

fn sync_settings_from_controller(ws: &mut DataForgeWorkspace, cx: &mut App) {
    let external_section = ws.settings_view.section;
    if external_section != ws.settings_ui.last_external_section {
        ws.settings_ui.last_external_section = external_section;
        ws.settings_ui.active_category = SettingsCategory::from_section(external_section);
    }
    sync_number_if_diff(
        &ws.settings_ui.editor_size_state,
        f64::from(ws.controller.settings.editor_font.size),
        cx,
    );
    sync_number_if_diff(
        &ws.settings_ui.value_size_state,
        f64::from(ws.controller.settings.value_font.size),
        cx,
    );
    sync_number_if_diff(
        &ws.settings_ui.fetch_limit_state,
        ws.controller.grid_model.page.limit as f64,
        cx,
    );
    sync_number_if_diff(
        &ws.settings_ui.mcp_port_state,
        f64::from(ws.controller.settings.mcp.port),
        cx,
    );
}

/// Renders the full-screen Apple System Settings preferences view.
#[must_use]
pub fn render_settings_screen(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    sync_settings_from_controller(ws, cx);

    let header = render_top_header(ws, cx);
    let sidebar = render_sidebar(ws, cx);
    let body = render_category_body(ws, cx);

    div()
        .id("workspace-settings-screen")
        .flex_1()
        .size_full()
        .flex()
        .flex_col()
        .bg(panel_surface_bg(cx))
        .child(header)
        .child(Separator::new())
        .child(
            div()
                .flex_1()
                .min_h(px(0.0))
                .flex()
                .flex_row()
                .child(sidebar)
                .child(body),
        )
        .into_any_element()
}

fn render_top_header(ws: &DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) -> AnyElement {
    let colors = cx.colors().clone();
    let notice = ws.settings_view.status_notice.clone();

    div()
        .h(px(48.0))
        .px(px(16.0))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(
                    Button::new("settings-back-workspace")
                        .variant(Variant::Ghost)
                        .size(HeroSize::Sm)
                        .on_press(cx.listener(|ws, _, _window, cx| {
                            ws.switch_screen(ScreenMode::Workspace, cx);
                        }))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.0))
                                .child(Icon::new(IconName::ArrowLeft).size(IconSize::Sm))
                                .child("Workspace"),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(herogpui::gpui::FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child("Settings"),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .when_some(notice, |el, msg| {
                    el.child(
                        Chip::new()
                            .variant(ChipVariant::Soft)
                            .color(HeroColor::Success)
                            .size(HeroSize::Sm)
                            .child(ChipLabel::new().child(msg)),
                    )
                })
                .child(
                    Button::new("settings-save-now")
                        .variant(Variant::Secondary)
                        .size(HeroSize::Sm)
                        .on_press(cx.listener(|ws, _, _window, cx| {
                            persist_settings(ws, cx);
                        }))
                        .child("Save Settings"),
                ),
        )
        .into_any_element()
}

fn render_sidebar(ws: &DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) -> AnyElement {
    let active_key = ws.settings_ui.active_category.key();
    let items: Vec<ListBoxItem> = SettingsCategory::ALL
        .into_iter()
        .map(|cat| {
            ListBoxItem::new(cat.key(), cat.label())
                .icon(cat.icon())
                .description(category_subtitle(cat))
        })
        .collect();

    div()
        .w(px(232.0))
        .h_full()
        .flex_shrink_0()
        .p(px(10.0))
        .bg(muted_surface_bg(cx))
        .border_r(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .child(
            ListBox::new("settings-category-list", items)
                .selection_mode(SelectionMode::Single)
                .selected_key(active_key)
                .on_selection_change(cx.listener(
                    |ws, selected: &HashSet<SharedString>, _window, cx| {
                        if let Some(cat) = selected
                            .iter()
                            .next()
                            .and_then(|k| SettingsCategory::from_key(k.as_ref()))
                        {
                            ws.settings_ui.active_category = cat;
                            let mapped = cat.to_section();
                            ws.settings_view.section = mapped;
                            ws.settings_ui.last_external_section = mapped;
                            cx.notify();
                        }
                    },
                )),
        )
        .into_any_element()
}

fn render_category_body(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let scroll_handle = ws.settings_ui.content_scroll.clone();
    let category = ws.settings_ui.active_category;

    let section_el = match category {
        SettingsCategory::Appearance => render_appearance_section(ws, cx),
        SettingsCategory::EditorAndFonts => render_editor_fonts_section(ws, cx),
        SettingsCategory::BehaviorAndSafety => render_behavior_safety_section(ws, cx),
        SettingsCategory::McpServer => render_mcp_section(ws, cx),
        SettingsCategory::Diagnostics => render_diagnostics_section(ws, cx),
    };

    let container = div()
        .w_full()
        .max_w(px(760.0))
        .p(px(24.0))
        .flex()
        .flex_col()
        .gap(px(16.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .text_base()
                        .font_weight(herogpui::gpui::FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(category.label()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted)
                        .child(category_subtitle(category)),
                ),
        )
        .child(section_el);

    div()
        .relative()
        .flex_1()
        .min_h(px(0.0))
        .child(
            div()
                .id("settings-scroll-content")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&scroll_handle)
                .child(container),
        )
        .child(Scrollbar::new("settings-scrollbar", scroll_handle))
        .into_any_element()
}

fn render_appearance_section(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let pref_key = theme_pref_key(ws.controller.settings.theme);
    let pref_items = vec![
        PickerItem::new("system", "System (Follow Windows)"),
        PickerItem::new("light", "Light"),
        PickerItem::new("dark", "Dark"),
        PickerItem::new("high-contrast", "High Contrast"),
    ];
    let pref_select = Select::new("settings-theme-pref", pref_items)
        .label("Interface Appearance")
        .description("Controls window chrome, sidebars, and drawers.")
        .full_width(true)
        .value(Some(SharedString::from(pref_key)))
        .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, window, cx| {
            if let Some(next_pref) = key.as_deref().and_then(parse_theme_pref) {
                apply_theme_preference(ws, next_pref, window, cx);
            }
        }));

    let preset_key = ws.controller.content_theme.id.0.clone();
    let preset_items = vec![
        PickerItem::new("builtin.light", "DataForge Light"),
        PickerItem::new("builtin.dark", "DataForge Dark"),
        PickerItem::new("builtin.high_contrast", "DataForge High Contrast"),
    ];
    let preset_select = Select::new("settings-content-preset", preset_items)
        .label("Content Theme Preset")
        .description("Drives SQL syntax highlighting and Data Grid surfaces (26 ThemeSlots).")
        .full_width(true)
        .value(Some(SharedString::from(preset_key)))
        .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, window, cx| {
            if let Some(id) = key.as_deref() {
                apply_builtin_content_theme(ws, id, window, cx);
            }
        }));

    let appearance_card = Card::new()
        .child(
            CardHeader::new()
                .child(CardTitle::new().child("Theme & Content Palette"))
                .child(CardDescription::new().child(
                    "Chrome adapts cleanly to Windows while preserving custom SQL syntax palettes.",
                )),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(12.0)))
                .child(pref_select)
                .child(Separator::new())
                .child(preset_select),
        );

    let importer_card = render_vscode_theme_import_card(ws, cx);

    div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(appearance_card)
        .child(importer_card)
        .into_any_element()
}

fn render_vscode_theme_preview(ws: &DataForgeWorkspace, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    let sql_preview = div()
        .p(px(12.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(ws.content_palette.editor_background)
        .border_1()
        .border_color(hairline_border_color(cx))
        .font(editor_gpui_font(&ws.controller.settings))
        .text_xs()
        .text_color(ws.content_palette.editor_text)
        .child(styled_sql_block(
            SAMPLE_PREVIEW_SQL,
            ws.active_grammar(),
            &ws.content_palette,
        ));

    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .text_xs()
                .font_weight(herogpui::gpui::FontWeight::MEDIUM)
                .text_color(colors.muted)
                .child("Live 26-Slot Syntax & Surface Preview"),
        )
        .child(sql_preview)
        .child(render_26_slot_swatches(&ws.content_palette, cx))
        .into_any_element()
}

fn render_vscode_theme_import_card(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let import_err = ws.settings_ui.theme_json_error.clone();
    let active_name = ws.controller.content_theme.name.clone();
    let active_slot_count = ws.controller.content_theme.colors.len();
    let last_import = ws.controller.last_theme_import.clone();

    let mut json_area = TextArea::new(&ws.settings_ui.theme_json_input)
        .rows(5)
        .font_family(editor_gpui_font(&ws.controller.settings).family)
        .full_width()
        .placeholder("Paste VS Code color theme JSON or load a .json theme file...");
    if let Some(err) = import_err {
        json_area = json_area.is_invalid(true).error_message(err);
    }

    let browse_btn = Button::new("settings-browse-vscode-theme")
        .variant(Variant::Secondary)
        .size(HeroSize::Sm)
        .on_press(cx.listener(|ws, _, window, cx| {
            browse_vscode_theme_file(ws, window, cx);
        }))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(Icon::new(IconName::FolderOpen).size(IconSize::Xs))
                .child("Load Theme JSON..."),
        );

    let apply_json_btn = Button::new("settings-apply-vscode-json")
        .variant(Variant::Primary)
        .size(HeroSize::Sm)
        .on_press(cx.listener(|ws, _, window, cx| {
            let raw = ws.settings_ui.theme_json_input.read(cx).value().to_owned();
            apply_imported_theme_bytes(ws, raw.as_bytes(), window, cx);
        }))
        .child("Import & Preview");

    let mut content = CardContent::new().sx(|d| d.gap(px(10.0)));
    if let Some(msg) = last_import {
        content = content.child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .color(HeroColor::Success)
                .size(HeroSize::Sm)
                .child(ChipLabel::new().child(msg)),
        );
    }
    content = content
        .child(json_area)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(browse_btn)
                .child(apply_json_btn),
        )
        .child(Separator::new())
        .child(render_vscode_theme_preview(ws, cx));

    Card::new()
        .child(
            CardHeader::new()
                .child(CardTitle::new().child("VS Code JSON Theme Import"))
                .child(CardDescription::new().child(format!(
                    "Active content theme: {active_name} ({active_slot_count}/26 ThemeSlots mapped)"
                ))),
        )
        .child(content)
        .into_any_element()
}

fn render_26_slot_swatches(palette: &ContentThemePalette, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    let mut grid = div().flex().flex_wrap().gap(px(MICRO_GAP_PX));
    for slot in ThemeSlot::ALL {
        let swatch_color = palette.color(slot);
        let short_key = slot.key().split('.').skip(1).collect::<Vec<_>>().join(".");
        grid = grid.child(
            div()
                .px(px(6.0))
                .py(px(3.0))
                .rounded(px(CONTROL_RADIUS_PX))
                .bg(muted_surface_bg(cx))
                .border_1()
                .border_color(hairline_border_color(cx))
                .flex()
                .items_center()
                .gap(px(5.0))
                .child(
                    div()
                        .w(px(10.0))
                        .h(px(10.0))
                        .rounded(px(3.0))
                        .bg(swatch_color)
                        .border_1()
                        .border_color(hairline_border_color(cx)),
                )
                .child(div().text_xs().text_color(colors.muted).child(short_key)),
        );
    }
    grid.into_any_element()
}

fn render_editor_fonts_section(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let editor_font_card = render_editor_font_card(ws, cx);
    let value_font_card = render_value_font_card(ws, cx);
    let editor_behavior_card = render_editor_behavior_card(ws, cx);

    div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(editor_font_card)
        .child(value_font_card)
        .child(editor_behavior_card)
        .into_any_element()
}

fn render_editor_font_card(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let spec = &ws.controller.settings.editor_font;
    let family_items = vec![
        PickerItem::new("Cascadia Mono", "Cascadia Mono"),
        PickerItem::new("Cascadia Code", "Cascadia Code"),
        PickerItem::new("Consolas", "Consolas"),
        PickerItem::new("JetBrains Mono", "JetBrains Mono"),
        PickerItem::new("Fira Code", "Fira Code"),
    ];
    let family_select = Select::new("settings-editor-font-family", family_items)
        .label("SQL & Structure Font Family")
        .full_width(true)
        .value(Some(SharedString::from(spec.family.clone())))
        .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _w, cx| {
            if let Some(fam) = key.as_ref() {
                ws.controller.settings.editor_font.family = fam.to_string();
                persist_settings(ws, cx);
            }
        }));

    let size_slider = Slider::new("settings-editor-font-size-slider", spec.size)
        .min_value(9.0)
        .max_value(28.0)
        .step(0.5)
        .label("Font Size (pt)")
        .show_value(true)
        .on_change(cx.listener(|ws, &val: &f32, _w, cx| {
            ws.controller.settings.editor_font.size = val.clamp(9.0, 28.0);
            persist_settings(ws, cx);
        }));

    let size_field = NumberField::new(&ws.settings_ui.editor_size_state)
        .label("Exact Size")
        .min_value(9.0)
        .max_value(28.0)
        .step(0.5)
        .on_change(cx.listener(|ws, &val: &f64, _w, cx| {
            ws.controller.settings.editor_font.size = (val as f32).clamp(9.0, 28.0);
            persist_settings(ws, cx);
        }));

    let line_height_slider = Slider::new("settings-editor-line-height", spec.line_height)
        .min_value(1.0)
        .max_value(2.2)
        .step(0.05)
        .label("Line Height")
        .show_value(true)
        .on_change(cx.listener(|ws, &val: &f32, _w, cx| {
            ws.controller.settings.editor_font.line_height = val.clamp(1.0, 2.2);
            persist_settings(ws, cx);
        }));

    let ligatures_switch = Switch::new("settings-editor-ligatures")
        .is_selected(spec.ligatures)
        .label("Programming Ligatures")
        .description("Renders >=, !=, => ligatures in fonts that support OpenType features.")
        .on_change(cx.listener(|ws, &enabled: &bool, _w, cx| {
            ws.controller.settings.editor_font.ligatures = enabled;
            persist_settings(ws, cx);
        }));

    Card::new()
        .child(
            CardHeader::new()
                .child(CardTitle::new().child("SQL Editor Font (editor_font)"))
                .child(
                    CardDescription::new()
                        .child("Monospace typography for SQL queries and AI SQL blocks."),
                ),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(10.0)))
                .child(family_select)
                .child(
                    div()
                        .flex()
                        .items_end()
                        .gap(px(12.0))
                        .child(div().flex_1().child(size_slider))
                        .child(div().w(px(120.0)).child(size_field)),
                )
                .child(line_height_slider)
                .child(Separator::new())
                .child(ligatures_switch),
        )
        .into_any_element()
}

fn render_value_font_card(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let spec = &ws.controller.settings.value_font;
    let family_items = vec![
        PickerItem::new("Segoe UI", "Segoe UI"),
        PickerItem::new("Segoe UI Variable", "Segoe UI Variable"),
        PickerItem::new("Cascadia Mono", "Cascadia Mono"),
        PickerItem::new("Consolas", "Consolas"),
        PickerItem::new("Inter", "Inter"),
    ];
    let family_select = Select::new("settings-value-font-family", family_items)
        .label("Data Grid & Inspector Value Font Family")
        .full_width(true)
        .value(Some(SharedString::from(spec.family.clone())))
        .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _w, cx| {
            if let Some(fam) = key.as_ref() {
                ws.controller.settings.value_font.family = fam.to_string();
                persist_settings(ws, cx);
            }
        }));

    let size_slider = Slider::new("settings-value-font-size-slider", spec.size)
        .min_value(9.0)
        .max_value(24.0)
        .step(0.5)
        .label("Value Size (pt)")
        .show_value(true)
        .on_change(cx.listener(|ws, &val: &f32, _w, cx| {
            ws.controller.settings.value_font.size = val.clamp(9.0, 24.0);
            persist_settings(ws, cx);
        }));

    let size_field = NumberField::new(&ws.settings_ui.value_size_state)
        .label("Exact Size")
        .min_value(9.0)
        .max_value(24.0)
        .step(0.5)
        .on_change(cx.listener(|ws, &val: &f64, _w, cx| {
            ws.controller.settings.value_font.size = (val as f32).clamp(9.0, 24.0);
            persist_settings(ws, cx);
        }));

    let sample_text = div()
        .px(px(10.0))
        .py(px(8.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(muted_surface_bg(cx))
        .font(value_gpui_font(&ws.controller.settings))
        .text_xs()
        .child("1,048,576  |  2026-11-14 01:05:12.409  |  order_uuid_7f9c");

    Card::new()
        .child(
            CardHeader::new()
                .child(CardTitle::new().child("Tabular Value Font (value_font)"))
                .child(CardDescription::new().child(
                    "Dedicated font for Data Grid cells, Row Inspector values, and query results.",
                )),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(10.0)))
                .child(family_select)
                .child(
                    div()
                        .flex()
                        .items_end()
                        .gap(px(12.0))
                        .child(div().flex_1().child(size_slider))
                        .child(div().w(px(120.0)).child(size_field)),
                )
                .child(sample_text),
        )
        .into_any_element()
}

fn render_editor_behavior_card(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let vim_on = ws.controller.vim_mode.is_some();
    let reduced_motion = detect_reduced_motion();

    let vim_switch = Switch::new("settings-vim-mode")
        .is_selected(vim_on)
        .label("Vim Keybindings in SQL Editor")
        .description("Enables Normal, Insert, Visual, and Command modes in the SQL editor.")
        .on_change(cx.listener(|ws, &want_on: &bool, _w, cx| {
            if (ws.controller.vim_mode.is_some()) != want_on {
                ws.controller.toggle_vim_mode();
            }
            cx.notify();
        }));

    Card::new()
        .child(
            CardHeader::new()
                .child(CardTitle::new().child("Editing & Accessibility"))
                .child(CardDescription::new().child("Editor modal bindings and Windows motion preferences.")),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(10.0)))
                .child(vim_switch)
                .child(Separator::new())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.0))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(herogpui::gpui::FontWeight::MEDIUM)
                                        .child("Windows Reduced Motion (SPI_GETCLIENTAREAANIMATION)"),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.colors().muted)
                                        .child("Drawers and modals automatically snap in 0ms when Windows animation is disabled."),
                                ),
                        )
                        .child(
                            Chip::new()
                                .variant(ChipVariant::Soft)
                                .color(if reduced_motion {
                                    HeroColor::Accent
                                } else {
                                    HeroColor::Default
                                })
                                .size(HeroSize::Sm)
                                .child(ChipLabel::new().child(if reduced_motion {
                                    "Reduced Motion Active"
                                } else {
                                    "Standard Motion"
                                })),
                        ),
                ),
        )
        .into_any_element()
}

fn render_behavior_safety_section(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let restore_switch = Switch::new("settings-restore-session")
        .is_selected(ws.controller.settings.restore_session)
        .label("Restore Workspace Tabs on Startup")
        .description(
            "Automatically reopen previous tabs and connection windows when launching DataForge.",
        )
        .on_change(cx.listener(|ws, &val: &bool, _w, cx| {
            ws.controller.settings.restore_session = val;
            persist_settings(ws, cx);
        }));

    let safe_mode_switch = Switch::new("settings-safe-mode")
        .is_selected(ws.controller.safe_mode_enabled)
        .label("Safe Mode Confirmation Prompts")
        .description("Require explicit modal approval before committing batch table edits or mutating writes.")
        .on_change(cx.listener(|ws, &val: &bool, _w, cx| {
            ws.controller.safe_mode_enabled = val;
            cx.notify();
        }));

    let fetch_limit = NumberField::new(&ws.settings_ui.fetch_limit_state)
        .label("Default Row Fetch Limit")
        .description("Rows per Data Grid page (10 - 10,000).")
        .min_value(10.0)
        .max_value(10_000.0)
        .step(50.0)
        .on_change(cx.listener(|ws, &val: &f64, _w, cx| {
            ws.controller.grid_model.page.limit = (val.round() as u64).clamp(10, 10_000);
            cx.notify();
        }));

    let timeout_field = NumberField::new(&ws.settings_ui.query_timeout_state)
        .label("Query Timeout (seconds)")
        .description("Maximum statement runtime before cancellation (5 - 600s).")
        .min_value(5.0)
        .max_value(600.0)
        .step(5.0)
        .on_change(cx.listener(|ws, &val: &f64, _w, cx| {
            ws.settings_ui.query_timeout_secs = (val.round() as u32).clamp(5, 600);
            cx.notify();
        }));

    Card::new()
        .child(
            CardHeader::new()
                .child(CardTitle::new().child("Session, Execution & Safety"))
                .child(
                    CardDescription::new()
                        .child("Controls startup restore behavior and Safe Mode guardrails."),
                ),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(12.0)))
                .child(restore_switch)
                .child(Separator::new())
                .child(safe_mode_switch)
                .child(Separator::new())
                .child(
                    div()
                        .flex()
                        .gap(px(12.0))
                        .child(div().flex_1().child(fetch_limit))
                        .child(div().flex_1().child(timeout_field)),
                ),
        )
        .into_any_element()
}

fn render_mcp_section(ws: &DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) -> AnyElement {
    let running = ws.controller.is_mcp_server_running();
    let live_addr = ws
        .controller
        .services
        .as_ref()
        .and_then(|s| s.mcp_server.lock().ok()?.address());
    let port = live_addr.map_or(ws.controller.settings.mcp.port, |a| a.port());
    let endpoint_url = format!("http://127.0.0.1:{port}{MCP_PATH}");
    let status_label = if running {
        format!("Running · {endpoint_url}")
    } else {
        "Stopped".to_owned()
    };

    let enable_switch = Switch::new("settings-mcp-enable")
        .is_selected(running)
        .is_disabled(ws.settings_ui.mcp_loading)
        .label("Enable Local Loopback MCP Server")
        .description(
            "Exposes active connections and schema inspection over authenticated loopback HTTP.",
        )
        .on_change(cx.listener(|ws, &enable: &bool, _w, cx| {
            toggle_mcp_with_vault_token(ws, enable, cx);
        }));

    let port_field = NumberField::new(&ws.settings_ui.mcp_port_state)
        .label("Loopback Port (127.0.0.1)")
        .min_value(1024.0)
        .max_value(65535.0)
        .step(1.0)
        .on_change(cx.listener(|ws, &val: &f64, _w, cx| {
            ws.controller.settings.mcp.port = (val.round() as u16).clamp(1024, u16::MAX);
            persist_settings(ws, cx);
        }));

    let token_card = render_mcp_token_and_snippets_card(ws, &endpoint_url, cx);

    let lifecycle_card = Card::new()
        .child(
            CardHeader::new()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(CardTitle::new().child("Model Context Protocol Endpoint"))
                        .child(
                            Chip::new()
                                .variant(ChipVariant::Soft)
                                .color(if running {
                                    HeroColor::Success
                                } else {
                                    HeroColor::Default
                                })
                                .size(HeroSize::Sm)
                                .child(ChipLabel::new().child(status_label)),
                        ),
                )
                .child(CardDescription::new().child("Local loopback endpoint strictly protected by Windows Credential Manager bearer token.")),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(12.0)))
                .child(enable_switch)
                .child(Separator::new())
                .child(div().w(px(220.0)).child(port_field)),
        );

    div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(lifecycle_card)
        .child(token_card)
        .into_any_element()
}

fn render_mcp_token_row(
    ws: &DataForgeWorkspace,
    has_token: bool,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let masked = match (
        ws.settings_ui.mcp_token.as_ref(),
        ws.settings_ui.reveal_mcp_token,
    ) {
        (Some(tok), true) => tok.clone(),
        (Some(_), false) => "tp_••••••••••••••••••••••••••••••••".to_owned(),
        (None, _) => "Unavailable (start MCP or refresh vault)".to_owned(),
    };
    let token_copied = ws.settings_ui.copied_key.as_deref() == Some("mcp-token");

    div()
        .p(px(8.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(muted_surface_bg(cx))
        .border_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .child(
            div()
                .min_w(px(0.0))
                .flex_1()
                .font(editor_gpui_font(&ws.controller.settings))
                .text_xs()
                .text_color(colors.foreground)
                .truncate()
                .child(masked),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(
                    Button::new("settings-mcp-reveal")
                        .variant(Variant::Ghost)
                        .size(HeroSize::Sm)
                        .is_icon_only(true)
                        .aria_label("Reveal or Hide Bearer Token")
                        .on_press(cx.listener(|ws, _, _w, cx| {
                            ws.settings_ui.reveal_mcp_token = !ws.settings_ui.reveal_mcp_token;
                            cx.notify();
                        }))
                        .child(
                            Icon::new(if ws.settings_ui.reveal_mcp_token {
                                IconName::EyeOff
                            } else {
                                IconName::Eye
                            })
                            .size(IconSize::Xs),
                        ),
                )
                .child(copy_chip_button(
                    "settings-mcp-copy-token",
                    "Copy Token",
                    token_copied,
                    has_token,
                    cx.listener(|ws, _, _w, cx| {
                        if let Some(tok) = ws.settings_ui.mcp_token.clone() {
                            ws.settings_ui
                                .copy_with_feedback("mcp-token".to_owned(), tok, cx);
                        }
                    }),
                    cx,
                ))
                .child(
                    Button::new("settings-mcp-rotate")
                        .variant(Variant::DangerSoft)
                        .size(HeroSize::Sm)
                        .on_press(cx.listener(|ws, _, _w, cx| {
                            rotate_mcp_token(ws, cx);
                        }))
                        .child("Rotate"),
                ),
        )
        .into_any_element()
}

fn render_mcp_token_and_snippets_card(
    ws: &DataForgeWorkspace,
    endpoint_url: &str,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let raw_token = ws.settings_ui.mcp_token.clone();
    let claude_copied = ws.settings_ui.copied_key.as_deref() == Some("mcp-claude");
    let cursor_copied = ws.settings_ui.copied_key.as_deref() == Some("mcp-cursor");
    let has_token = raw_token.is_some();
    let snippet_token = raw_token.unwrap_or_else(|| "<MCP_BEARER_TOKEN>".to_owned());
    let claude_json = build_claude_desktop_snippet(endpoint_url, &snippet_token);
    let cursor_json = build_cursor_snippet(endpoint_url, &snippet_token);
    let claude_preview = claude_json.clone();

    Card::new()
        .child(
            CardHeader::new()
                .child(CardTitle::new().child("Bearer Token & IDE Integration"))
                .child(CardDescription::new().child(
                    "Copy ready-to-paste client configs computed from live port and keyring token.",
                )),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(10.0)))
                .child(render_mcp_token_row(ws, has_token, cx))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(copy_chip_button(
                            "settings-mcp-copy-claude",
                            "Copy Claude Desktop Config",
                            claude_copied,
                            true,
                            cx.listener(move |ws, _, _w, cx| {
                                ws.settings_ui.copy_with_feedback(
                                    "mcp-claude".to_owned(),
                                    claude_json.clone(),
                                    cx,
                                );
                            }),
                            cx,
                        ))
                        .child(copy_chip_button(
                            "settings-mcp-copy-cursor",
                            "Copy Cursor Config",
                            cursor_copied,
                            true,
                            cx.listener(move |ws, _, _w, cx| {
                                ws.settings_ui.copy_with_feedback(
                                    "mcp-cursor".to_owned(),
                                    cursor_json.clone(),
                                    cx,
                                );
                            }),
                            cx,
                        )),
                )
                .child(
                    div()
                        .p(px(10.0))
                        .rounded(px(CONTROL_RADIUS_PX))
                        .bg(ws.content_palette.editor_background)
                        .border_1()
                        .border_color(hairline_border_color(cx))
                        .font(editor_gpui_font(&ws.controller.settings))
                        .text_xs()
                        .text_color(ws.content_palette.editor_text)
                        .child(claude_preview),
                ),
        )
        .into_any_element()
}

fn render_diagnostics_logs_card(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let log_dir = resolve_log_dir();
    let log_display = log_dir
        .as_ref()
        .map_or_else(|| "Unavailable".to_owned(), |p| p.display().to_string());
    let log_copied = ws.settings_ui.copied_key.as_deref() == Some("diag-log");
    let log_copy_val = log_display.clone();
    let log_open_path = log_dir.clone();

    Card::new()
        .child(
            CardHeader::new()
                .child(CardTitle::new().child("Logs & Crash Reports"))
                .child(
                    CardDescription::new()
                        .child("Structured tracing and panic crash dumps stored locally."),
                ),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(10.0)))
                .child(
                    div()
                        .px(px(10.0))
                        .py(px(8.0))
                        .rounded(px(CONTROL_RADIUS_PX))
                        .bg(muted_surface_bg(cx))
                        .border_1()
                        .border_color(hairline_border_color(cx))
                        .font(editor_gpui_font(&ws.controller.settings))
                        .text_xs()
                        .text_color(colors.foreground)
                        .child(log_display),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            Button::new("settings-open-log-folder")
                                .variant(Variant::Primary)
                                .size(HeroSize::Sm)
                                .is_disabled(log_open_path.is_none())
                                .on_press(cx.listener(move |_ws, _, _w, cx| {
                                    if let Some(dir) = log_open_path.as_ref() {
                                        if let Err(err) = std::fs::create_dir_all(dir) {
                                            push_error_toast(
                                                "Failed to prepare log folder",
                                                Some(SharedString::from(err.to_string())),
                                                cx,
                                            );
                                            return;
                                        }
                                        cx.open_with_system(dir);
                                    }
                                }))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(Icon::new(IconName::FolderOpen).size(IconSize::Xs))
                                        .child("Open Log Folder"),
                                ),
                        )
                        .child(copy_chip_button(
                            "settings-copy-log-path",
                            "Copy Path",
                            log_copied,
                            log_dir.is_some(),
                            cx.listener(move |ws, _, _w, cx| {
                                ws.settings_ui.copy_with_feedback(
                                    "diag-log".to_owned(),
                                    log_copy_val.clone(),
                                    cx,
                                );
                            }),
                            cx,
                        )),
                ),
        )
        .into_any_element()
}

fn render_diagnostics_section(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let engine_ver = if ws.controller.toolbar_status.engine_version.is_empty() {
        "Not connected".to_owned()
    } else {
        ws.controller.toolbar_status.engine_version.clone()
    };
    let host_label = if ws.controller.toolbar_status.host.is_empty() {
        "None".to_owned()
    } else {
        ws.controller.toolbar_status.host.clone()
    };

    let versions_card = Card::new()
        .child(
            CardHeader::new()
                .child(CardTitle::new().child("Build & Connected Runtime"))
                .child(
                    CardDescription::new()
                        .child("Genuine binary metadata and active session telemetry."),
                ),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(8.0)))
                .child(diag_kv_row("DataForge App", env!("CARGO_PKG_VERSION"), cx))
                .child(Separator::new())
                .child(diag_kv_row(
                    "Platform",
                    &format!("{} ({})", std::env::consts::OS, std::env::consts::ARCH),
                    cx,
                ))
                .child(Separator::new())
                .child(diag_kv_row("Active Engine", &engine_ver, cx))
                .child(Separator::new())
                .child(diag_kv_row("Active Endpoint", &host_label, cx))
                .child(Separator::new())
                .child(diag_kv_row(
                    "Pending Cell Edits",
                    &ws.controller
                        .grid_model
                        .edits
                        .modified_cells
                        .len()
                        .to_string(),
                    cx,
                )),
        );

    div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(render_diagnostics_logs_card(ws, cx))
        .child(versions_card)
        .into_any_element()
}

fn diag_kv_row(label: &str, value: &str, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .flex()
        .items_center()
        .justify_between()
        .text_xs()
        .child(div().text_color(colors.muted).child(label.to_owned()))
        .child(
            div()
                .font_weight(herogpui::gpui::FontWeight::MEDIUM)
                .text_color(colors.foreground)
                .child(value.to_owned()),
        )
        .into_any_element()
}

fn copy_chip_button(
    id: &'static str,
    label: &'static str,
    copied: bool,
    enabled: bool,
    handler: impl Fn(&herogpui::gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let colors = cx.colors().clone();
    Button::new(id)
        .variant(Variant::Secondary)
        .size(HeroSize::Sm)
        .is_disabled(!enabled)
        .on_press(handler)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(5.0))
                .child(
                    Icon::new(if copied {
                        IconName::Check
                    } else {
                        IconName::Copy
                    })
                    .size(IconSize::Xs)
                    .color(if copied {
                        colors.success.color
                    } else {
                        colors.foreground
                    }),
                )
                .child(if copied { "Copied" } else { label }),
        )
        .into_any_element()
}

fn persist_settings(ws: &mut DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) {
    match storage_paths() {
        Ok(paths) => {
            let path = AppSettings::file_path(&paths);
            if let Err(err) = ws.controller.save_settings_to(&path) {
                push_error_toast(
                    "Failed to save settings",
                    Some(SharedString::from(err.to_string())),
                    cx,
                );
            } else {
                ws.settings_ui.dirty = false;
                ws.settings_view.status_notice = Some("Saved".to_owned());
            }
        }
        Err(err) => {
            push_error_toast(
                "Storage paths unavailable",
                Some(SharedString::from(err.to_string())),
                cx,
            );
        }
    }
    cx.notify();
}

fn apply_theme_preference(
    ws: &mut DataForgeWorkspace,
    pref: ThemePreference,
    window: &Window,
    cx: &mut Context<DataForgeWorkspace>,
) {
    ws.controller.settings.theme = pref;
    if ws.controller.content_theme.id.is_builtin() {
        let appearance = match pref {
            ThemePreference::Light => ThemeAppearance::Light,
            ThemePreference::Dark => ThemeAppearance::Dark,
            ThemePreference::HighContrast => ThemeAppearance::HighContrast,
            ThemePreference::System => {
                if ws.controller.system_theme.is_dark() {
                    ThemeAppearance::Dark
                } else {
                    ThemeAppearance::Light
                }
            }
        };
        ws.controller.content_theme = ThemeDefinition::default_for(appearance);
    }
    ws.refresh_theme(window, cx);
    persist_settings(ws, cx);
}

fn apply_builtin_content_theme(
    ws: &mut DataForgeWorkspace,
    id: &str,
    window: &Window,
    cx: &mut Context<DataForgeWorkspace>,
) {
    ws.controller.content_theme = match id {
        "builtin.light" => ThemeDefinition::builtin_light(),
        "builtin.high_contrast" => ThemeDefinition::builtin_high_contrast(),
        _ => ThemeDefinition::builtin_dark(),
    };
    ws.controller.last_theme_import = None;
    ws.settings_ui.theme_json_error = None;
    ws.refresh_theme(window, cx);
    persist_settings(ws, cx);
}

fn apply_imported_theme_bytes(
    ws: &mut DataForgeWorkspace,
    bytes: &[u8],
    window: &Window,
    cx: &mut Context<DataForgeWorkspace>,
) {
    match VsCodeThemeImporter::import_bytes(bytes) {
        Ok(def) => {
            let summary = format!("Imported '{}' ({} slots)", def.name, def.colors.len());
            ws.controller.content_theme = def;
            ws.controller.last_theme_import = Some(summary.clone());
            ws.settings_ui.theme_json_error = None;
            ws.refresh_theme(window, cx);
            push_success_toast("Theme applied", Some(SharedString::from(summary)), cx);
            cx.notify();
        }
        Err(err) => {
            ws.settings_ui.theme_json_error = Some(err.to_string());
            push_error_toast(
                "Invalid theme JSON",
                Some(SharedString::from(err.to_string())),
                cx,
            );
            cx.notify();
        }
    }
}

fn browse_vscode_theme_file(
    ws: &mut DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) {
    let rx = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Select VS Code Theme JSON".into()),
    });
    ws.settings_ui.picker_task = Some(cx.spawn(
        async move |this: WeakEntity<DataForgeWorkspace>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let Ok(bytes) = std::fs::read(&path) else {
                let _ = this.update(cx, |_, cx| {
                    push_error_toast("Could not read theme file", None, cx);
                });
                return;
            };
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let _ = this.update(cx, |ws, cx| {
                match VsCodeThemeImporter::import_bytes(&bytes) {
                    Ok(def) => {
                        let summary =
                            format!("Imported '{}' ({} slots)", def.name, def.colors.len());
                        ws.controller.content_theme = def;
                        ws.controller.last_theme_import = Some(summary.clone());
                        ws.settings_ui.theme_json_error = None;
                        ws.content_palette =
                            ContentThemePalette::from_definition(&ws.controller.content_theme);
                        ws.settings_ui.theme_json_input.update(cx, |input, cx| {
                            input.set_value(text);
                            cx.notify();
                        });
                        push_success_toast("Theme imported", Some(SharedString::from(summary)), cx);
                    }
                    Err(err) => {
                        ws.settings_ui.theme_json_error = Some(err.to_string());
                        push_error_toast(
                            "Invalid VS Code theme JSON",
                            Some(SharedString::from(err.to_string())),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        },
    ));
}

fn toggle_mcp_with_vault_token(
    ws: &mut DataForgeWorkspace,
    enable: bool,
    cx: &mut Context<DataForgeWorkspace>,
) {
    let Some(services) = ws.controller.services.clone() else {
        push_error_toast("Services unavailable", None, cx);
        return;
    };
    let port = ws.controller.settings.mcp.port;
    ws.settings_ui.mcp_loading = true;
    cx.notify();

    if !enable {
        if let Ok(mut guard) = services.mcp_server.lock() {
            guard.stop();
        }
        ws.controller.settings.mcp.enabled = false;
        ws.settings_ui.mcp_loading = false;
        persist_settings(ws, cx);
        push_success_toast("MCP Server stopped", None, cx);
        return;
    }

    let token_res = services
        .runtime
        .block_on(async { obtain_token(&*services.credentials).await });
    let token = match token_res {
        Ok(tok) => tok,
        Err(err) => {
            ws.settings_ui.mcp_loading = false;
            push_error_toast(
                "MCP credential vault error",
                Some(SharedString::from(err.to_string())),
                cx,
            );
            cx.notify();
            return;
        }
    };
    ws.settings_ui.mcp_token = Some(token.secret().expose_secret().to_owned());

    let start_res = services.runtime.block_on(async {
        let mut ctrl = {
            let Ok(mut guard) = services.mcp_server.lock() else {
                return Err("MCP controller lock poisoned".to_owned());
            };
            if !guard.is_running() {
                *guard = dataforge_services::McpServerController::new(Some(port));
            }
            std::mem::replace(
                &mut *guard,
                dataforge_services::McpServerController::new(Some(port)),
            )
        };
        let res = ctrl
            .start(
                &services.runtime,
                services.mcp_host.clone(),
                services.connection_store.clone(),
                token,
            )
            .await
            .map_err(|err| err.to_string());
        if let Ok(mut guard) = services.mcp_server.lock() {
            *guard = ctrl;
        }
        res
    });

    ws.settings_ui.mcp_loading = false;
    match start_res {
        Ok(addr) => {
            ws.controller.settings.mcp.enabled = true;
            ws.controller.settings.mcp.port = addr.port();
            persist_settings(ws, cx);
            push_success_toast(
                "MCP Server running",
                Some(SharedString::from(format!("http://{addr}{MCP_PATH}"))),
                cx,
            );
        }
        Err(err) => {
            push_error_toast(
                "Failed to start MCP Server",
                Some(SharedString::from(err)),
                cx,
            );
        }
    }
    cx.notify();
}

fn rotate_mcp_token(ws: &mut DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) {
    let Some(services) = ws.controller.services.clone() else {
        return;
    };
    let res = services
        .runtime
        .block_on(async { replace_token(&*services.credentials).await });
    match res {
        Ok(tok) => {
            ws.settings_ui.mcp_token = Some(tok.secret().expose_secret().to_owned());
            push_success_toast(
                "MCP bearer token rotated",
                ws.controller
                    .is_mcp_server_running()
                    .then(|| SharedString::from("Restart MCP Server to enforce the new token.")),
                cx,
            );
        }
        Err(err) => {
            push_error_toast(
                "Failed to rotate token",
                Some(SharedString::from(err.to_string())),
                cx,
            );
        }
    }
    cx.notify();
}

fn resolve_log_dir() -> Option<PathBuf> {
    storage_paths().ok().map(|p| p.state.join("logs"))
}

fn build_claude_desktop_snippet(url: &str, token: &str) -> String {
    format!(
        "{{\n  \"mcpServers\": {{\n    \"dataforge\": {{\n      \"url\": \"{url}\",\n      \"headers\": {{\n        \"Authorization\": \"Bearer {token}\"\n      }}\n    }}\n  }}\n}}"
    )
}

fn build_cursor_snippet(url: &str, token: &str) -> String {
    format!(
        "{{\n  \"mcpServers\": {{\n    \"dataforge\": {{\n      \"type\": \"streamable-http\",\n      \"url\": \"{url}\",\n      \"headers\": {{\n        \"Authorization\": \"Bearer {token}\"\n      }}\n    }}\n  }}\n}}"
    )
}

fn sync_number_if_diff(entity: &Entity<NumberState>, target: f64, cx: &mut App) {
    let current = entity.read(cx).value();
    if (current - target).abs() > 0.001 {
        entity.update(cx, |s, cx| {
            s.set_value(target, cx);
            cx.notify();
        });
    }
}

const fn category_subtitle(cat: SettingsCategory) -> &'static str {
    match cat {
        SettingsCategory::Appearance => "System light/dark/high-contrast & VS Code theme import",
        SettingsCategory::EditorAndFonts => "SQL monospace & Data Grid value fonts, Vim mode",
        SettingsCategory::BehaviorAndSafety => {
            "Startup restore, Safe Mode, fetch limits & timeouts"
        }
        SettingsCategory::McpServer => "Local HTTP MCP server, bearer token & client snippets",
        SettingsCategory::Diagnostics => "Log directory, app metadata & live connection telemetry",
    }
}

const fn theme_pref_key(pref: ThemePreference) -> &'static str {
    match pref {
        ThemePreference::System => "system",
        ThemePreference::Light => "light",
        ThemePreference::Dark => "dark",
        ThemePreference::HighContrast => "high-contrast",
    }
}

fn parse_theme_pref(key: &str) -> Option<ThemePreference> {
    match key {
        "system" => Some(ThemePreference::System),
        "light" => Some(ThemePreference::Light),
        "dark" => Some(ThemePreference::Dark),
        "high-contrast" => Some(ThemePreference::HighContrast),
        _ => None,
    }
}
