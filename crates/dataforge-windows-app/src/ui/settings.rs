//! Full-screen Apple System Settings-style Preferences screen covering Appearance
//! & VS Code Theme JSON import, separate Editor/Value fonts, Behavior & Safety,
//! live MCP Server lifecycle + IDE configs, and real Diagnostics.

use std::path::PathBuf;
use std::time::Duration;

use dataforge_state::theme::{ThemeAppearance, ThemeDefinition, ThemeSlot, VsCodeThemeImporter};
use dataforge_windows_platform::{
    AppSettings, ThemePreference, WindowBackdropPreference, detect_reduced_motion, storage_paths,
};
use herogpui::components::anim::{Curve, Edge, Motion, entering_from};
use herogpui::components::{
    Button, Card, CardContent, CardDescription, CardHeader, CardTitle, Chip, ChipLabel,
    ChipVariant, Icon, IconName, IconSize, InputState, NumberField, NumberState, PickerItem,
    ProgressBar, Scrollbar, Select, Separator, Switch, TextArea, Variant,
};
use herogpui::core::{Color as HeroColor, Size as HeroSize, with_alpha};
use herogpui::gpui::prelude::FluentBuilder as _;
use herogpui::gpui::{
    AnyElement, App, AppContext as _, AsyncApp, ClipboardItem, Context, Entity, FontWeight,
    ImageSource, InteractiveElement, IntoElement, ParentElement, PathPromptOptions, ScrollHandle,
    SharedString, StatefulInteractiveElement, Styled, Task, WeakEntity, Window, div, img, px,
};
use herogpui::theme::ActiveTheme;
use secrecy::ExposeSecret;
use tablepro_mcp::{DEFAULT_PORT, MCP_PATH, obtain_token, replace_token};

use super::DataForgeWorkspace;
use super::shell_state::{SettingsSection, push_error_toast, push_success_toast};
use super::syntax_bridge::styled_sql_block;
use super::theme::{
    CONTROL_RADIUS_PX, ContentThemePalette, HAIRLINE_BORDER_PX, MICRO_GAP_PX, editor_gpui_font,
    effective_duration_ms, hairline_border_color, is_reduced_motion, muted_surface_bg,
    panel_surface_bg, value_gpui_font,
};
use crate::app::ScreenMode;
use crate::i18n::{Locale, Strings};
use crate::updater::{InstallMode, UpdateState};

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
    About,
}

impl SettingsCategory {
    const ALL: [Self; 6] = [
        Self::Appearance,
        Self::EditorAndFonts,
        Self::BehaviorAndSafety,
        Self::McpServer,
        Self::Diagnostics,
        Self::About,
    ];

    const fn key(self) -> &'static str {
        match self {
            Self::Appearance => "appearance",
            Self::EditorAndFonts => "editor-fonts",
            Self::BehaviorAndSafety => "behavior-safety",
            Self::McpServer => "mcp-server",
            Self::Diagnostics => "diagnostics",
            Self::About => "about",
        }
    }

    const fn label(self, s: &Strings) -> &'static str {
        match self {
            Self::Appearance => s.cat_appearance,
            Self::EditorAndFonts => s.cat_editor_fonts,
            Self::BehaviorAndSafety => s.cat_behavior_safety,
            Self::McpServer => s.cat_mcp_server,
            Self::Diagnostics => s.cat_diagnostics,
            Self::About => s.cat_about,
        }
    }

    const fn icon(self) -> IconName {
        match self {
            Self::Appearance => IconName::Palette,
            Self::EditorAndFonts => IconName::Type,
            Self::BehaviorAndSafety => IconName::Shield,
            Self::McpServer => IconName::Server,
            Self::Diagnostics => IconName::Activity,
            Self::About => IconName::Info,
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
            Self::Appearance | Self::Diagnostics | Self::About => SettingsSection::Appearance,
            Self::EditorAndFonts => SettingsSection::Editor,
            Self::BehaviorAndSafety => SettingsSection::Shortcuts,
            Self::McpServer => SettingsSection::McpServer,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SettingsLayoutMetrics {
    sidebar_width: f32,
    content_max_width: f32,
    horizontal_padding: f32,
    columns: usize,
}

impl SettingsLayoutMetrics {
    fn from_viewport_width(viewport_width: f32) -> Self {
        let width = if viewport_width.is_finite() {
            viewport_width.max(320.0)
        } else {
            1280.0
        };
        let sidebar_width = if width < 920.0 {
            216.0
        } else if width < 1080.0 {
            230.0
        } else if width < 1500.0 {
            264.0
        } else {
            272.0
        };
        let horizontal_padding = if width < 1080.0 {
            20.0
        } else if width < 1440.0 {
            28.0
        } else {
            32.0
        };
        let available_body_width = (width - sidebar_width).max(0.0);
        let columns = if available_body_width >= 1020.0 { 2 } else { 1 };
        let content_max_width = if columns >= 2 { 1320.0 } else { 960.0 };
        Self {
            sidebar_width,
            content_max_width,
            horizontal_padding,
            columns,
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
    editor_line_height_state: Entity<NumberState>,
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
    updater_task: Option<Task<()>>,
    auto_prompted_version: Option<String>,
}

impl SettingsScreenState {
    /// Initializes Settings screen state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, cx: &mut Context<DataForgeWorkspace>) -> Self {
        let theme_json_input = cx.new(|cx| InputState::new(cx));
        let editor_size_state = cx.new(|cx| NumberState::new(cx, 13.0));
        let editor_line_height_state = cx.new(|cx| NumberState::new(cx, 1.4));
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
            editor_line_height_state,
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
            updater_task: None,
            auto_prompted_version: None,
        };
        state.load_mcp_token_async(cx);
        state.start_updater_bridge(cx);
        state
    }

    fn start_updater_bridge(&mut self, cx: &mut Context<DataForgeWorkspace>) {
        self.updater_task = Some(cx.spawn(
            async move |this: WeakEntity<DataForgeWorkspace>, cx: &mut AsyncApp| {
                let Ok(rx) = this.update(cx, |ws, cx| {
                    let rx = ws.controller.updater.subscribe();
                    let drained = ws.controller.updater.drain_events();
                    if drained {
                        maybe_open_update_modal_on_available(ws);
                    }
                    if ws.controller.trigger_auto_update_check() || drained {
                        cx.notify();
                    }
                    rx
                }) else {
                    return;
                };
                while rx.recv().await.is_ok() {
                    let Ok(()) = this.update(cx, |ws, cx| {
                        if ws.controller.updater.drain_events() {
                            maybe_open_update_modal_on_available(ws);
                            cx.notify();
                        }
                    }) else {
                        break;
                    };
                }
            },
        ));
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
        &ws.settings_ui.editor_line_height_state,
        f64::from(ws.controller.settings.editor_font.line_height),
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
    window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    sync_settings_from_controller(ws, cx);

    let rem_scale = (f32::from(window.rem_size()) / 16.0).clamp(0.75, 2.0);
    let viewport_width = f32::from(window.viewport_size().width) / rem_scale;
    let layout = SettingsLayoutMetrics::from_viewport_width(viewport_width);

    let header = render_top_header(ws, cx);
    let sidebar = render_sidebar(ws, layout.sidebar_width, cx);
    let body = render_category_body(ws, layout, cx);

    div()
        .id("workspace-settings-screen")
        .flex_1()
        .size_full()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .bg(panel_surface_bg(cx))
        .child(header)
        .child(Separator::new())
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
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
    let s = &ws.controller.strings;
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
                                .child(s.workspace),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(s.settings),
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
                        .child(s.save_settings),
                ),
        )
        .into_any_element()
}

fn render_sidebar_nav_item(
    cat: SettingsCategory,
    active_cat: SettingsCategory,
    strings: &Strings,
    nav_motion: Motion,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let is_active = cat == active_cat;
    let row_bg = if is_active {
        with_alpha(colors.accent.color, 0.13)
    } else {
        herogpui::gpui::transparent_black()
    };
    let row_border = if is_active {
        with_alpha(colors.accent.color, 0.24)
    } else {
        herogpui::gpui::transparent_black()
    };
    let badge_bg = if is_active {
        colors.accent.color
    } else {
        with_alpha(colors.foreground, 0.07)
    };
    let icon_fg = if is_active {
        colors.accent.foreground
    } else {
        colors.muted
    };
    let label_fg = if is_active {
        colors.foreground
    } else {
        colors.muted
    };
    let hover_bg = with_alpha(colors.foreground, 0.055);

    div()
        .id(SharedString::from(format!("settings-nav-{}", cat.key())))
        .h(px(34.0))
        .w_full()
        .px(px(8.0))
        .rounded(px(7.0))
        .bg(row_bg)
        .border_1()
        .border_color(row_border)
        .flex()
        .items_center()
        .gap(px(10.0))
        .cursor_pointer()
        .when(!is_active, |el| el.hover(move |s| s.bg(hover_bg)))
        .on_click(cx.listener(move |ws, _, _window, cx| {
            if ws.settings_ui.active_category != cat {
                ws.settings_ui.active_category = cat;
                let mapped = cat.to_section();
                ws.settings_view.section = mapped;
                ws.settings_ui.last_external_section = mapped;
                cx.notify();
            }
        }))
        .child(
            div()
                .w(px(22.0))
                .h(px(22.0))
                .rounded(px(5.0))
                .flex_shrink_0()
                .bg(badge_bg)
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(cat.icon()).size(IconSize::Xs).color(icon_fg)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_sm()
                .truncate()
                .font_weight(if is_active {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(label_fg)
                .child(cat.label(strings)),
        )
        .when(is_active, |el| {
            let indicator = div()
                .w(px(3.0))
                .h(px(14.0))
                .rounded(px(2.0))
                .bg(colors.accent.color);
            el.child(entering_from(
                indicator,
                SharedString::from(format!("settings-nav-pill-{}", cat.key())),
                Edge::Left,
                px(4.0),
                nav_motion,
                cx,
            ))
        })
        .into_any_element()
}

fn render_sidebar(
    ws: &DataForgeWorkspace,
    sidebar_width: f32,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let active_cat = ws.settings_ui.active_category;
    let nav_motion = Motion {
        ms: effective_duration_ms(140, cx),
        scale: 1.0,
        curve: Curve::OutFluid,
    };

    let mut list = div()
        .id("settings-category-list")
        .flex()
        .flex_col()
        .gap(px(3.0));

    for cat in SettingsCategory::ALL {
        if cat == SettingsCategory::About {
            list = list.child(div().py(px(4.0)).px(px(6.0)).child(Separator::new()));
        }
        list = list.child(render_sidebar_nav_item(
            cat,
            active_cat,
            &ws.controller.strings,
            nav_motion,
            cx,
        ));
    }

    div()
        .w(px(sidebar_width))
        .h_full()
        .flex_shrink_0()
        .p(px(10.0))
        .bg(muted_surface_bg(cx))
        .border_r(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .child(list)
        .into_any_element()
}

fn render_category_body(
    ws: &DataForgeWorkspace,
    layout: SettingsLayoutMetrics,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let scroll_handle = ws.settings_ui.content_scroll.clone();
    let category = ws.settings_ui.active_category;

    let section_el = match category {
        SettingsCategory::Appearance => render_appearance_section(ws, layout.columns, cx),
        SettingsCategory::EditorAndFonts => render_editor_fonts_section(ws, layout.columns, cx),
        SettingsCategory::BehaviorAndSafety => render_behavior_safety_section(ws, cx),
        SettingsCategory::McpServer => render_mcp_section(ws, layout.columns, cx),
        SettingsCategory::Diagnostics => render_diagnostics_section(ws, layout.columns, cx),
        SettingsCategory::About => render_about_section(ws, layout.columns, cx),
    };

    let s = ws.controller.strings.clone();
    let container = div()
        .w_full()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .gap(px(18.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(category.label(&s)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted)
                        .child(category_subtitle(category, &s)),
                ),
        )
        .child(section_el);

    let body_motion = Motion {
        ms: effective_duration_ms(220, cx),
        scale: 0.992,
        curve: Curve::OutFluid,
    };
    let animated_container = entering_from(
        container,
        SharedString::from(format!("settings-cat-enter-{}", category.key())),
        Edge::Bottom,
        px(10.0),
        body_motion,
        cx,
    );

    div()
        .relative()
        .flex_1()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .child(
            div()
                .id("settings-scroll-content")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&scroll_handle)
                .child(
                    div()
                        .w_full()
                        .px(px(layout.horizontal_padding))
                        .py(px(24.0))
                        .child(
                            div()
                                .w_full()
                                .max_w(px(layout.content_max_width))
                                .mx_auto()
                                .child(animated_container),
                        ),
                ),
        )
        .child(Scrollbar::new("settings-scrollbar", scroll_handle))
        .into_any_element()
}

fn render_two_column_grid(
    columns: usize,
    left: Vec<AnyElement>,
    right: Vec<AnyElement>,
) -> AnyElement {
    if columns >= 2 {
        div()
            .w_full()
            .flex()
            .flex_row()
            .items_start()
            .gap(px(16.0))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .gap(px(14.0))
                    .children(left),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .gap(px(14.0))
                    .children(right),
            )
            .into_any_element()
    } else {
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .children(left)
            .children(right)
            .into_any_element()
    }
}

fn settings_row(
    label: impl Into<SharedString>,
    description: Option<&'static str>,
    control: impl IntoElement,
    cx: &App,
) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .w_full()
        .min_w(px(0.0))
        .overflow_hidden()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap_x(px(16.0))
        .gap_y(px(8.0))
        .child(
            div()
                .flex_1()
                .min_w(px(160.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.foreground)
                        .child(label.into()),
                )
                .when_some(description, |el, desc| {
                    el.child(div().text_xs().text_color(colors.muted).child(desc))
                }),
        )
        .child(control)
        .into_any_element()
}

fn render_appearance_section(
    ws: &DataForgeWorkspace,
    columns: usize,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let appearance_card = render_appearance_card(ws, cx);
    let importer_card = render_vscode_theme_import_card(ws, cx);
    render_two_column_grid(columns, vec![appearance_card], vec![importer_card])
}

fn render_appearance_card(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let language_items: Vec<PickerItem> = Locale::ALL
        .into_iter()
        .map(|loc| PickerItem::new(loc.code(), loc.native_name()))
        .collect();
    let language_select = div().w(px(230.0)).max_w_full().flex_shrink_0().child(
        Select::new("settings-interface-language", language_items)
            .full_width(true)
            .value(Some(SharedString::from(ws.controller.locale.code())))
            .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _window, cx| {
                if let Some(next_locale) = key.as_deref().and_then(Locale::from_code) {
                    ws.controller.set_locale(next_locale);
                    persist_settings(ws, cx);
                }
            })),
    );

    let pref_key = theme_pref_key(ws.controller.settings.theme);
    let pref_items = vec![
        PickerItem::new("system", s.theme_pref_system),
        PickerItem::new("light", s.theme_pref_light),
        PickerItem::new("dark", s.theme_pref_dark),
        PickerItem::new("high-contrast", s.theme_pref_high_contrast),
    ];
    let pref_select = div().w(px(230.0)).max_w_full().flex_shrink_0().child(
        Select::new("settings-theme-pref", pref_items)
            .full_width(true)
            .value(Some(SharedString::from(pref_key)))
            .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, window, cx| {
                if let Some(next_pref) = key.as_deref().and_then(parse_theme_pref) {
                    apply_theme_preference(ws, next_pref, window, cx);
                }
            })),
    );

    let (backdrop_select, preset_select, syntax_select) = render_theme_and_blur_selects(ws, cx);

    Card::new()
        .sx(|c| c.w_full().overflow_hidden())
        .child(
            CardHeader::new()
                .child(CardTitle::new().child(s.theme_card_title))
                .child(CardDescription::new().child(s.theme_card_desc)),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(12.0)))
                .child(settings_row(
                    s.language_label,
                    Some(s.language_desc),
                    language_select,
                    cx,
                ))
                .child(Separator::new())
                .child(settings_row(
                    s.interface_appearance,
                    Some(s.interface_appearance_desc),
                    pref_select,
                    cx,
                ))
                .child(Separator::new())
                .child(settings_row(
                    s.window_backdrop,
                    Some(s.window_backdrop_desc),
                    backdrop_select,
                    cx,
                ))
                .child(Separator::new())
                .child(settings_row(
                    s.theme_preset_label,
                    Some(s.theme_preset_desc),
                    preset_select,
                    cx,
                ))
                .child(Separator::new())
                .child(settings_row(
                    s.syntax_palette_label,
                    Some(s.syntax_palette_desc),
                    syntax_select,
                    cx,
                )),
        )
        .into_any_element()
}

fn render_theme_and_blur_selects(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> (AnyElement, AnyElement, AnyElement) {
    let s = &ws.controller.strings;
    let backdrop_key = backdrop_pref_key(ws.controller.settings.window_backdrop);
    let backdrop_items = vec![
        PickerItem::new("opaque", s.backdrop_opaque),
        PickerItem::new("gaussian_blur", s.backdrop_gaussian_blur),
        PickerItem::new("mica", s.backdrop_mica),
        PickerItem::new("mica_alt", s.backdrop_mica_alt),
    ];
    let backdrop_select = div()
        .w(px(230.0))
        .max_w_full()
        .flex_shrink_0()
        .child(
            Select::new("settings-window-backdrop", backdrop_items)
                .full_width(true)
                .value(Some(SharedString::from(backdrop_key)))
                .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, window, cx| {
                    if let Some(next_backdrop) = key.as_deref().and_then(parse_backdrop_pref) {
                        apply_window_backdrop(ws, next_backdrop, window, cx);
                    }
                })),
        )
        .into_any_element();

    let all_presets = ThemeDefinition::builtin_presets();
    let preset_key = ws
        .controller
        .settings
        .content_theme_id
        .clone()
        .unwrap_or_else(|| ws.controller.content_theme.id.0.clone());
    let preset_items: Vec<PickerItem> = all_presets
        .iter()
        .map(|t| PickerItem::new(t.id.0.clone(), t.name.clone()))
        .collect();
    let preset_select = div()
        .w(px(230.0))
        .max_w_full()
        .flex_shrink_0()
        .child(
            Select::new("settings-content-preset", preset_items)
                .full_width(true)
                .value(Some(SharedString::from(preset_key)))
                .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, window, cx| {
                    if let Some(id) = key.as_deref() {
                        apply_builtin_content_theme(ws, id, window, cx);
                    }
                })),
        )
        .into_any_element();

    let syntax_key = ws
        .controller
        .settings
        .syntax_palette_id
        .clone()
        .unwrap_or_else(|| "match_theme".to_owned());
    let mut syntax_items = vec![PickerItem::new("match_theme", s.match_active_theme)];
    for t in &all_presets {
        syntax_items.push(PickerItem::new(t.id.0.clone(), t.name.clone()));
    }
    let syntax_select = div()
        .w(px(230.0))
        .max_w_full()
        .flex_shrink_0()
        .child(
            Select::new("settings-syntax-palette", syntax_items)
                .full_width(true)
                .value(Some(SharedString::from(syntax_key)))
                .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, window, cx| {
                    if let Some(id) = key.as_deref() {
                        apply_syntax_palette(ws, id, window, cx);
                    }
                })),
        )
        .into_any_element();

    (backdrop_select, preset_select, syntax_select)
}

fn render_vscode_theme_preview(ws: &DataForgeWorkspace, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &ws.controller.strings;
    let sql_preview = div()
        .w_full()
        .overflow_hidden()
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
                .child(s.vscode_preview_heading),
        )
        .child(sql_preview)
        .child(render_26_slot_swatches(&ws.content_palette, cx))
        .into_any_element()
}

fn render_vscode_theme_import_card(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let import_err = ws.settings_ui.theme_json_error.clone();
    let active_name = ws.controller.content_theme.name.clone();
    let active_slot_count = ws.controller.content_theme.colors.len();
    let last_import = ws.controller.last_theme_import.clone();

    let mut json_area = TextArea::new(&ws.settings_ui.theme_json_input)
        .rows(5)
        .font_family(editor_gpui_font(&ws.controller.settings).family)
        .full_width()
        .placeholder(s.vscode_import_placeholder);
    if let Some(err) = import_err {
        json_area = json_area.is_invalid(true).error_message(err);
    }

    let json_container = div()
        .w_full()
        .p(px(4.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(muted_surface_bg(cx))
        .border_1()
        .border_color(hairline_border_color(cx))
        .child(json_area);

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
                .child(s.vscode_load_json),
        );

    let apply_json_btn = Button::new("settings-apply-vscode-json")
        .variant(Variant::Primary)
        .size(HeroSize::Sm)
        .on_press(cx.listener(|ws, _, window, cx| {
            let raw = ws.settings_ui.theme_json_input.read(cx).value().to_owned();
            apply_imported_theme_bytes(ws, raw.as_bytes(), window, cx);
        }))
        .child(s.vscode_import_btn);

    let mut content = CardContent::new().sx(|d| d.gap(px(12.0)));
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
        .child(json_container)
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.0))
                .child(browse_btn)
                .child(apply_json_btn),
        )
        .child(Separator::new())
        .child(render_vscode_theme_preview(ws, cx));

    Card::new()
        .sx(|c| c.w_full().overflow_hidden())
        .child(
            CardHeader::new()
                .child(CardTitle::new().child(s.vscode_import_title))
                .child(
                    CardDescription::new().child(
                        s.theme_slots_summary_tpl
                            .replace("{name}", &active_name)
                            .replace("{count}", &active_slot_count.to_string()),
                    ),
                ),
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
    columns: usize,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let editor_font_card = render_editor_font_card(ws, cx);
    let value_font_card = render_value_font_card(ws, cx);
    let editor_behavior_card = render_editor_behavior_card(ws, cx);

    render_two_column_grid(
        columns,
        vec![editor_font_card],
        vec![value_font_card, editor_behavior_card],
    )
}

const EDITOR_FONT_FAMILIES: [&str; 5] = [
    "Cascadia Mono",
    "Cascadia Code",
    "Consolas",
    "JetBrains Mono",
    "Fira Code",
];

fn render_editor_font_card(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let spec = &ws.controller.settings.editor_font;
    let family_items: Vec<PickerItem> = EDITOR_FONT_FAMILIES
        .into_iter()
        .map(|f| PickerItem::new(f, f))
        .collect();
    let family_select = div().w(px(230.0)).max_w_full().flex_shrink_0().child(
        Select::new("settings-editor-font-family", family_items)
            .full_width(true)
            .value(Some(SharedString::from(spec.family.clone())))
            .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _w, cx| {
                if let Some(fam) = key.as_ref() {
                    ws.controller.settings.editor_font.family = fam.to_string();
                    persist_settings(ws, cx);
                }
            })),
    );

    let size_field = div()
        .w(px(156.0))
        .max_w_full()
        .flex_shrink_0()
        .overflow_hidden()
        .child(
            NumberField::new(&ws.settings_ui.editor_size_state)
                .full_width(true)
                .min_value(9.0)
                .max_value(28.0)
                .step(0.5)
                .on_change(cx.listener(|ws, &val: &f64, _w, cx| {
                    ws.controller.settings.editor_font.size = (val as f32).clamp(9.0, 28.0);
                    persist_settings(ws, cx);
                })),
        );

    let line_height_field = div()
        .w(px(156.0))
        .max_w_full()
        .flex_shrink_0()
        .overflow_hidden()
        .child(
            NumberField::new(&ws.settings_ui.editor_line_height_state)
                .full_width(true)
                .min_value(1.0)
                .max_value(2.2)
                .step(0.05)
                .on_change(cx.listener(|ws, &val: &f64, _w, cx| {
                    ws.controller.settings.editor_font.line_height = (val as f32).clamp(1.0, 2.2);
                    persist_settings(ws, cx);
                })),
        );

    let ligatures_switch = Switch::new("settings-editor-ligatures")
        .is_selected(spec.ligatures)
        .on_change(cx.listener(|ws, &enabled: &bool, _w, cx| {
            ws.controller.settings.editor_font.ligatures = enabled;
            persist_settings(ws, cx);
        }));

    let s = &ws.controller.strings;
    Card::new()
        .sx(|c| c.w_full().overflow_hidden())
        .child(
            CardHeader::new()
                .child(CardTitle::new().child(s.editor_font_title))
                .child(CardDescription::new().child(s.editor_font_desc)),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(12.0)))
                .child(settings_row(s.sql_font_family, None, family_select, cx))
                .child(Separator::new())
                .child(settings_row(
                    s.font_size_pt,
                    Some("9.0 – 28.0 pt."),
                    size_field,
                    cx,
                ))
                .child(Separator::new())
                .child(settings_row(
                    s.line_height_label,
                    Some("1.00 – 2.20."),
                    line_height_field,
                    cx,
                ))
                .child(Separator::new())
                .child(settings_row(
                    s.ligatures_label,
                    Some(s.ligatures_desc),
                    ligatures_switch,
                    cx,
                )),
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
    let family_select = div().w(px(230.0)).max_w_full().flex_shrink_0().child(
        Select::new("settings-value-font-family", family_items)
            .full_width(true)
            .value(Some(SharedString::from(spec.family.clone())))
            .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _w, cx| {
                if let Some(fam) = key.as_ref() {
                    ws.controller.settings.value_font.family = fam.to_string();
                    persist_settings(ws, cx);
                }
            })),
    );

    let size_field = div()
        .w(px(156.0))
        .max_w_full()
        .flex_shrink_0()
        .overflow_hidden()
        .child(
            NumberField::new(&ws.settings_ui.value_size_state)
                .full_width(true)
                .min_value(9.0)
                .max_value(24.0)
                .step(0.5)
                .on_change(cx.listener(|ws, &val: &f64, _w, cx| {
                    ws.controller.settings.value_font.size = (val as f32).clamp(9.0, 24.0);
                    persist_settings(ws, cx);
                })),
        );

    let sample_text = div()
        .w_full()
        .overflow_hidden()
        .px(px(10.0))
        .py(px(8.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(muted_surface_bg(cx))
        .border_1()
        .border_color(hairline_border_color(cx))
        .font(value_gpui_font(&ws.controller.settings))
        .text_xs()
        .child("1,048,576  |  2026-11-14 01:05:12.409  |  order_uuid_7f9c");

    let s = &ws.controller.strings;
    Card::new()
        .sx(|c| c.w_full().overflow_hidden())
        .child(
            CardHeader::new()
                .child(CardTitle::new().child(s.value_font_title))
                .child(CardDescription::new().child(s.value_font_desc)),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(12.0)))
                .child(settings_row(s.value_font_family, None, family_select, cx))
                .child(Separator::new())
                .child(settings_row(
                    s.value_size_pt,
                    Some("9.0 – 24.0 pt."),
                    size_field,
                    cx,
                ))
                .child(sample_text),
        )
        .into_any_element()
}

fn render_editor_behavior_card(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let vim_on = ws.controller.vim_mode.is_some();
    let reduced_motion = detect_reduced_motion();

    let vim_switch = Switch::new("settings-vim-mode")
        .is_selected(vim_on)
        .on_change(cx.listener(|ws, &want_on: &bool, _w, cx| {
            if (ws.controller.vim_mode.is_some()) != want_on {
                ws.controller.toggle_vim_mode();
            }
            cx.notify();
        }));

    let motion_chip = Chip::new()
        .variant(ChipVariant::Soft)
        .color(if reduced_motion {
            HeroColor::Accent
        } else {
            HeroColor::Default
        })
        .size(HeroSize::Sm)
        .child(ChipLabel::new().child(if reduced_motion {
            s.reduced_motion_active
        } else {
            s.standard_motion
        }));

    Card::new()
        .sx(|c| c.w_full().overflow_hidden())
        .child(
            CardHeader::new()
                .child(CardTitle::new().child(s.editing_access_title))
                .child(CardDescription::new().child(s.editing_access_desc)),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(12.0)))
                .child(settings_row(
                    s.vim_keybindings,
                    Some(s.vim_keybindings_desc),
                    vim_switch,
                    cx,
                ))
                .child(Separator::new())
                .child(settings_row(
                    s.reduced_motion_label,
                    Some(s.reduced_motion_desc),
                    motion_chip,
                    cx,
                )),
        )
        .into_any_element()
}

fn render_behavior_safety_section(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let restore_switch = Switch::new("settings-restore-session")
        .is_selected(ws.controller.settings.restore_session)
        .on_change(cx.listener(|ws, &val: &bool, _w, cx| {
            ws.controller.settings.restore_session = val;
            persist_settings(ws, cx);
        }));

    let restore_last_session_switch = Switch::new("settings-restore-last-session")
        .is_selected(ws.controller.settings.restore_last_session)
        .on_change(cx.listener(|ws, &val: &bool, _w, cx| {
            ws.controller.settings.restore_last_session = val;
            persist_settings(ws, cx);
        }));

    let safe_mode_switch = Switch::new("settings-safe-mode")
        .is_selected(ws.controller.safe_mode_enabled)
        .on_change(cx.listener(|ws, &val: &bool, _w, cx| {
            ws.controller.safe_mode_enabled = val;
            cx.notify();
        }));

    let (fetch_limit, timeout_field) = render_behavior_number_fields(ws, cx);

    Card::new()
        .sx(|c| c.w_full().overflow_hidden())
        .child(
            CardHeader::new()
                .child(CardTitle::new().child(s.behavior_card_title))
                .child(CardDescription::new().child(s.behavior_card_desc)),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(12.0)))
                .child(settings_row(
                    s.restore_tabs_label,
                    Some(s.restore_tabs_desc),
                    restore_switch,
                    cx,
                ))
                .child(Separator::new())
                .child(settings_row(
                    s.settings_restore_session,
                    Some(s.settings_restore_session_desc),
                    restore_last_session_switch,
                    cx,
                ))
                .child(Separator::new())
                .child(settings_row(
                    s.safe_mode_prompt_label,
                    Some(s.safe_mode_prompt_desc),
                    safe_mode_switch,
                    cx,
                ))
                .child(Separator::new())
                .child(settings_row(
                    s.fetch_limit_label,
                    Some(s.fetch_limit_desc),
                    fetch_limit,
                    cx,
                ))
                .child(Separator::new())
                .child(settings_row(
                    s.query_timeout_label,
                    Some(s.query_timeout_desc),
                    timeout_field,
                    cx,
                )),
        )
        .into_any_element()
}

fn render_behavior_number_fields(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> (AnyElement, AnyElement) {
    let fetch_limit = div()
        .w(px(156.0))
        .max_w_full()
        .flex_shrink_0()
        .overflow_hidden()
        .child(
            NumberField::new(&ws.settings_ui.fetch_limit_state)
                .full_width(true)
                .min_value(10.0)
                .max_value(10_000.0)
                .step(50.0)
                .on_change(cx.listener(|ws, &val: &f64, _w, cx| {
                    ws.controller.grid_model.page.limit = (val.round() as u64).clamp(10, 10_000);
                    cx.notify();
                })),
        )
        .into_any_element();

    let timeout_field = div()
        .w(px(156.0))
        .max_w_full()
        .flex_shrink_0()
        .overflow_hidden()
        .child(
            NumberField::new(&ws.settings_ui.query_timeout_state)
                .full_width(true)
                .min_value(5.0)
                .max_value(600.0)
                .step(5.0)
                .on_change(cx.listener(|ws, &val: &f64, _w, cx| {
                    ws.settings_ui.query_timeout_secs = (val.round() as u32).clamp(5, 600);
                    cx.notify();
                })),
        )
        .into_any_element();

    (fetch_limit, timeout_field)
}

fn render_mcp_section(
    ws: &DataForgeWorkspace,
    columns: usize,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let running = ws.controller.is_mcp_server_running();
    let live_addr = ws
        .controller
        .services
        .as_ref()
        .and_then(|s| s.mcp_server.lock().ok()?.address());
    let port = live_addr.map_or(ws.controller.settings.mcp.port, |a| a.port());
    let endpoint_url = format!("http://127.0.0.1:{port}{MCP_PATH}");
    let s = &ws.controller.strings;
    let status_label = if running {
        format!("{} · {endpoint_url}", s.mcp_running_prefix)
    } else {
        s.mcp_stopped.to_owned()
    };

    let lifecycle_card = render_mcp_lifecycle_card(ws, running, &status_label, cx);
    let token_card = render_mcp_token_and_snippets_card(ws, &endpoint_url, cx);

    render_two_column_grid(columns, vec![lifecycle_card], vec![token_card])
}

fn render_mcp_lifecycle_card(
    ws: &DataForgeWorkspace,
    running: bool,
    status_label: &str,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let enable_switch = Switch::new("settings-mcp-enable")
        .is_selected(running)
        .is_disabled(ws.settings_ui.mcp_loading)
        .on_change(cx.listener(|ws, &enable: &bool, _w, cx| {
            toggle_mcp_with_vault_token(ws, enable, cx);
        }));

    let port_field = div()
        .w(px(156.0))
        .max_w_full()
        .flex_shrink_0()
        .overflow_hidden()
        .child(
            NumberField::new(&ws.settings_ui.mcp_port_state)
                .full_width(true)
                .min_value(1024.0)
                .max_value(65535.0)
                .step(1.0)
                .on_change(cx.listener(|ws, &val: &f64, _w, cx| {
                    ws.controller.settings.mcp.port = (val.round() as u16).clamp(1024, u16::MAX);
                    persist_settings(ws, cx);
                })),
        );

    let s = &ws.controller.strings;
    Card::new()
        .sx(|c| c.w_full().overflow_hidden())
        .child(
            CardHeader::new()
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .gap(px(8.0))
                        .child(CardTitle::new().child(s.mcp_card_title))
                        .child(
                            Chip::new()
                                .variant(ChipVariant::Soft)
                                .color(if running {
                                    HeroColor::Success
                                } else {
                                    HeroColor::Default
                                })
                                .size(HeroSize::Sm)
                                .child(ChipLabel::new().child(status_label.to_owned())),
                        ),
                )
                .child(CardDescription::new().child(s.mcp_card_desc)),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(12.0)))
                .child(settings_row(
                    s.mcp_enable_label,
                    Some(s.mcp_enable_desc),
                    enable_switch,
                    cx,
                ))
                .child(Separator::new())
                .child(settings_row(
                    s.mcp_port_label,
                    Some(s.mcp_port_desc),
                    port_field,
                    cx,
                )),
        )
        .into_any_element()
}

fn render_mcp_token_row(
    ws: &DataForgeWorkspace,
    has_token: bool,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &ws.controller.strings;
    let masked = match (
        ws.settings_ui.mcp_token.as_ref(),
        ws.settings_ui.reveal_mcp_token,
    ) {
        (Some(tok), true) => tok.clone(),
        (Some(_), false) => "tp_••••••••••••••••••••••••••••••••".to_owned(),
        (None, _) => s.mcp_token_unavailable.to_owned(),
    };
    let token_copied = ws.settings_ui.copied_key.as_deref() == Some("mcp-token");

    div()
        .w_full()
        .p(px(8.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(muted_surface_bg(cx))
        .border_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .child(
            div()
                .min_w(px(140.0))
                .flex_1()
                .font(editor_gpui_font(&ws.controller.settings))
                .text_xs()
                .text_color(colors.foreground)
                .truncate()
                .child(masked),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(
                    Button::new("settings-mcp-reveal")
                        .variant(Variant::Ghost)
                        .size(HeroSize::Sm)
                        .is_icon_only(true)
                        .aria_label(s.mcp_reveal_token_aria)
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
                    s.mcp_copy_token,
                    s.copied,
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
                        .child(s.mcp_rotate),
                ),
        )
        .into_any_element()
}

fn render_mcp_token_and_snippets_card(
    ws: &DataForgeWorkspace,
    endpoint_url: &str,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let raw_token = ws.settings_ui.mcp_token.clone();
    let claude_copied = ws.settings_ui.copied_key.as_deref() == Some("mcp-claude");
    let cursor_copied = ws.settings_ui.copied_key.as_deref() == Some("mcp-cursor");
    let has_token = raw_token.is_some();
    let snippet_token = raw_token.unwrap_or_else(|| "<MCP_BEARER_TOKEN>".to_owned());
    let claude_json = build_claude_desktop_snippet(endpoint_url, &snippet_token);
    let cursor_json = build_cursor_snippet(endpoint_url, &snippet_token);
    let claude_preview = claude_json.clone();

    Card::new()
        .sx(|c| c.w_full().overflow_hidden())
        .child(
            CardHeader::new()
                .child(CardTitle::new().child(s.mcp_tokens_title))
                .child(CardDescription::new().child(s.mcp_tokens_desc)),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(10.0)))
                .child(render_mcp_token_row(ws, has_token, cx))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.0))
                        .child(copy_chip_button(
                            "settings-mcp-copy-claude",
                            s.mcp_copy_claude,
                            s.copied,
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
                            s.mcp_copy_cursor,
                            s.copied,
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
                        .w_full()
                        .overflow_hidden()
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
    let s = &ws.controller.strings;
    let log_dir = resolve_log_dir();
    let log_display = log_dir
        .as_ref()
        .map_or_else(|| s.unavailable.to_owned(), |p| p.display().to_string());
    let log_copied = ws.settings_ui.copied_key.as_deref() == Some("diag-log");
    let log_copy_val = log_display.clone();
    let log_open_path = log_dir.clone();

    Card::new()
        .sx(|c| c.w_full().overflow_hidden())
        .child(
            CardHeader::new()
                .child(CardTitle::new().child(s.diag_logs_title))
                .child(CardDescription::new().child(s.diag_logs_desc)),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(10.0)))
                .child(
                    div()
                        .w_full()
                        .overflow_hidden()
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
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            Button::new("settings-open-log-folder")
                                .variant(Variant::Primary)
                                .size(HeroSize::Sm)
                                .is_disabled(log_open_path.is_none())
                                .on_press(cx.listener(move |ws, _, _w, cx| {
                                    if let Some(dir) = log_open_path.as_ref() {
                                        if let Err(err) = std::fs::create_dir_all(dir) {
                                            push_error_toast(
                                                ws.controller.strings.diag_prepare_logs_failed,
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
                                        .child(s.diag_open_logs),
                                ),
                        )
                        .child(copy_chip_button(
                            "settings-copy-log-path",
                            s.diag_copy_path,
                            s.copied,
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
    columns: usize,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let engine_ver = if ws.controller.toolbar_status.engine_version.is_empty() {
        s.not_connected.to_owned()
    } else {
        ws.controller.toolbar_status.engine_version.clone()
    };
    let host_label = if ws.controller.toolbar_status.host.is_empty() {
        s.none_label.to_owned()
    } else {
        ws.controller.toolbar_status.host.clone()
    };

    let logs_card = render_diagnostics_logs_card(ws, cx);
    let versions_card = Card::new()
        .sx(|c| c.w_full().overflow_hidden())
        .child(
            CardHeader::new()
                .child(CardTitle::new().child(s.diag_runtime_title))
                .child(CardDescription::new().child(s.diag_runtime_desc)),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(8.0)))
                .child(diag_kv_row(s.diag_app_label, env!("CARGO_PKG_VERSION"), cx))
                .child(Separator::new())
                .child(diag_kv_row(
                    s.diag_platform_label,
                    &format!("{} ({})", std::env::consts::OS, std::env::consts::ARCH),
                    cx,
                ))
                .child(Separator::new())
                .child(diag_kv_row(s.diag_engine_label, &engine_ver, cx))
                .child(Separator::new())
                .child(diag_kv_row(s.diag_endpoint_label, &host_label, cx))
                .child(Separator::new())
                .child(diag_kv_row(
                    s.diag_pending_edits_label,
                    &ws.controller
                        .grid_model
                        .edits
                        .modified_cells
                        .len()
                        .to_string(),
                    cx,
                )),
        )
        .into_any_element();

    render_two_column_grid(columns, vec![logs_card], vec![versions_card])
}

fn render_about_hero_card(version: &str, strings: &Strings, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    Card::new()
        .sx(|c| c.w_full().overflow_hidden())
        .child(
            CardContent::new().sx(|d| d.py(px(18.0))).child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(18.0))
                    .child(
                        img(ImageSource::Image(crate::branding::dataforge_logo_image()))
                            .w(px(64.0))
                            .h(px(64.0))
                            .flex_shrink_0(),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .text_lg()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(colors.foreground)
                                            .child(strings.app_title),
                                    )
                                    .child(
                                        Chip::new()
                                            .variant(ChipVariant::Soft)
                                            .color(HeroColor::Accent)
                                            .size(HeroSize::Sm)
                                            .child(ChipLabel::new().child(format!("v{version}"))),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.muted)
                                    .child(strings.about_tagline),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(with_alpha(colors.muted, 0.8))
                                    .child(strings.about_sub_tagline),
                            ),
                    ),
            ),
        )
        .into_any_element()
}

fn render_about_section(
    ws: &DataForgeWorkspace,
    columns: usize,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let version = env!("CARGO_PKG_VERSION");
    let hero_card = render_about_hero_card(version, s, cx);
    let updates_card = render_about_updates_card(ws, cx);
    let security_card = render_about_security_card(s, cx);
    let license_card = render_about_license_card(ws, version, cx);
    let runtime_card = render_about_runtime_card(ws, version, cx);

    let cards_grid = if columns >= 2 {
        render_two_column_grid(
            columns,
            vec![updates_card, runtime_card],
            vec![security_card, license_card],
        )
    } else {
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(updates_card)
            .child(security_card)
            .child(license_card)
            .child(runtime_card)
            .into_any_element()
    };

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(hero_card)
        .child(cards_grid)
        .into_any_element()
}

fn render_about_runtime_card(
    ws: &DataForgeWorkspace,
    version: &str,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let os_arch = format!("{} ({})", std::env::consts::OS, std::env::consts::ARCH);
    let ui_stack = "Rust + HeroGPUI (Direct3D 11)";
    let motion_label = if is_reduced_motion(cx) {
        s.reduced_motion_active
    } else {
        s.standard_motion
    };
    let theme_name = ws.controller.content_theme.name.clone();
    let sys_info_payload = format!(
        "{} v{version}\n{}: {os_arch}\n{}: {ui_stack}\n{}: {motion_label}\n{}: {} ({})\n{}: {theme_name}",
        s.app_title,
        s.diag_platform_label,
        s.about_ui_stack,
        s.about_motion_pref,
        s.about_locale_label,
        ws.controller.locale.native_name(),
        ws.controller.locale.code(),
        s.about_content_theme_label,
    );
    let sys_info_copied = ws.settings_ui.copied_key.as_deref() == Some("about-sys-info");

    Card::new()
        .sx(|c| c.w_full().overflow_hidden())
        .child(
            CardHeader::new()
                .child(CardTitle::new().child(s.about_runtime_title))
                .child(CardDescription::new().child(s.about_runtime_desc)),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(10.0)))
                .child(diag_kv_row(s.about_version, version, cx))
                .child(Separator::new())
                .child(diag_kv_row(s.about_arch, &os_arch, cx))
                .child(Separator::new())
                .child(diag_kv_row(s.about_ui_stack, ui_stack, cx))
                .child(Separator::new())
                .child(diag_kv_row(s.about_motion_pref, motion_label, cx))
                .child(Separator::new())
                .child(diag_kv_row(
                    s.language_label,
                    ws.controller.locale.native_name(),
                    cx,
                ))
                .child(Separator::new())
                .child(div().pt(px(2.0)).flex().items_center().justify_end().child(
                    copy_chip_button(
                        "settings-about-copy-info",
                        s.about_copy_sys_info,
                        s.copied,
                        sys_info_copied,
                        true,
                        cx.listener(move |ws, _, _w, cx| {
                            ws.settings_ui.copy_with_feedback(
                                "about-sys-info".to_owned(),
                                sys_info_payload.clone(),
                                cx,
                            );
                        }),
                        cx,
                    ),
                )),
        )
        .into_any_element()
}

fn render_about_security_card(s: &Strings, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    Card::new()
        .sx(|c| c.w_full().overflow_hidden())
        .child(
            CardHeader::new()
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .gap(px(8.0))
                        .child(
                            div()
                                .min_w(px(0.0))
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .child(
                                    Icon::new(IconName::ShieldCheck)
                                        .size(IconSize::Sm)
                                        .color(colors.success.color),
                                )
                                .child(CardTitle::new().child(s.about_security_title)),
                        )
                        .child(
                            Chip::new()
                                .variant(ChipVariant::Soft)
                                .color(HeroColor::Success)
                                .size(HeroSize::Sm)
                                .child(ChipLabel::new().child("SECURITY.md")),
                        ),
                )
                .child(CardDescription::new().child(s.about_security_desc)),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(10.0)))
                .child(diag_kv_row(s.sec_ai_privacy_k, s.sec_ai_privacy_v, cx))
                .child(Separator::new())
                .child(diag_kv_row(s.sec_credentials_k, s.sec_credentials_v, cx))
                .child(Separator::new())
                .child(diag_kv_row(s.sec_safe_mode_k, s.sec_safe_mode_v, cx))
                .child(Separator::new())
                .child(diag_kv_row(s.sec_filter_k, s.sec_filter_v, cx))
                .child(Separator::new())
                .child(diag_kv_row(s.sec_disclosure_k, s.sec_disclosure_v, cx)),
        )
        .into_any_element()
}

fn render_about_license_card(
    ws: &DataForgeWorkspace,
    version: &str,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let license_payload = format!(
        "{} v{version}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}:\n- {}: {}\n- {}: {}\n- {}: {}\n- {}: {}\n- {}: {}",
        s.app_title,
        s.lic_app_k,
        s.lic_app_v,
        s.lic_upstream_k,
        s.lic_upstream_v,
        s.lic_coc_k,
        s.lic_coc_v,
        s.lic_dual_k,
        s.lic_dual_v,
        s.about_security_policy_label,
        s.sec_ai_privacy_k,
        s.sec_ai_privacy_v,
        s.sec_credentials_k,
        s.sec_credentials_v,
        s.sec_safe_mode_k,
        s.sec_safe_mode_v,
        s.sec_filter_k,
        s.sec_filter_v,
        s.sec_disclosure_k,
        s.sec_disclosure_v,
    );
    let license_copied = ws.settings_ui.copied_key.as_deref() == Some("about-license-info");

    Card::new()
        .sx(|c| c.w_full().overflow_hidden())
        .child(
            CardHeader::new()
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .gap(px(8.0))
                        .child(CardTitle::new().child(s.about_license_title))
                        .child(
                            Chip::new()
                                .variant(ChipVariant::Soft)
                                .color(HeroColor::Accent)
                                .size(HeroSize::Sm)
                                .child(ChipLabel::new().child("MIT & AGPL-3.0")),
                        ),
                )
                .child(CardDescription::new().child(s.about_license_desc)),
        )
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(10.0)))
                .child(diag_kv_row(s.lic_app_k, s.lic_app_v, cx))
                .child(Separator::new())
                .child(diag_kv_row(s.lic_upstream_k, s.lic_upstream_v, cx))
                .child(Separator::new())
                .child(diag_kv_row(s.lic_coc_k, s.lic_coc_v, cx))
                .child(Separator::new())
                .child(diag_kv_row(s.lic_dual_k, s.lic_dual_v, cx))
                .child(Separator::new())
                .child(div().pt(px(2.0)).flex().items_center().justify_end().child(
                    copy_chip_button(
                        "settings-about-copy-license",
                        s.about_copy_license_info,
                        s.copied,
                        license_copied,
                        true,
                        cx.listener(move |ws, _, _w, cx| {
                            ws.settings_ui.copy_with_feedback(
                                "about-license-info".to_owned(),
                                license_payload.clone(),
                                cx,
                            );
                        }),
                        cx,
                    ),
                )),
        )
        .into_any_element()
}

fn render_about_updates_card(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let version = env!("CARGO_PKG_VERSION");
    let install_mode = ws.controller.updater.install_mode();
    let update_state = ws.controller.updater.state().clone();

    let mode_label = match install_mode {
        InstallMode::VelopackManaged => s.updates_mode_velopack,
        InstallMode::Standalone => s.updates_mode_standalone,
    };

    let auto_check_switch = Switch::new("settings-auto-check-updates")
        .is_selected(ws.controller.settings.auto_check_updates)
        .on_change(cx.listener(|ws, &val: &bool, _w, cx| {
            ws.controller.settings.auto_check_updates = val;
            persist_settings(ws, cx);
        }));

    Card::new()
        .sx(|c| c.w_full().overflow_hidden())
        .child(render_updates_card_header(install_mode, mode_label, s, cx))
        .child(
            CardContent::new()
                .sx(|d| d.gap(px(12.0)))
                .child(diag_kv_row(s.about_version, &format!("v{version}"), cx))
                .child(Separator::new())
                .child(diag_kv_row(s.updates_install_mode_label, mode_label, cx))
                .child(Separator::new())
                .child(settings_row(
                    s.updates_auto_check_label,
                    Some(s.updates_auto_check_desc),
                    auto_check_switch,
                    cx,
                ))
                .child(Separator::new())
                .child(render_updates_status_block(&update_state, s, cx))
                .child(render_updates_actions(install_mode, &update_state, s, cx)),
        )
        .into_any_element()
}

fn render_updates_card_header(
    install_mode: InstallMode,
    mode_label: &'static str,
    s: &Strings,
    cx: &App,
) -> CardHeader {
    let colors = cx.colors().clone();
    let chip_color = match install_mode {
        InstallMode::VelopackManaged => HeroColor::Success,
        InstallMode::Standalone => HeroColor::Default,
    };
    CardHeader::new()
        .child(
            div()
                .w_full()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .min_w(px(0.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            Icon::new(IconName::CloudDownload)
                                .size(IconSize::Sm)
                                .color(colors.accent.color),
                        )
                        .child(CardTitle::new().child(s.about_updates_title)),
                )
                .child(
                    Chip::new()
                        .variant(ChipVariant::Soft)
                        .color(chip_color)
                        .size(HeroSize::Sm)
                        .child(ChipLabel::new().child(mode_label)),
                ),
        )
        .child(CardDescription::new().child(s.about_updates_desc))
}

fn update_status_text(state: &UpdateState, s: &Strings) -> (String, bool) {
    match state {
        UpdateState::Idle => (s.updates_status_idle.to_owned(), false),
        UpdateState::Checking => (s.updates_status_checking.to_owned(), false),
        UpdateState::UpToDate => (s.updates_status_up_to_date.to_owned(), false),
        UpdateState::Available { version, .. } => (
            s.updates_status_available_tpl.replace("{version}", version),
            false,
        ),
        UpdateState::Downloading { percent } => (
            s.updates_status_downloading_tpl
                .replace("{percent}", &percent.to_string()),
            false,
        ),
        UpdateState::ReadyToRestart { version } => (
            s.updates_status_ready_tpl.replace("{version}", version),
            false,
        ),
        UpdateState::StandaloneMode => (s.updates_status_standalone.to_owned(), false),
        UpdateState::Error(err) => (s.updates_status_error_tpl.replace("{error}", err), true),
    }
}

fn render_updates_status_block(state: &UpdateState, s: &Strings, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    let (status_text, is_error) = update_status_text(state, s);
    let status_color = match state {
        UpdateState::Available { .. } | UpdateState::ReadyToRestart { .. } => colors.accent.color,
        UpdateState::UpToDate => colors.success.color,
        UpdateState::Error(_) if is_error => colors.danger.color,
        _ => colors.muted,
    };

    let mut block = div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(diag_kv_row(s.updates_status_label, "", cx))
        .child(div().text_xs().text_color(status_color).child(status_text));

    match state {
        UpdateState::Checking => {
            block = block.child(
                ProgressBar::new("settings-update-checking-progress")
                    .is_indeterminate(true)
                    .size(HeroSize::Sm)
                    .color(HeroColor::Accent),
            );
        }
        UpdateState::Downloading { percent } => {
            block = block.child(
                ProgressBar::new("settings-update-download-progress")
                    .value(f32::from(*percent))
                    .min_value(0.0)
                    .max_value(100.0)
                    .size(HeroSize::Sm)
                    .color(HeroColor::Accent),
            );
        }
        UpdateState::Available { notes, .. } if !notes.trim().is_empty() => {
            block = block.child(render_release_notes_preview(notes, s, cx));
        }
        _ => {}
    }
    block.into_any_element()
}

fn render_release_notes_preview(notes: &str, s: &Strings, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.muted)
                .child(s.updates_release_notes_label),
        )
        .child(
            div()
                .id("settings-update-release-notes")
                .w_full()
                .max_h(px(140.0))
                .overflow_y_scroll()
                .rounded(px(CONTROL_RADIUS_PX))
                .border_1()
                .border_color(hairline_border_color(cx))
                .p(px(10.0))
                .text_xs()
                .text_color(colors.foreground)
                .child(notes.to_owned()),
        )
        .into_any_element()
}

fn maybe_open_update_modal_on_available(ws: &mut DataForgeWorkspace) {
    if let UpdateState::Available { version, .. } = ws.controller.updater.state().clone()
        && ws.settings_ui.auto_prompted_version.as_deref() != Some(version.as_str())
    {
        ws.settings_ui.auto_prompted_version = Some(version);
        ws.modals_ui.open_update_modal();
    }
}

fn render_updates_actions(
    install_mode: InstallMode,
    state: &UpdateState,
    s: &Strings,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let is_checking = matches!(state, UpdateState::Checking);
    let is_downloading = matches!(state, UpdateState::Downloading { .. });
    let is_standalone = install_mode == InstallMode::Standalone;

    let check_btn = Button::new("settings-update-check-btn")
        .variant(Variant::Secondary)
        .size(HeroSize::Sm)
        .is_disabled(is_standalone || is_checking || is_downloading)
        .on_press(cx.listener(|ws, _, _w, cx| {
            ws.settings_ui.auto_prompted_version = None;
            ws.controller.check_for_updates();
            cx.notify();
        }))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(Icon::new(IconName::RefreshCw).size(IconSize::Xs))
                .child(if is_checking {
                    s.updates_btn_checking
                } else {
                    s.updates_btn_check
                }),
        );

    let mut row = div()
        .pt(px(2.0))
        .flex()
        .flex_wrap()
        .items_center()
        .justify_end()
        .gap(px(8.0))
        .child(check_btn);

    if matches!(
        state,
        UpdateState::Available { .. }
            | UpdateState::Downloading { .. }
            | UpdateState::ReadyToRestart { .. }
            | UpdateState::Error(_)
    ) {
        let modal_btn = Button::new("settings-update-open-modal-btn")
            .variant(Variant::Secondary)
            .size(HeroSize::Sm)
            .on_press(cx.listener(|ws, _, _w, cx| {
                ws.modals_ui.open_update_modal();
                cx.notify();
            }))
            .child(s.update_modal_title);
        row = row.child(modal_btn);
    }

    append_updates_action_cta(row, state, is_downloading, s, cx).into_any_element()
}

fn append_updates_action_cta(
    mut row: herogpui::gpui::Div,
    state: &UpdateState,
    is_downloading: bool,
    s: &Strings,
    cx: &mut Context<DataForgeWorkspace>,
) -> herogpui::gpui::Div {
    if matches!(state, UpdateState::Available { .. }) || is_downloading {
        let download_btn = Button::new("settings-update-download-btn")
            .variant(Variant::Primary)
            .size(HeroSize::Sm)
            .is_disabled(is_downloading)
            .on_press(cx.listener(|ws, _, _w, cx| {
                ws.controller.download_update();
                ws.modals_ui.open_update_modal();
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(Icon::new(IconName::Download).size(IconSize::Xs))
                    .child(if is_downloading {
                        s.updates_btn_downloading
                    } else {
                        s.update_download_install
                    }),
            );
        row = row.child(download_btn);
    }

    if matches!(state, UpdateState::ReadyToRestart { .. }) {
        let restart_btn = Button::new("settings-update-restart-btn")
            .variant(Variant::Primary)
            .size(HeroSize::Sm)
            .on_press(cx.listener(|ws, _, _w, cx| {
                if let Err(err) = ws.controller.apply_update_and_restart() {
                    push_error_toast(
                        ws.controller.strings.updates_toast_apply_failed,
                        Some(SharedString::from(err)),
                        cx,
                    );
                    cx.notify();
                }
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(Icon::new(IconName::RotateCw).size(IconSize::Xs))
                    .child(s.update_apply_restart),
            );
        row = row.child(restart_btn);
    }

    row
}

fn diag_kv_row(label: &str, value: &str, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .w_full()
        .min_w(px(0.0))
        .flex()
        .flex_wrap()
        .items_baseline()
        .justify_between()
        .gap_x(px(16.0))
        .gap_y(px(2.0))
        .text_xs()
        .child(
            div()
                .flex_shrink_0()
                .text_color(colors.muted)
                .child(label.to_owned()),
        )
        .child(
            div()
                .min_w(px(0.0))
                .max_w_full()
                .font_weight(herogpui::gpui::FontWeight::MEDIUM)
                .text_color(colors.foreground)
                .child(value.to_owned()),
        )
        .into_any_element()
}

fn copy_chip_button(
    id: &'static str,
    label: &'static str,
    copied_label: &'static str,
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
                .child(if copied { copied_label } else { label }),
        )
        .into_any_element()
}

fn persist_settings(ws: &mut DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) {
    match storage_paths() {
        Ok(paths) => {
            let path = AppSettings::file_path(&paths);
            if let Err(err) = ws.controller.save_settings_to(&path) {
                push_error_toast(
                    ws.controller.strings.settings_save_failed,
                    Some(SharedString::from(err.to_string())),
                    cx,
                );
            } else {
                ws.settings_ui.dirty = false;
                ws.settings_view.status_notice =
                    Some(ws.controller.strings.settings_saved.to_owned());
            }
        }
        Err(err) => {
            push_error_toast(
                ws.controller.strings.storage_paths_unavailable,
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
    let mut base =
        ThemeDefinition::from_builtin_id(id).unwrap_or_else(ThemeDefinition::builtin_dark);
    // Sync chrome appearance & Gaussian blur automatically when selecting a theme preset.
    ws.controller.settings.theme = match base.appearance {
        ThemeAppearance::Light => ThemePreference::Light,
        ThemeAppearance::Dark => ThemePreference::Dark,
        ThemeAppearance::HighContrast => ThemePreference::HighContrast,
    };
    if id == "builtin.gaussian_dark" || id == "builtin.gaussian_light" {
        ws.controller.settings.window_backdrop = WindowBackdropPreference::GaussianBlur;
    }
    ws.controller.settings.content_theme_id = Some(id.to_owned());
    if let Some(syntax_id) = ws.controller.settings.syntax_palette_id.as_deref()
        && let Some(syntax_src) = ThemeDefinition::from_builtin_id(syntax_id)
    {
        base = base.with_syntax_from(&syntax_src);
    }
    ws.controller.content_theme = base;
    ws.controller.last_theme_import = None;
    ws.settings_ui.theme_json_error = None;
    ws.refresh_theme(window, cx);
    persist_settings(ws, cx);
}

fn apply_window_backdrop(
    ws: &mut DataForgeWorkspace,
    backdrop: WindowBackdropPreference,
    window: &Window,
    cx: &mut Context<DataForgeWorkspace>,
) {
    ws.controller.settings.window_backdrop = backdrop;
    ws.refresh_theme(window, cx);
    persist_settings(ws, cx);
}

fn apply_syntax_palette(
    ws: &mut DataForgeWorkspace,
    id: &str,
    window: &Window,
    cx: &mut Context<DataForgeWorkspace>,
) {
    if id == "match_theme" {
        ws.controller.settings.syntax_palette_id = None;
        let base_id = ws
            .controller
            .settings
            .content_theme_id
            .as_deref()
            .unwrap_or(&ws.controller.content_theme.id.0);
        if let Some(base) = ThemeDefinition::from_builtin_id(base_id) {
            ws.controller.content_theme = base;
        }
    } else if let Some(syntax_src) = ThemeDefinition::from_builtin_id(id) {
        ws.controller.settings.syntax_palette_id = Some(id.to_owned());
        ws.controller.content_theme = ws.controller.content_theme.with_syntax_from(&syntax_src);
    }
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
            let summary = ws
                .controller
                .strings
                .vscode_imported_summary_tpl
                .replace("{name}", &def.name)
                .replace("{count}", &def.colors.len().to_string());
            ws.controller.content_theme = def;
            ws.controller.last_theme_import = Some(summary.clone());
            ws.settings_ui.theme_json_error = None;
            ws.refresh_theme(window, cx);
            push_success_toast(
                ws.controller.strings.theme_applied_toast,
                Some(SharedString::from(summary)),
                cx,
            );
            cx.notify();
        }
        Err(err) => {
            ws.settings_ui.theme_json_error = Some(err.to_string());
            push_error_toast(
                ws.controller.strings.invalid_theme_json_toast,
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
        prompt: Some(ws.controller.strings.select_vscode_theme_prompt.into()),
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
                let _ = this.update(cx, |ws, cx| {
                    push_error_toast(ws.controller.strings.theme_file_read_failed_toast, None, cx);
                });
                return;
            };
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let _ = this.update(cx, |ws, cx| {
                match VsCodeThemeImporter::import_bytes(&bytes) {
                    Ok(def) => {
                        let summary = ws
                            .controller
                            .strings
                            .vscode_imported_summary_tpl
                            .replace("{name}", &def.name)
                            .replace("{count}", &def.colors.len().to_string());
                        ws.controller.content_theme = def;
                        ws.controller.last_theme_import = Some(summary.clone());
                        ws.settings_ui.theme_json_error = None;
                        ws.content_palette =
                            ContentThemePalette::from_definition(&ws.controller.content_theme);
                        ws.settings_ui.theme_json_input.update(cx, |input, cx| {
                            input.set_value(text);
                            cx.notify();
                        });
                        push_success_toast(
                            ws.controller.strings.theme_imported_toast,
                            Some(SharedString::from(summary)),
                            cx,
                        );
                    }
                    Err(err) => {
                        ws.settings_ui.theme_json_error = Some(err.to_string());
                        push_error_toast(
                            ws.controller.strings.invalid_vscode_theme_json_toast,
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
        push_error_toast(ws.controller.strings.services_unavailable_toast, None, cx);
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
        push_success_toast(ws.controller.strings.mcp_stopped_toast, None, cx);
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
                ws.controller.strings.mcp_vault_error_toast,
                Some(SharedString::from(err.to_string())),
                cx,
            );
            cx.notify();
            return;
        }
    };
    ws.settings_ui.mcp_token = Some(token.secret().expose_secret().to_owned());

    let lock_poisoned_msg = ws.controller.strings.mcp_lock_poisoned_err;
    let start_res = services.runtime.block_on(async {
        let mut ctrl = {
            let Ok(mut guard) = services.mcp_server.lock() else {
                return Err(lock_poisoned_msg.to_owned());
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
                ws.controller.strings.mcp_running_toast,
                Some(SharedString::from(format!("http://{addr}{MCP_PATH}"))),
                cx,
            );
        }
        Err(err) => {
            push_error_toast(
                ws.controller.strings.mcp_start_failed_toast,
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
                ws.controller.strings.mcp_token_rotated_toast,
                ws.controller
                    .is_mcp_server_running()
                    .then(|| SharedString::from(ws.controller.strings.mcp_restart_enforce_desc)),
                cx,
            );
        }
        Err(err) => {
            push_error_toast(
                ws.controller.strings.mcp_rotate_failed_toast,
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

const fn category_subtitle(cat: SettingsCategory, s: &Strings) -> &'static str {
    match cat {
        SettingsCategory::Appearance => s.cat_appearance_sub,
        SettingsCategory::EditorAndFonts => s.cat_editor_fonts_sub,
        SettingsCategory::BehaviorAndSafety => s.cat_behavior_safety_sub,
        SettingsCategory::McpServer => s.cat_mcp_server_sub,
        SettingsCategory::Diagnostics => s.cat_diagnostics_sub,
        SettingsCategory::About => s.cat_about_sub,
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

const fn backdrop_pref_key(pref: WindowBackdropPreference) -> &'static str {
    match pref {
        WindowBackdropPreference::Opaque => "opaque",
        WindowBackdropPreference::GaussianBlur => "gaussian_blur",
        WindowBackdropPreference::Mica => "mica",
        WindowBackdropPreference::MicaAlt => "mica_alt",
    }
}

fn parse_backdrop_pref(key: &str) -> Option<WindowBackdropPreference> {
    match key {
        "opaque" => Some(WindowBackdropPreference::Opaque),
        "gaussian_blur" => Some(WindowBackdropPreference::GaussianBlur),
        "mica" => Some(WindowBackdropPreference::Mica),
        "mica_alt" => Some(WindowBackdropPreference::MicaAlt),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::SettingsLayoutMetrics;

    #[test]
    fn narrow_viewport_collapses_to_single_column_with_compact_sidebar() {
        let m = SettingsLayoutMetrics::from_viewport_width(800.0);
        assert!((210.0..=232.0).contains(&m.sidebar_width));
        assert_eq!(m.columns, 1);
        assert_eq!(m.horizontal_padding, 20.0);
        assert!(m.content_max_width >= 760.0);
    }

    #[test]
    fn standard_viewport_gives_full_sidebar_and_single_column() {
        let m = SettingsLayoutMetrics::from_viewport_width(1280.0);
        assert!((252.0..=272.0).contains(&m.sidebar_width));
        assert!(1280.0 - m.sidebar_width < 1020.0);
        assert_eq!(m.columns, 1);
        assert_eq!(m.horizontal_padding, 28.0);
        assert!(m.content_max_width >= 900.0);
    }

    #[test]
    fn wide_viewport_enables_two_columns_and_generous_width() {
        let m = SettingsLayoutMetrics::from_viewport_width(1600.0);
        assert!((252.0..=272.0).contains(&m.sidebar_width));
        assert!(1600.0 - m.sidebar_width >= 1020.0);
        assert_eq!(m.columns, 2);
        assert_eq!(m.horizontal_padding, 32.0);
        assert!((1180.0..=1380.0).contains(&m.content_max_width));
    }

    #[test]
    fn ultrawide_viewport_caps_centered_content_width_and_uses_two_columns() {
        let m = SettingsLayoutMetrics::from_viewport_width(2560.0);
        assert!((252.0..=272.0).contains(&m.sidebar_width));
        assert_eq!(m.columns, 2);
        assert_eq!(m.horizontal_padding, 32.0);
        assert!((1180.0..=1380.0).contains(&m.content_max_width));
    }

    #[test]
    fn non_finite_viewport_falls_back_safely() {
        let m = SettingsLayoutMetrics::from_viewport_width(f32::NAN);
        assert!((252.0..=272.0).contains(&m.sidebar_width));
        assert_eq!(m.columns, 1);
    }
}
