//! Non-blocking `320px` right-side AI SQL Assistant drawer backed by
//! `AiAssistantService`, real Tokio streaming, and `syntax_bridge` SQL code blocks.

use std::collections::BTreeMap;
use std::time::Duration;

use herogpui::components::anim::{Curve, Edge, Motion, entering_from};
use herogpui::components::{
    Button, Card, CardContent, Chip, ChipLabel, ChipVariant, Icon, IconName, IconSize, Input,
    InputState, InputType, PickerItem, Scrollbar, Select, Separator, Spinner, TextArea, Variant,
};
use herogpui::core::{Color as HeroColor, Size as HeroSize, with_alpha};
use herogpui::gpui::prelude::FluentBuilder as _;
use herogpui::gpui::{
    AnyElement, App, AppContext as _, AsyncApp, ClipboardItem, Context, Entity, InteractiveElement,
    IntoElement, ParentElement, ScrollHandle, SharedString, StatefulInteractiveElement, Styled,
    Task, WeakEntity, Window, div, px,
};
use herogpui::theme::ActiveTheme;
use tableminus_services::{
    AiAssistantService, AiEndpointConfig, DEFAULT_ANTHROPIC_URL, DEFAULT_OLLAMA_URL,
    DEFAULT_OPENAI_URL,
};
use tableminus_state::tabs::TabConnectionId;
use tablepro_ai::{ChatMessage, QueryLanguage, ReplyEvent};
use tablepro_core::AiProvider;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::TableMinusWorkspace;
use super::shell_state::{WorkspaceSubView, push_error_toast, push_success_toast};
use super::syntax_bridge::styled_sql_block;
use super::theme::{
    COMPACT_GAP_PX, CONTROL_RADIUS_PX, HAIRLINE_BORDER_PX, MICRO_GAP_PX, drawer_duration,
    editor_gpui_font, hairline_border_color, muted_surface_bg, panel_surface_bg,
};
use crate::views::ai_drawer::AiMessage;

#[derive(Debug, Clone, PartialEq, Eq)]
enum MessageSegment {
    Prose(String),
    Sql(String),
}

/// Persistent UI state for the right-hand AI Assistant drawer.
pub struct AiDrawerViewState {
    pub scroll_to_bottom: bool,
    config: AiEndpointConfig,
    api_key_input: Entity<InputState>,
    prompt_input: Entity<InputState>,
    transcript_scroll: ScrollHandle,
    fetched_models: BTreeMap<&'static str, Vec<String>>,
    key_status: BTreeMap<&'static str, bool>,
    fetching_models: bool,
    config_expanded: bool,
    stream_cancel: Option<CancellationToken>,
    stream_task: Option<Task<()>>,
    model_task: Option<Task<()>>,
    key_probe_task: Option<Task<()>>,
    copied_block_key: Option<String>,
    copied_gen: u64,
    copy_reset_task: Option<Task<()>>,
    last_error: Option<String>,
}

impl AiDrawerViewState {
    /// Initializes AI drawer state owned by [`TableMinusWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, cx: &mut Context<TableMinusWorkspace>) -> Self {
        let api_key_input = cx.new(|cx| InputState::new(cx));
        let prompt_input = cx.new(|cx| InputState::new(cx));
        let mut state = Self {
            scroll_to_bottom: false,
            config: AiEndpointConfig::ollama("llama3.2"),
            api_key_input,
            prompt_input,
            transcript_scroll: ScrollHandle::new(),
            fetched_models: BTreeMap::new(),
            key_status: BTreeMap::new(),
            fetching_models: false,
            config_expanded: true,
            stream_cancel: None,
            stream_task: None,
            model_task: None,
            key_probe_task: None,
            copied_block_key: None,
            copied_gen: 0,
            copy_reset_task: None,
            last_error: None,
        };
        state.probe_stored_keys(cx);
        state
    }

    fn probe_stored_keys(&mut self, cx: &mut Context<TableMinusWorkspace>) {
        self.key_probe_task = Some(cx.spawn(
            async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
                let services = this
                    .read_with(cx, |ws, _| ws.controller.services.clone())
                    .ok()
                    .flatten();
                let Some(services) = services else {
                    return;
                };
                let Some(ai) = services.ai.clone() else {
                    return;
                };
                let runtime = services.runtime.clone();
                let statuses = runtime
                    .spawn(async move {
                        let mut out = Vec::new();
                        for provider in [AiProvider::OpenAi, AiProvider::Anthropic] {
                            let has_key = matches!(ai.load_api_key(provider).await, Ok(Some(_)));
                            out.push((provider.id(), has_key));
                        }
                        out
                    })
                    .await
                    .unwrap_or_default();
                let _ = this.update(cx, |ws, cx| {
                    for (id, present) in statuses {
                        ws.ai_drawer_ui.key_status.insert(id, present);
                    }
                    cx.notify();
                });
            },
        ));
    }

    fn copy_sql_with_feedback(
        &mut self,
        block_key: String,
        sql: String,
        cx: &mut Context<TableMinusWorkspace>,
    ) {
        cx.write_to_clipboard(ClipboardItem::new_string(sql));
        self.copied_gen = self.copied_gen.wrapping_add(1);
        let generation = self.copied_gen;
        self.copied_block_key = Some(block_key);
        self.copy_reset_task = Some(cx.spawn(
            async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
                cx.background_executor()
                    .timer(Duration::from_millis(1400))
                    .await;
                let _ = this.update(cx, |ws, cx| {
                    if ws.ai_drawer_ui.copied_gen == generation {
                        ws.ai_drawer_ui.copied_block_key = None;
                        cx.notify();
                    }
                });
            },
        ));
        cx.notify();
    }
}

/// Renders the non-blocking `320px` right-hand AI SQL Assistant drawer.
#[must_use]
pub fn render_ai_drawer(
    ws: &mut TableMinusWorkspace,
    _window: &mut Window,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let header = render_header(ws, cx);
    let privacy_bar = render_privacy_strip(ws, cx);
    let config_card = render_provider_config_panel(ws, cx);
    let transcript = render_transcript(ws, cx);
    let composer = render_composer(ws, cx);

    let drawer = div()
        .id("workspace-ai-drawer")
        .w(px(320.0))
        .h_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .bg(panel_surface_bg(cx))
        .border_l(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .child(header)
        .child(privacy_bar)
        .when(ws.ai_drawer_ui.config_expanded, |el| el.child(config_card))
        .child(Separator::new())
        .child(transcript)
        .child(Separator::new())
        .child(composer);

    let ms = u64::try_from(drawer_duration(cx).as_millis()).unwrap_or(0);
    let motion = Motion {
        ms,
        scale: 1.0,
        curve: Curve::OutFluid,
    };
    entering_from(
        drawer,
        "workspace-ai-slide",
        Edge::Right,
        px(28.0),
        motion,
        cx,
    )
}

fn render_header(ws: &TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) -> AnyElement {
    let colors = cx.colors().clone();
    let expanded = ws.ai_drawer_ui.config_expanded;
    let has_messages = !ws.controller.ai_drawer.messages.is_empty();

    div()
        .h(px(40.0))
        .px(px(12.0))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(COMPACT_GAP_PX))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    Icon::new(IconName::Sparkles)
                        .size(IconSize::Sm)
                        .color(colors.accent.color),
                )
                .child(
                    div()
                        .text_xs()
                        .font_weight(herogpui::gpui::FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(ws.controller.strings.ai_assistant_title),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(MICRO_GAP_PX))
                .when(has_messages, |el| {
                    el.child(
                        Button::new("ai-clear-chat")
                            .variant(Variant::Ghost)
                            .size(HeroSize::Sm)
                            .on_press(cx.listener(|ws, _, _window, cx| {
                                ws.controller.ai_drawer.messages.clear();
                                ws.ai_drawer_ui.last_error = None;
                                cx.notify();
                            }))
                            .child(ws.controller.strings.clear),
                    )
                })
                .child(
                    Button::new("ai-toggle-config")
                        .variant(if expanded {
                            Variant::Secondary
                        } else {
                            Variant::Ghost
                        })
                        .size(HeroSize::Sm)
                        .is_icon_only(true)
                        .aria_label(ws.controller.strings.ai_provider_settings)
                        .on_press(cx.listener(|ws, _, _window, cx| {
                            ws.ai_drawer_ui.config_expanded = !ws.ai_drawer_ui.config_expanded;
                            cx.notify();
                        }))
                        .child(Icon::new(IconName::SlidersHorizontal).size(IconSize::Xs)),
                )
                .child(
                    Button::new("ai-close-drawer")
                        .variant(Variant::Ghost)
                        .size(HeroSize::Sm)
                        .is_icon_only(true)
                        .aria_label(ws.controller.strings.ai_close_drawer)
                        .on_press(cx.listener(|ws, _, _window, cx| {
                            ws.controller.layout_config.ai_drawer_visible = false;
                            ws.controller.toolbar_status.ai_open = false;
                            ws.controller.sync_layout_counts();
                            cx.notify();
                        }))
                        .child(Icon::new(IconName::X).size(IconSize::Sm)),
                ),
        )
        .into_any_element()
}

fn render_privacy_strip(ws: &TableMinusWorkspace, _cx: &App) -> AnyElement {
    let provider = ws.ai_drawer_ui.config.provider;
    let model = ws.ai_drawer_ui.config.model.clone();

    div()
        .px(px(12.0))
        .pb(px(8.0))
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(MICRO_GAP_PX))
        .child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .color(HeroColor::Success)
                .size(HeroSize::Sm)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(Icon::new(IconName::ShieldCheck).size(IconSize::Xs))
                        .child(ChipLabel::new().child(ws.controller.strings.ai_privacy_banner)),
                ),
        )
        .child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .color(HeroColor::Default)
                .size(HeroSize::Sm)
                .child(
                    ChipLabel::new().child(format!("{} · {model}", provider_short_name(provider))),
                ),
        )
        .into_any_element()
}

fn render_provider_config_panel(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &ws.controller.strings;
    let active_provider = ws.ai_drawer_ui.config.provider;
    let active_model = ws.ai_drawer_ui.config.model.clone();
    let provider_items = vec![
        PickerItem::new(
            AiProvider::OpenAiCompatible.id(),
            s.ai_provider_ollama_local,
        ),
        PickerItem::new(AiProvider::OpenAi.id(), "OpenAI (BYOK)"),
        PickerItem::new(AiProvider::Anthropic.id(), "Anthropic (BYOK)"),
    ];
    let provider_select = Select::new("ai-provider-select", provider_items)
        .aria_label(s.ai_provider_label)
        .height(px(28.0))
        .trigger_text_size(px(12.0))
        .full_width(true)
        .value(Some(SharedString::from(active_provider.id())))
        .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _window, cx| {
            if let Some(id) = key.as_ref().and_then(|k| AiProvider::from_id(k.as_ref())) {
                switch_provider(&mut ws.ai_drawer_ui, id);
                cx.notify();
            }
        }));

    let model_options = available_models_for(&ws.ai_drawer_ui, active_provider);
    let model_items: Vec<PickerItem> = model_options
        .iter()
        .map(|m| PickerItem::new(m.clone(), m.clone()))
        .collect();
    let model_select = Select::new("ai-model-select", model_items)
        .aria_label(s.ai_model_label)
        .height(px(28.0))
        .trigger_text_size(px(12.0))
        .full_width(true)
        .value(Some(SharedString::from(active_model)))
        .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _window, cx| {
            if let Some(model) = key.as_ref() {
                ws.ai_drawer_ui.config.model = model.to_string();
                cx.notify();
            }
        }));

    let refresh_btn = Button::new("ai-refresh-models")
        .variant(Variant::Secondary)
        .size(HeroSize::Sm)
        .is_icon_only(true)
        .is_disabled(ws.ai_drawer_ui.fetching_models)
        .aria_label(s.ai_refresh_models)
        .on_press(cx.listener(|ws, _, _window, cx| {
            fetch_models_async(ws, cx);
        }))
        .child(Icon::new(IconName::RefreshCw).size(IconSize::Xs));

    let needs_key = active_provider.requires_key();
    let has_saved_key = ws
        .ai_drawer_ui
        .key_status
        .get(active_provider.id())
        .copied()
        .unwrap_or(false);

    div()
        .px(px(12.0))
        .pb(px(10.0))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(provider_select)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(div().flex_1().min_w(px(0.0)).child(model_select))
                .child(refresh_btn),
        )
        .when(needs_key, |el| {
            el.child(render_api_key_section(
                ws,
                active_provider,
                has_saved_key,
                &colors,
                cx,
            ))
        })
        .into_any_element()
}

fn render_api_key_section(
    ws: &TableMinusWorkspace,
    active_provider: AiProvider,
    has_saved_key: bool,
    colors: &herogpui::theme::ThemeColors,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let key_input = Input::new(&ws.ai_drawer_ui.api_key_input)
        .input_type(InputType::Password)
        .height(px(28.0))
        .text_size(px(12.0))
        .full_width()
        .placeholder(
            s.ai_api_key_placeholder
                .replace("{provider}", provider_short_name(active_provider)),
        );
    let save_btn = Button::new("ai-save-key")
        .variant(Variant::Secondary)
        .size(HeroSize::Sm)
        .on_press(cx.listener(move |ws, _, _window, cx| {
            save_provider_api_key(ws, active_provider, cx);
        }))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(Icon::new(IconName::Key).size(IconSize::Xs))
                .child(s.save),
        );

    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(div().flex_1().min_w(px(0.0)).child(key_input))
                .child(save_btn),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    Chip::new()
                        .variant(ChipVariant::Soft)
                        .color(if has_saved_key {
                            HeroColor::Success
                        } else {
                            HeroColor::Warning
                        })
                        .size(HeroSize::Sm)
                        .child(ChipLabel::new().child(if has_saved_key {
                            s.ai_key_stored
                        } else {
                            s.ai_key_required
                        })),
                )
                .child(div().text_xs().text_color(colors.muted).child("BYOK")),
        )
        .into_any_element()
}

fn render_transcript(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let scroll_handle = ws.ai_drawer_ui.transcript_scroll.clone();
    let messages = &ws.controller.ai_drawer.messages;
    let waiting = ws.ai_panel.waiting_reply;
    let err_banner = ws.ai_drawer_ui.last_error.clone();

    let mut feed = div().w_full().p(px(12.0)).flex().flex_col().gap(px(10.0));
    if messages.is_empty() && !waiting {
        feed = feed.child(render_empty_transcript_card(ws, cx));
    } else {
        for (idx, msg) in messages.iter().enumerate() {
            feed = feed.child(render_message_bubble(ws, idx, msg, cx));
        }
    }
    if waiting {
        feed = feed.child(render_streaming_indicator(&ws.controller.strings, cx));
    }
    if let Some(err) = err_banner {
        let colors = cx.colors().clone();
        feed = feed.child(
            div()
                .p(px(8.0))
                .rounded(px(CONTROL_RADIUS_PX))
                .bg(with_alpha(colors.danger.color, 0.12))
                .border_1()
                .border_color(colors.danger.color)
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .text_xs()
                        .font_weight(herogpui::gpui::FontWeight::SEMIBOLD)
                        .text_color(colors.danger.color)
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .size(IconSize::Xs)
                                .color(colors.danger.color),
                        )
                        .child(ws.controller.strings.failed),
                )
                .child(div().text_xs().text_color(colors.danger.color).child(err)),
        );
    }

    div()
        .relative()
        .flex_1()
        .min_h(px(0.0))
        .child(
            div()
                .id("ai-transcript-scroll")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&scroll_handle)
                .child(feed),
        )
        .child(Scrollbar::new("ai-transcript-scrollbar", scroll_handle))
        .into_any_element()
}

fn render_empty_transcript_card(ws: &TableMinusWorkspace, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &ws.controller.strings;
    let schema = ws
        .controller
        .cached_schemas
        .first()
        .map_or_else(|| s.default_schema.to_owned(), |sch| sch.name.clone());
    let table_count = ws.controller.cached_objects.len();
    let objects_word = if table_count == 1 {
        s.object_singular
    } else {
        s.object_plural
    };
    let active_tbl = ws
        .controller
        .active_table
        .as_ref()
        .map_or_else(|| s.ai_no_active_table.to_owned(), |t| t.name.clone());
    let context_line = s
        .ai_empty_context_template
        .replace("{schema}", &schema)
        .replace("{count}", &table_count.to_string())
        .replace("{objects}", objects_word)
        .replace("{table}", &active_tbl);

    Card::new()
        .child(
            CardContent::new()
                .sx(|d| d.p(px(12.0)).gap(px(6.0)))
                .child(
                    div()
                        .text_xs()
                        .font_weight(herogpui::gpui::FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(s.ai_empty_title),
                )
                .child(div().text_xs().text_color(colors.muted).child(context_line))
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted)
                        .child(s.ai_empty_desc),
                ),
        )
        .into_any_element()
}

fn render_message_bubble(
    ws: &TableMinusWorkspace,
    msg_idx: usize,
    msg: &AiMessage,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &ws.controller.strings;
    if msg.is_user {
        return div()
            .w_full()
            .p(px(10.0))
            .rounded(px(CONTROL_RADIUS_PX))
            .bg(with_alpha(colors.accent.color, 0.12))
            .border_1()
            .border_color(with_alpha(colors.accent.color, 0.3))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .text_xs()
                    .font_weight(herogpui::gpui::FontWeight::SEMIBOLD)
                    .text_color(colors.accent.color)
                    .child(s.ai_role_you),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.foreground)
                    .child(msg.text.clone()),
            )
            .into_any_element();
    }

    let segments = split_markdown_sql(&msg.text);
    let mut card = div()
        .w_full()
        .p(px(10.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(muted_surface_bg(cx))
        .border_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(
                    Icon::new(IconName::Bot)
                        .size(IconSize::Xs)
                        .color(colors.accent.color),
                )
                .child(
                    div()
                        .text_xs()
                        .font_weight(herogpui::gpui::FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(s.ai_role_assistant),
                ),
        );

    for (seg_idx, segment) in segments.into_iter().enumerate() {
        match segment {
            MessageSegment::Prose(text) => {
                if !text.trim().is_empty() {
                    card = card.child(div().text_xs().text_color(colors.foreground).child(text));
                }
            }
            MessageSegment::Sql(sql) => {
                card = card.child(render_sql_code_block(ws, msg_idx, seg_idx, sql, cx));
            }
        }
    }

    card.into_any_element()
}

fn render_sql_code_block(
    ws: &TableMinusWorkspace,
    msg_idx: usize,
    seg_idx: usize,
    sql: String,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let grammar = ws.active_grammar();
    let styled_sql = styled_sql_block(&sql, grammar, &ws.content_palette);
    let editor_font = editor_gpui_font(&ws.controller.settings);

    div()
        .w_full()
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(ws.content_palette.editor_background)
        .border_1()
        .border_color(hairline_border_color(cx))
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            div()
                .px(px(8.0))
                .py(px(6.0))
                .font(editor_font)
                .text_xs()
                .text_color(ws.content_palette.editor_text)
                .child(styled_sql),
        )
        .child(Separator::new())
        .child(render_sql_block_actions(ws, msg_idx, seg_idx, sql, cx))
        .into_any_element()
}

fn render_sql_block_actions(
    ws: &TableMinusWorkspace,
    msg_idx: usize,
    seg_idx: usize,
    sql: String,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &ws.controller.strings;
    let block_key = format!("{msg_idx}:{seg_idx}");
    let is_copied = ws.ai_drawer_ui.copied_block_key.as_deref() == Some(block_key.as_str());
    let sql_insert = sql.clone();
    let sql_tab = sql.clone();
    let element_id = msg_idx * 32 + seg_idx;

    div()
        .px(px(6.0))
        .py(px(4.0))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(4.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(
                    Button::new(("ai-sql-insert", element_id))
                        .variant(Variant::Primary)
                        .size(HeroSize::Sm)
                        .on_press(cx.listener(move |ws, _, _window, cx| {
                            insert_sql_into_active_editor(ws, &sql_insert, cx);
                        }))
                        .child(s.ai_sql_insert),
                )
                .child(
                    Button::new(("ai-sql-new-tab", element_id))
                        .variant(Variant::Secondary)
                        .size(HeroSize::Sm)
                        .on_press(cx.listener(move |ws, _, _window, cx| {
                            open_sql_in_new_tab(ws, &sql_tab, cx);
                        }))
                        .child(s.ai_sql_new_tab),
                ),
        )
        .child(
            Button::new(("ai-sql-copy", element_id))
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .on_press(cx.listener(move |ws, _, _window, cx| {
                    ws.ai_drawer_ui
                        .copy_sql_with_feedback(block_key.clone(), sql.clone(), cx);
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(
                            Icon::new(if is_copied {
                                IconName::Check
                            } else {
                                IconName::Copy
                            })
                            .size(IconSize::Xs)
                            .color(if is_copied {
                                colors.success.color
                            } else {
                                colors.foreground
                            }),
                        )
                        .child(if is_copied { s.copied } else { s.copy_label }),
                ),
        )
        .into_any_element()
}

fn render_streaming_indicator(strings: &crate::i18n::Strings, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .px(px(10.0))
        .py(px(8.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(muted_surface_bg(cx))
        .border_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(Spinner::new("ai-streaming-spinner").size(HeroSize::Sm))
        .child(
            div()
                .text_xs()
                .text_color(colors.muted)
                .child(strings.ai_streaming),
        )
        .into_any_element()
}

fn render_composer(ws: &TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) -> AnyElement {
    let s = &ws.controller.strings;
    let waiting = ws.ai_panel.waiting_reply;
    let chips = render_quick_schema_chips(ws, cx);
    let area = TextArea::new(&ws.ai_drawer_ui.prompt_input)
        .rows(3)
        .full_width()
        .placeholder(s.ai_prompt_placeholder);

    let send_or_cancel = if waiting {
        Button::new("ai-cancel-stream")
            .variant(Variant::DangerSoft)
            .size(HeroSize::Sm)
            .full_width(true)
            .on_press(cx.listener(|ws, _, _window, cx| {
                if let Some(cancel) = ws.ai_drawer_ui.stream_cancel.take() {
                    cancel.cancel();
                }
                ws.ai_panel.waiting_reply = false;
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.0))
                    .child(Icon::new(IconName::Square).size(IconSize::Xs))
                    .child(s.stop_generation),
            )
    } else {
        Button::new("ai-send-prompt")
            .variant(Variant::Primary)
            .size(HeroSize::Sm)
            .full_width(true)
            .on_press(cx.listener(|ws, _, _window, cx| {
                submit_ai_prompt(ws, cx);
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.0))
                    .child(Icon::new(IconName::Send).size(IconSize::Xs))
                    .child(s.send),
            )
    };

    div()
        .p(px(12.0))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(chips)
        .child(area)
        .child(send_or_cancel)
        .into_any_element()
}

fn render_quick_schema_chips(
    ws: &TableMinusWorkspace,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let select_prompt = match ws.controller.active_table.as_ref() {
        Some(table) => s.ai_prompt_select.replace("{table}", &table.name),
        None => s.ai_prompt_select_current.to_owned(),
    };
    let presets = [
        (
            "ai-chip-explain",
            s.ai_chip_explain,
            s.ai_prompt_explain.to_owned(),
        ),
        (
            "ai-chip-optimize",
            s.ai_chip_optimize,
            s.ai_prompt_optimize.to_owned(),
        ),
        ("ai-chip-select", s.ai_chip_select, select_prompt),
    ];

    let mut row = div().flex().flex_wrap().gap(px(MICRO_GAP_PX));
    for (id, label, template) in presets {
        row = row.child(
            Button::new(id)
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .on_press(cx.listener(move |ws, _, _window, cx| {
                    ws.ai_drawer_ui.prompt_input.update(cx, |input, cx| {
                        input.set_value(template.clone());
                        cx.notify();
                    });
                    ws.controller.ai_drawer.draft_prompt = template.clone();
                    ws.ai_panel.prompt_draft = template.clone();
                    cx.notify();
                }))
                .child(label),
        );
    }
    row.into_any_element()
}

fn switch_provider(state: &mut AiDrawerViewState, provider: AiProvider) {
    state.config.provider = provider;
    state.config.base_url = match provider {
        AiProvider::OpenAiCompatible => DEFAULT_OLLAMA_URL.to_owned(),
        AiProvider::OpenAi => DEFAULT_OPENAI_URL.to_owned(),
        AiProvider::Anthropic => DEFAULT_ANTHROPIC_URL.to_owned(),
    };
    let models = available_models_for(state, provider);
    if !models.contains(&state.config.model) {
        state.config.model = models.into_iter().next().unwrap_or_default();
    }
    state.last_error = None;
}

fn available_models_for(state: &AiDrawerViewState, provider: AiProvider) -> Vec<String> {
    if let Some(fetched) = state.fetched_models.get(provider.id())
        && !fetched.is_empty()
    {
        return fetched.clone();
    }
    match provider {
        AiProvider::OpenAiCompatible => vec![
            "llama3.2".to_owned(),
            "qwen2.5-coder:latest".to_owned(),
            "deepseek-coder-v2".to_owned(),
        ],
        AiProvider::OpenAi => vec![
            "gpt-4.1".to_owned(),
            "gpt-4.1-mini".to_owned(),
            "gpt-4o-mini".to_owned(),
        ],
        AiProvider::Anthropic => vec![
            "claude-sonnet-4-20250514".to_owned(),
            "claude-3-7-sonnet-latest".to_owned(),
            "claude-3-5-haiku-latest".to_owned(),
        ],
    }
}

fn save_provider_api_key(
    ws: &mut TableMinusWorkspace,
    provider: AiProvider,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let raw_key = ws
        .ai_drawer_ui
        .api_key_input
        .read(cx)
        .value()
        .trim()
        .to_owned();
    if raw_key.is_empty() {
        push_error_toast(ws.controller.strings.ai_err_empty_key, None, cx);
        return;
    }
    match ws.controller.store_ai_api_key_blocking(provider, &raw_key) {
        Ok(()) => {
            ws.ai_drawer_ui.key_status.insert(provider.id(), true);
            ws.ai_drawer_ui.api_key_input.update(cx, |input, cx| {
                input.set_value(String::new());
                cx.notify();
            });
            let s = &ws.controller.strings;
            push_success_toast(
                s.ai_toast_key_saved,
                Some(SharedString::from(
                    s.ai_toast_key_saved_desc
                        .replace("{provider}", provider_short_name(provider)),
                )),
                cx,
            );
            cx.notify();
        }
        Err(err) => {
            push_error_toast(
                ws.controller.strings.ai_err_save_key,
                Some(SharedString::from(err.to_string())),
                cx,
            );
        }
    }
}

fn fetch_models_async(ws: &mut TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) {
    let Some(services) = ws.controller.services.clone() else {
        push_error_toast(ws.controller.strings.ai_err_service_unavailable, None, cx);
        return;
    };
    let Some(ai) = services.ai.clone() else {
        push_error_toast(ws.controller.strings.ai_err_client_uninitialized, None, cx);
        return;
    };
    let cfg = ws.ai_drawer_ui.config.clone();
    let provider_id = cfg.provider.id();
    let runtime = services.runtime.clone();
    ws.ai_drawer_ui.fetching_models = true;
    cx.notify();

    ws.ai_drawer_ui.model_task = Some(cx.spawn(
        async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
            let outcome = runtime
                .spawn(async move {
                    let endpoint = ai.resolve_endpoint(&cfg).await?;
                    ai.list_models(&endpoint).await
                })
                .await;

            let _ = this.update(cx, |ws, cx| {
                ws.ai_drawer_ui.fetching_models = false;
                let s = &ws.controller.strings;
                match outcome {
                    Ok(Ok(models)) if !models.is_empty() => {
                        ws.ai_drawer_ui.config.model = models[0].clone();
                        ws.ai_drawer_ui.fetched_models.insert(provider_id, models);
                        push_success_toast(s.ai_toast_models_refreshed, None, cx);
                    }
                    Ok(Ok(_)) => {
                        push_error_toast(s.ai_err_no_models, None, cx);
                    }
                    Ok(Err(err)) => {
                        push_error_toast(
                            s.ai_err_list_models,
                            Some(SharedString::from(err.to_string())),
                            cx,
                        );
                    }
                    Err(join_err) => {
                        push_error_toast(
                            s.ai_err_refresh_aborted,
                            Some(SharedString::from(join_err.to_string())),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        },
    ));
}

struct AiPromptContext {
    cfg: AiEndpointConfig,
    editor_sql: String,
    engine: String,
    db: Option<String>,
    schema: Option<String>,
    tables: Vec<String>,
    active_table: Option<tablepro_core::meta::TableRef>,
    workspace_id: tableminus_state::connection_window::WorkspaceId,
}

fn build_ai_context(ws: &TableMinusWorkspace) -> (AiPromptContext, Vec<ChatMessage>) {
    let cfg = ws.ai_drawer_ui.config.clone();
    let editor_sql = ws.controller.editor_sql_text();
    let engine = ws.controller.toolbar_status.engine_version.clone();
    let db = (!ws.controller.toolbar_status.database.is_empty())
        .then(|| ws.controller.toolbar_status.database.clone());
    let schema = ws.controller.cached_schemas.first().map(|s| s.name.clone());
    let tables: Vec<String> = ws
        .controller
        .cached_objects
        .iter()
        .take(60)
        .map(|o| o.name.clone())
        .collect();
    let active_table = ws.controller.active_table.clone();
    let workspace_id = ws.controller.active_workspace;
    let history: Vec<ChatMessage> = ws
        .controller
        .ai_drawer
        .messages
        .iter()
        .map(|m| {
            if m.is_user {
                ChatMessage::user(m.text.clone())
            } else {
                ChatMessage::assistant(m.text.clone())
            }
        })
        .collect();

    (
        AiPromptContext {
            cfg,
            editor_sql,
            engine,
            db,
            schema,
            tables,
            active_table,
            workspace_id,
        },
        history,
    )
}

fn dispatch_ai_request(
    services: &std::sync::Arc<crate::services::ServiceHub>,
    ai: std::sync::Arc<AiAssistantService>,
    ctx: AiPromptContext,
    history: Vec<ChatMessage>,
    cancel: CancellationToken,
    tx: mpsc::UnboundedSender<ReplyEvent>,
) {
    let runtime = services.runtime.clone();
    let sessions = services.sessions.clone();
    let active_table = ctx.active_table;
    let workspace_id = ctx.workspace_id;
    runtime.spawn(async move {
        let mut described = Vec::new();
        if let Some(table) = active_table {
            let session = {
                let guard = sessions.read().await;
                guard.session(workspace_id)
            };
            if let Some(session) = session
                && let Ok(cols) = session.columns(table.clone()).await
            {
                described.push((table.name, cols.to_vec()));
            }
        }
        let schema_ctx = AiAssistantService::build_schema_context(
            ctx.engine,
            QueryLanguage::Sql,
            ctx.db,
            ctx.schema,
            ctx.tables,
            described,
        );
        match ai.resolve_endpoint(&ctx.cfg).await {
            Ok(endpoint) => {
                ai.stream_chat(
                    &endpoint,
                    &schema_ctx,
                    (!ctx.editor_sql.trim().is_empty()).then_some(ctx.editor_sql.as_str()),
                    history,
                    cancel,
                    tx,
                )
                .await;
            }
            Err(err) => {
                let _ = tx.send(ReplyEvent::Failed(tablepro_ai::AiError::Protocol(
                    err.to_string(),
                )));
            }
        }
    });
}

fn submit_ai_prompt(ws: &mut TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) {
    let prompt = ws
        .ai_drawer_ui
        .prompt_input
        .read(cx)
        .value()
        .trim()
        .to_owned();
    if prompt.is_empty() || ws.ai_panel.waiting_reply {
        return;
    }
    let Some(services) = ws.controller.services.clone() else {
        push_error_toast(ws.controller.strings.ai_err_service_unavailable, None, cx);
        return;
    };
    let Some(ai) = services.ai.clone() else {
        push_error_toast(ws.controller.strings.ai_err_client_uninitialized, None, cx);
        return;
    };

    ws.controller.ai_drawer.messages.push(AiMessage {
        is_user: true,
        text: prompt.clone(),
    });
    ws.controller.ai_drawer.draft_prompt.clear();
    ws.ai_panel.prompt_draft.clear();
    ws.ai_drawer_ui.prompt_input.update(cx, |input, cx| {
        input.set_value(String::new());
        cx.notify();
    });
    ws.ai_drawer_ui.last_error = None;
    ws.ai_panel.waiting_reply = true;

    let cancel = CancellationToken::new();
    ws.ai_drawer_ui.stream_cancel = Some(cancel.clone());
    let (prompt_ctx, history) = build_ai_context(ws);
    let (tx, rx) = mpsc::unbounded_channel::<ReplyEvent>();
    dispatch_ai_request(&services, ai, prompt_ctx, history, cancel, tx);

    ws.ai_drawer_ui.stream_task = Some(spawn_ai_reply_pump(rx, cx));
    cx.notify();
}

fn spawn_ai_reply_pump(
    mut rx: mpsc::UnboundedReceiver<ReplyEvent>,
    cx: &mut Context<TableMinusWorkspace>,
) -> Task<()> {
    cx.spawn(
        async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
            while let Some(ev) = rx.recv().await {
                let is_terminal = matches!(ev, ReplyEvent::Finished(_) | ReplyEvent::Failed(_));
                let res = this.update(cx, |ws, cx| {
                    match ev {
                        ReplyEvent::Text(_) => ws.controller.apply_ai_reply_event(ev),
                        ReplyEvent::Finished(_) => {
                            ws.ai_panel.waiting_reply = false;
                            ws.ai_drawer_ui.stream_cancel = None;
                        }
                        ReplyEvent::Failed(err) => {
                            ws.ai_panel.waiting_reply = false;
                            ws.ai_drawer_ui.stream_cancel = None;
                            ws.ai_drawer_ui.last_error = Some(err.to_string());
                        }
                    }
                    cx.notify();
                });
                if res.is_err() || is_terminal {
                    break;
                }
            }
            let _ = this.update(cx, |ws, cx| {
                ws.ai_panel.waiting_reply = false;
                ws.ai_drawer_ui.stream_cancel = None;
                cx.notify();
            });
        },
    )
}

fn insert_sql_into_active_editor(
    ws: &mut TableMinusWorkspace,
    sql: &str,
    cx: &mut Context<TableMinusWorkspace>,
) {
    ws.controller.set_editor_sql(sql);
    if ws.workspace_pane.sub_view == WorkspaceSubView::DataGrid
        || ws.workspace_pane.sub_view == WorkspaceSubView::Structure
    {
        ws.workspace_pane.sub_view = WorkspaceSubView::SqlEditor;
    }
    push_success_toast(ws.controller.strings.ai_toast_sql_inserted, None, cx);
    cx.notify();
}

fn open_sql_in_new_tab(
    ws: &mut TableMinusWorkspace,
    sql: &str,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let tab_title = ws.controller.strings.ai_query_tab_title;
    let toast_title = ws.controller.strings.ai_toast_sql_new_tab;
    let conn_id = ws
        .controller
        .tabs
        .active_tab()
        .and_then(|id| ws.controller.tabs.tabs().iter().find(|t| t.id == id))
        .map_or_else(
            || TabConnectionId("default".to_owned()),
            |t| t.connection_id.clone(),
        );
    ws.controller.tabs.open_query_tab(conn_id, Some(tab_title));
    ws.controller.set_editor_sql(sql);
    ws.controller.sync_layout_counts();
    ws.workspace_pane.sub_view = WorkspaceSubView::SqlEditor;
    push_success_toast(toast_title, None, cx);
    cx.notify();
}

fn split_markdown_sql(input: &str) -> Vec<MessageSegment> {
    let mut segments = Vec::new();
    let mut prose = String::new();
    let mut code = String::new();
    let mut in_fence = false;

    for line in input.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            if in_fence {
                segments.push(MessageSegment::Sql(code.trim_end().to_owned()));
                code.clear();
                in_fence = false;
            } else {
                if !prose.trim().is_empty() {
                    segments.push(MessageSegment::Prose(prose.trim().to_owned()));
                }
                prose.clear();
                in_fence = true;
            }
        } else if in_fence {
            code.push_str(line);
        } else {
            prose.push_str(line);
        }
    }

    if in_fence && !code.trim().is_empty() {
        segments.push(MessageSegment::Sql(code.trim_end().to_owned()));
    } else if !prose.trim().is_empty() {
        segments.push(MessageSegment::Prose(prose.trim().to_owned()));
    }

    segments
}

const fn provider_short_name(provider: AiProvider) -> &'static str {
    match provider {
        AiProvider::OpenAiCompatible => "Ollama",
        AiProvider::OpenAi => "OpenAI",
        AiProvider::Anthropic => "Anthropic",
    }
}
