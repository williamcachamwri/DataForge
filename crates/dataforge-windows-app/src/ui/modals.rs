//! Blocking Modal & AlertDialog layer with subtle backdrop scrim strictly reserved
//! for true blocking interactions: SSH host key trust, passphrases, interactive auth,
//! Safe Mode confirmation, close-transaction guard, and real atomic row export.

use std::collections::VecDeque;
use std::path::PathBuf;

use dataforge_services::export::{ExportFormat, ExportOptions, export_rows_atomic};
use futures::channel::oneshot;
use herogpui::components::{
    AlertDialog, AlertDialogSize, Button, Chip, ChipLabel, ChipVariant, Icon, IconName, IconSize,
    Input, InputState, InputType, Modal, ModalSize, PickerItem, ProgressBar, Select, Separator,
    Spinner, SpinnerSize, Variant,
};
use herogpui::core::{Backdrop, Color as HeroColor, Size as HeroSize, with_alpha};
use herogpui::gpui::{
    AnyElement, App, AppContext as _, AsyncApp, Context, Entity, FontWeight, InteractiveElement,
    IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, Task, WeakEntity,
    Window, div, px,
};
use herogpui::theme::ActiveTheme;

use super::DataForgeWorkspace;
use super::shell_state::{WorkspaceSubView, push_error_toast, push_success_toast};
use super::theme::{
    COMPACT_GAP_PX, CONTROL_RADIUS_PX, editor_gpui_font, hairline_border_color, is_reduced_motion,
    muted_surface_bg,
};
use crate::dialogs::DialogOutcome;
use crate::i18n::Strings;
use crate::updater::UpdateState;

/// Blocking modal interaction queued for native HeroGPUI resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockingPrompt {
    /// Trust verification for an unknown or changed SSH host fingerprint.
    SshHostKey {
        host: String,
        port: u16,
        algorithm: String,
        fingerprint: String,
    },
    /// Passphrase prompt for an encrypted SSH private key.
    SshPassphrase { key_label: String },
    /// Interactive challenge-response authentication prompt.
    InteractiveAuth { prompt: String, masked: bool },
    /// Safe Mode warning before executing a mutating SQL or batch commit.
    SafeModeConfirmation { summary: String },
}

type PromptResolver = Box<dyn FnOnce(DialogOutcome) + Send + 'static>;

struct QueuedPrompt {
    id: u64,
    prompt: BlockingPrompt,
    resolver: Option<PromptResolver>,
}

/// Export source selection when both grid and SQL result set exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExportSource {
    #[default]
    Auto,
    DataGrid,
    QueryResult,
}

/// Persistent UI state for modal dialogs and confirmation sheets.
pub struct ModalLayerState {
    pub export_modal_open: bool,
    pub update_modal_open: bool,
    export_format: ExportFormat,
    export_source: ExportSource,
    export_path_input: Entity<InputState>,
    export_error: Option<String>,
    export_picker_task: Option<Task<()>>,
    prompt_input: Entity<InputState>,
    prompt_queue: VecDeque<QueuedPrompt>,
    next_prompt_id: u64,
    active_prompt_id: Option<u64>,
}

impl std::fmt::Debug for ModalLayerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModalLayerState")
            .field("export_modal_open", &self.export_modal_open)
            .field("update_modal_open", &self.update_modal_open)
            .field("export_format", &self.export_format)
            .field("queued_prompts", &self.prompt_queue.len())
            .finish_non_exhaustive()
    }
}

impl ModalLayerState {
    /// Initializes modal overlay state owned by [`DataForgeWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, cx: &mut Context<DataForgeWorkspace>) -> Self {
        let export_path_input = cx.new(|cx| InputState::new(cx));
        let prompt_input = cx.new(|cx| InputState::new(cx));
        Self {
            export_modal_open: false,
            update_modal_open: false,
            export_format: ExportFormat::Csv,
            export_source: ExportSource::Auto,
            export_path_input,
            export_error: None,
            export_picker_task: None,
            prompt_input,
            prompt_queue: VecDeque::new(),
            next_prompt_id: 1,
            active_prompt_id: None,
        }
    }

    /// Opens the software update modal dialog.
    pub fn open_update_modal(&mut self) {
        self.update_modal_open = true;
    }

    /// Closes the software update modal dialog.
    pub fn close_update_modal(&mut self) {
        self.update_modal_open = false;
    }

    /// Opens the result/grid export modal and pre-populates a suggested filename.
    pub fn open_export(&mut self, _window: &mut Window, cx: &mut Context<DataForgeWorkspace>) {
        self.export_modal_open = true;
        self.export_error = None;
        let current = self.export_path_input.read(cx).value().trim().to_owned();
        if current.is_empty() {
            let suggested = default_export_path(self.export_format, None);
            self.export_path_input.update(cx, |input, cx| {
                input.set_value(suggested.display().to_string());
                cx.notify();
            });
        }
    }

    /// Queues a blocking prompt modal and invokes `resolver` when the user responds.
    pub fn enqueue_prompt(
        &mut self,
        prompt: BlockingPrompt,
        resolver: impl FnOnce(DialogOutcome) + Send + 'static,
    ) {
        let id = self.next_prompt_id;
        self.next_prompt_id = self.next_prompt_id.wrapping_add(1);
        self.prompt_queue.push_back(QueuedPrompt {
            id,
            prompt,
            resolver: Some(Box::new(resolver)),
        });
    }

    /// Queues a blocking prompt and returns an async [`oneshot::Receiver`] for the reply.
    pub fn enqueue_prompt_oneshot(
        &mut self,
        prompt: BlockingPrompt,
    ) -> oneshot::Receiver<DialogOutcome> {
        let (tx, rx) = oneshot::channel();
        self.enqueue_prompt(prompt, move |outcome| {
            let _ = tx.send(outcome);
        });
        rx
    }

    /// Closes dismissible modals and cancels any queued prompts.
    pub fn close_all(&mut self) {
        self.export_modal_open = false;
        self.update_modal_open = false;
        self.export_error = None;
        while let Some(mut item) = self.prompt_queue.pop_front() {
            if let Some(resolver) = item.resolver.take() {
                resolver(DialogOutcome::Cancelled);
            }
        }
        self.active_prompt_id = None;
    }

    /// Returns `true` if any modal or workspace confirmation sheet is open.
    #[must_use]
    pub fn has_open_modal(&self, ws: &DataForgeWorkspace) -> bool {
        self.export_modal_open
            || self.update_modal_open
            || ws.workspace_pane.close_guard_open
            || ws.workspace_pane.confirm_commit_open
            || !self.prompt_queue.is_empty()
    }

    fn resolve_front_prompt(&mut self, outcome: DialogOutcome, cx: &mut App) {
        if let Some(mut front) = self.prompt_queue.pop_front()
            && let Some(resolver) = front.resolver.take()
        {
            resolver(outcome);
        }
        self.active_prompt_id = None;
        self.prompt_input.update(cx, |input, cx| {
            input.set_value(String::new());
            cx.notify();
        });
    }
}

/// Renders active blocking confirmation alerts or modal dialogs above the workspace.
#[must_use]
pub fn render_modals(
    ws: &mut DataForgeWorkspace,
    window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> Option<AnyElement> {
    if !ws.modals_ui.has_open_modal(ws) {
        return None;
    }

    if let Some((prompt_id, prompt)) = ws
        .modals_ui
        .prompt_queue
        .front()
        .map(|q| (q.id, q.prompt.clone()))
    {
        if ws.modals_ui.active_prompt_id != Some(prompt_id) {
            ws.modals_ui.active_prompt_id = Some(prompt_id);
            ws.modals_ui.prompt_input.update(cx, |input, cx| {
                input.set_value(String::new());
                cx.notify();
            });
        }
        return Some(render_queued_prompt_modal(ws, &prompt, cx));
    }

    if ws.workspace_pane.close_guard_open {
        return Some(render_close_transaction_guard(ws, window, cx));
    }

    if ws.workspace_pane.confirm_commit_open {
        return Some(render_safe_mode_commit_modal(ws, cx));
    }

    if ws.modals_ui.export_modal_open {
        return Some(render_export_modal(ws, cx));
    }

    if ws.modals_ui.update_modal_open {
        return Some(render_update_modal(ws, cx));
    }

    None
}

fn render_queued_prompt_modal(
    ws: &DataForgeWorkspace,
    prompt: &BlockingPrompt,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    match prompt {
        BlockingPrompt::SshHostKey {
            host,
            port,
            algorithm,
            fingerprint,
        } => render_ssh_host_key_modal(ws, host, *port, algorithm, fingerprint, cx),
        BlockingPrompt::SshPassphrase { key_label } => render_secret_prompt_modal(
            ws,
            s.ssh_passphrase_title,
            &s.ssh_passphrase_prompt_tpl.replace("{key}", key_label),
            true,
            s.passphrase_label,
            cx,
        ),
        BlockingPrompt::InteractiveAuth { prompt, masked } => render_secret_prompt_modal(
            ws,
            s.interactive_auth_title,
            prompt,
            *masked,
            if *masked {
                s.secret_pin_label
            } else {
                s.response_label
            },
            cx,
        ),
        BlockingPrompt::SafeModeConfirmation { summary } => {
            render_generic_safe_mode_modal(ws, summary, cx)
        }
    }
}

fn render_ssh_host_key_body(
    ws: &DataForgeWorkspace,
    host: &str,
    port: u16,
    algorithm: &str,
    fingerprint: &str,
    cx: &App,
) -> AnyElement {
    let colors = cx.colors().clone();
    let editor_font = editor_gpui_font(&ws.controller.settings);
    let s = &ws.controller.strings;
    let endpoint = format!("{host}:{port}");

    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div().text_xs().text_color(colors.foreground).child(
                s.ssh_host_key_unverified_tpl
                    .replace("{endpoint}", &endpoint),
            ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    Chip::new()
                        .variant(ChipVariant::Soft)
                        .color(HeroColor::Default)
                        .size(HeroSize::Sm)
                        .child(ChipLabel::new().child(algorithm.to_owned())),
                )
                .child(
                    Chip::new()
                        .variant(ChipVariant::Soft)
                        .color(HeroColor::Warning)
                        .size(HeroSize::Sm)
                        .child(ChipLabel::new().child(s.unknown_host_key_chip)),
                ),
        )
        .child(
            div()
                .p(px(10.0))
                .rounded(px(CONTROL_RADIUS_PX))
                .bg(muted_surface_bg(cx))
                .border_1()
                .border_color(hairline_border_color(cx))
                .font(editor_font)
                .text_xs()
                .text_color(colors.foreground)
                .child(fingerprint.to_owned()),
        )
        .into_any_element()
}

fn render_ssh_host_key_modal(
    ws: &DataForgeWorkspace,
    host: &str,
    port: u16,
    algorithm: &str,
    fingerprint: &str,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;

    Modal::new()
        .id("modal-ssh-host-key")
        .is_open(true)
        .size(ModalSize::Md)
        .backdrop(Backdrop::Blur)
        .is_dismissible(false)
        .title(s.ssh_host_key_title)
        .icon(IconName::ShieldCheck)
        .icon_color(HeroColor::Warning)
        .on_open_change(cx.listener(|ws, &open: &bool, _w, cx| {
            if !open {
                ws.modals_ui
                    .resolve_front_prompt(DialogOutcome::Cancelled, cx);
                cx.notify();
            }
        }))
        .child(render_ssh_host_key_body(
            ws,
            host,
            port,
            algorithm,
            fingerprint,
            cx,
        ))
        .footer_child(
            Button::new("ssh-hostkey-reject")
                .variant(Variant::DangerSoft)
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    ws.modals_ui
                        .resolve_front_prompt(DialogOutcome::Rejected, cx);
                    cx.notify();
                }))
                .child(s.reject_label),
        )
        .footer_child(
            Button::new("ssh-hostkey-once")
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    ws.modals_ui
                        .resolve_front_prompt(DialogOutcome::Accepted(Some("once".to_owned())), cx);
                    cx.notify();
                }))
                .child(s.trust_once_label),
        )
        .footer_child(
            Button::new("ssh-hostkey-save")
                .variant(Variant::Primary)
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    ws.modals_ui
                        .resolve_front_prompt(DialogOutcome::Accepted(Some("save".to_owned())), cx);
                    cx.notify();
                }))
                .child(s.save_host_key_label),
        )
        .into_any_element()
}

fn render_secret_prompt_modal(
    ws: &DataForgeWorkspace,
    title: &str,
    prompt_text: &str,
    masked: bool,
    label: &str,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &ws.controller.strings;
    let input = Input::new(&ws.modals_ui.prompt_input)
        .input_type(if masked {
            InputType::Password
        } else {
            InputType::Text
        })
        .label(label.to_owned())
        .full_width()
        .on_submit(cx.listener(|ws, val: &str, _w, cx| {
            ws.modals_ui
                .resolve_front_prompt(DialogOutcome::Accepted(Some(val.to_owned())), cx);
            cx.notify();
        }));

    Modal::new()
        .id("modal-secret-prompt")
        .is_open(true)
        .size(ModalSize::Sm)
        .backdrop(Backdrop::Blur)
        .is_dismissible(false)
        .title(title.to_owned())
        .icon(IconName::Key)
        .icon_color(HeroColor::Accent)
        .on_open_change(cx.listener(|ws, &open: &bool, _w, cx| {
            if !open {
                ws.modals_ui
                    .resolve_front_prompt(DialogOutcome::Cancelled, cx);
                cx.notify();
            }
        }))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted)
                        .child(prompt_text.to_owned()),
                )
                .child(input),
        )
        .footer_child(
            Button::new("secret-prompt-cancel")
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    ws.modals_ui
                        .resolve_front_prompt(DialogOutcome::Cancelled, cx);
                    cx.notify();
                }))
                .child(s.cancel_label),
        )
        .footer_child(
            Button::new("secret-prompt-submit")
                .variant(Variant::Primary)
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    let val = ws.modals_ui.prompt_input.read(cx).value().to_owned();
                    ws.modals_ui
                        .resolve_front_prompt(DialogOutcome::Accepted(Some(val)), cx);
                    cx.notify();
                }))
                .child(s.continue_label),
        )
        .into_any_element()
}

fn render_generic_safe_mode_modal(
    ws: &DataForgeWorkspace,
    summary: &str,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let editor_font = editor_gpui_font(&ws.controller.settings);
    let s = &ws.controller.strings;

    Modal::new()
        .id("modal-safe-mode-prompt")
        .is_open(true)
        .size(ModalSize::Md)
        .backdrop(Backdrop::Blur)
        .is_dismissible(false)
        .title(s.safe_mode_title)
        .icon(IconName::TriangleAlert)
        .icon_color(HeroColor::Warning)
        .on_open_change(cx.listener(|ws, &open: &bool, _w, cx| {
            if !open {
                ws.modals_ui
                    .resolve_front_prompt(DialogOutcome::Cancelled, cx);
                cx.notify();
            }
        }))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.foreground)
                        .child(s.safe_mode_review_desc),
                )
                .child(
                    div()
                        .p(px(10.0))
                        .rounded(px(CONTROL_RADIUS_PX))
                        .bg(ws.content_palette.editor_background)
                        .border_1()
                        .border_color(hairline_border_color(cx))
                        .font(editor_font)
                        .text_xs()
                        .text_color(ws.content_palette.editor_text)
                        .child(summary.to_owned()),
                ),
        )
        .footer_child(
            Button::new("safemode-prompt-cancel")
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    ws.modals_ui
                        .resolve_front_prompt(DialogOutcome::Cancelled, cx);
                    cx.notify();
                }))
                .child(s.cancel_label),
        )
        .footer_child(
            Button::new("safemode-prompt-confirm")
                .variant(Variant::Danger)
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    ws.modals_ui
                        .resolve_front_prompt(DialogOutcome::Accepted(None), cx);
                    cx.notify();
                }))
                .child(s.execute_write_label),
        )
        .into_any_element()
}

fn render_close_transaction_guard(
    ws: &DataForgeWorkspace,
    _window: &mut Window,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let summary = build_pending_impact_summary(ws);
    let has_edits = ws.controller.grid_model.edits.has_pending_edits();
    let s = &ws.controller.strings;

    AlertDialog::new(s.close_guard_title)
        .id("alert-close-guard")
        .is_open(true)
        .size(AlertDialogSize::Md)
        .status(HeroColor::Warning)
        .backdrop(Backdrop::Blur)
        .is_dismissible(false)
        .is_keyboard_dismiss_disabled(false)
        .description(summary)
        .on_open_change(cx.listener(|ws, &open: &bool, _w, cx| {
            if !open {
                ws.workspace_pane.close_guard_open = false;
                ws.pending_quit_after_confirm = false;
                cx.notify();
            }
        }))
        .footer_child(
            Button::new("close-guard-cancel")
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    ws.workspace_pane.close_guard_open = false;
                    ws.pending_quit_after_confirm = false;
                    cx.notify();
                }))
                .child(s.cancel_label),
        )
        .footer_child(
            Button::new("close-guard-discard")
                .variant(Variant::DangerSoft)
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, window, cx| {
                    ws.controller.rollback_grid_edits();
                    ws.controller.open_transactions.clear();
                    ws.workspace_pane.close_guard_open = false;
                    ws.pending_quit_after_confirm = true;
                    window.remove_window();
                    cx.notify();
                }))
                .child(s.discard_and_close_label),
        )
        .footer_child(
            Button::new("close-guard-commit")
                .variant(Variant::Primary)
                .size(HeroSize::Sm)
                .on_press(cx.listener(move |ws, _, window, cx| {
                    if has_edits {
                        let prev_safe = ws.controller.safe_mode_enabled;
                        ws.controller.safe_mode_enabled = false;
                        let res = ws.controller.commit_grid_edits_blocking();
                        ws.controller.safe_mode_enabled = prev_safe;
                        if let Err(err) = res {
                            push_error_toast(
                                ws.controller.strings.commit_failed_closing,
                                Some(SharedString::from(err.to_string())),
                                cx,
                            );
                            return;
                        }
                    }
                    ws.controller.open_transactions.clear();
                    ws.workspace_pane.close_guard_open = false;
                    ws.pending_quit_after_confirm = true;
                    window.remove_window();
                    cx.notify();
                }))
                .child(if has_edits {
                    s.commit_and_close_label
                } else {
                    s.close_anyway_label
                }),
        )
        .into_any_element()
}

fn render_safe_mode_commit_modal(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let summary = build_pending_impact_summary(ws);
    let safe_mode = ws.controller.safe_mode_enabled;
    let s = &ws.controller.strings;

    Modal::new()
        .id("modal-safe-commit")
        .is_open(true)
        .size(ModalSize::Md)
        .backdrop(Backdrop::Blur)
        .is_dismissible(true)
        .title(if safe_mode {
            s.safe_mode_batch_commit_title
        } else {
            s.confirm_pending_edits_title
        })
        .icon(IconName::TriangleAlert)
        .icon_color(if safe_mode {
            HeroColor::Warning
        } else {
            HeroColor::Accent
        })
        .on_open_change(cx.listener(|ws, &open: &bool, _w, cx| {
            if !open {
                ws.workspace_pane.confirm_commit_open = false;
                cx.notify();
            }
        }))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(div().text_xs().text_color(colors.foreground).child(summary))
                .child(
                    div()
                        .px(px(10.0))
                        .py(px(8.0))
                        .rounded(px(CONTROL_RADIUS_PX))
                        .bg(with_alpha(colors.warning.color, 0.1))
                        .border_1()
                        .border_color(hairline_border_color(cx))
                        .text_xs()
                        .text_color(colors.muted)
                        .child(s.batch_commit_tx_note),
                ),
        )
        .footer_child(
            Button::new("commit-modal-cancel")
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    ws.workspace_pane.confirm_commit_open = false;
                    cx.notify();
                }))
                .child(s.cancel_label),
        )
        .footer_child(
            Button::new("commit-modal-confirm")
                .variant(Variant::Primary)
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    let prev_safe = ws.controller.safe_mode_enabled;
                    ws.controller.safe_mode_enabled = false;
                    let res = ws.controller.commit_grid_edits_blocking();
                    ws.controller.safe_mode_enabled = prev_safe;
                    let s = &ws.controller.strings;
                    match res {
                        Ok(affected) => {
                            ws.workspace_pane.confirm_commit_open = false;
                            let desc = if affected == 1 {
                                s.row_affected_singular.to_owned()
                            } else {
                                s.rows_affected_tpl
                                    .replace("{count}", &affected.to_string())
                            };
                            push_success_toast(
                                s.edits_committed_toast,
                                Some(SharedString::from(desc)),
                                cx,
                            );
                        }
                        Err(err) => {
                            push_error_toast(
                                s.commit_failed_toast,
                                Some(SharedString::from(err.to_string())),
                                cx,
                            );
                        }
                    }
                    cx.notify();
                }))
                .child(s.commit_changes_label),
        )
        .into_any_element()
}

fn render_export_form_body(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let format = ws.modals_ui.export_format;
    let source = ws.modals_ui.export_source;
    let dataset_summary = resolve_export_dataset_label(ws, source);
    let s = &ws.controller.strings;

    let format_items = vec![
        PickerItem::new("csv", s.export_fmt_csv),
        PickerItem::new("json-array", s.export_fmt_json_array),
        PickerItem::new("json-lines", s.export_fmt_json_lines),
        PickerItem::new("sql-insert", s.export_fmt_sql_insert),
    ];
    let format_select = Select::new("export-format-select", format_items)
        .label(s.export_format_label)
        .full_width(true)
        .value(Some(SharedString::from(export_format_key(format))))
        .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _w, cx| {
            if let Some(next_fmt) = key.as_deref().and_then(parse_export_format) {
                ws.modals_ui.export_format = next_fmt;
                update_path_extension(&ws.modals_ui.export_path_input, next_fmt, cx);
                cx.notify();
            }
        }));

    let source_items = vec![
        PickerItem::new("auto", s.export_src_auto),
        PickerItem::new("grid", s.export_src_grid),
        PickerItem::new("query", s.export_src_query),
    ];
    let source_select = Select::new("export-source-select", source_items)
        .label(s.export_source_label)
        .full_width(true)
        .value(Some(SharedString::from(export_source_key(source))))
        .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _w, cx| {
            if let Some(next_src) = key.as_deref().and_then(parse_export_source) {
                ws.modals_ui.export_source = next_src;
                cx.notify();
            }
        }));

    let mut path_input = Input::new(&ws.modals_ui.export_path_input)
        .label(s.export_dest_path_label)
        .full_width()
        .placeholder("C:\\Users\\...\\export.csv");
    if let Some(err) = ws.modals_ui.export_error.clone() {
        path_input = path_input.is_invalid(true).error_message(err);
    }

    let browse_btn = Button::new("export-browse-path")
        .variant(Variant::Secondary)
        .size(HeroSize::Sm)
        .on_press(cx.listener(|ws, _, _w, cx| {
            browse_export_save_path(ws, cx);
        }))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(5.0))
                .child(Icon::new(IconName::FolderOpen).size(IconSize::Xs))
                .child(s.browse_btn),
        );

    div()
        .flex()
        .flex_col()
        .gap(px(COMPACT_GAP_PX + 2.0))
        .child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .color(HeroColor::Default)
                .size(HeroSize::Sm)
                .child(ChipLabel::new().child(dataset_summary)),
        )
        .child(format_select)
        .child(source_select)
        .child(Separator::new())
        .child(
            div()
                .flex()
                .items_end()
                .gap(px(8.0))
                .child(div().flex_1().min_w(px(0.0)).child(path_input))
                .child(browse_btn),
        )
        .into_any_element()
}

fn render_export_modal(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;

    Modal::new()
        .id("modal-export-data")
        .is_open(true)
        .size(ModalSize::Md)
        .backdrop(Backdrop::Blur)
        .is_dismissible(true)
        .title(s.export_modal_title)
        .icon(IconName::Download)
        .icon_color(HeroColor::Accent)
        .on_open_change(cx.listener(|ws, &open: &bool, _w, cx| {
            if !open {
                ws.modals_ui.export_modal_open = false;
                ws.modals_ui.export_error = None;
                cx.notify();
            }
        }))
        .child(render_export_form_body(ws, cx))
        .footer_child(
            Button::new("export-modal-cancel")
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    ws.modals_ui.export_modal_open = false;
                    ws.modals_ui.export_error = None;
                    cx.notify();
                }))
                .child(s.cancel_label),
        )
        .footer_child(
            Button::new("export-modal-run")
                .variant(Variant::Primary)
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    execute_export(ws, cx);
                }))
                .child(s.export),
        )
        .into_any_element()
}

fn browse_export_save_path(ws: &mut DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) {
    let ext = export_extension(ws.modals_ui.export_format);
    let table_stem = ws
        .controller
        .active_table
        .as_ref()
        .map_or("query_export", |t| t.name.as_str());
    let suggested = format!("{table_stem}.{ext}");
    let start_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let rx = cx.prompt_for_new_path(&start_dir, Some(&suggested));

    ws.modals_ui.export_picker_task = Some(cx.spawn(
        async move |this: WeakEntity<DataForgeWorkspace>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(path))) = rx.await else {
                return;
            };
            let _ = this.update(cx, |ws, cx| {
                ws.modals_ui.export_path_input.update(cx, |input, cx| {
                    input.set_value(path.display().to_string());
                    cx.notify();
                });
                ws.modals_ui.export_error = None;
                cx.notify();
            });
        },
    ));
}

fn execute_export(ws: &mut DataForgeWorkspace, cx: &mut Context<DataForgeWorkspace>) {
    let s = ws.controller.strings.clone();
    let raw_path = ws
        .modals_ui
        .export_path_input
        .read(cx)
        .value()
        .trim()
        .to_owned();
    if raw_path.is_empty() {
        ws.modals_ui.export_error = Some(s.export_err_missing_path.to_owned());
        cx.notify();
        return;
    }
    let path = PathBuf::from(&raw_path);
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        ws.modals_ui.export_error = Some(err.to_string());
        cx.notify();
        return;
    }

    let format = ws.modals_ui.export_format;
    let use_query = should_export_query_result(ws, ws.modals_ui.export_source);

    let outcome = if use_query {
        export_active_query_result(ws, &path, format, &s)
    } else {
        ws.controller
            .export_active_grid(&path, format)
            .map_err(|err| err.to_string())
    };

    match outcome {
        Ok(count) => {
            ws.modals_ui.export_modal_open = false;
            ws.modals_ui.export_error = None;
            let path_str = path.display().to_string();
            let desc = if count == 1 {
                s.export_wrote_one_row_tpl.replace("{path}", &path_str)
            } else {
                s.export_wrote_rows_tpl
                    .replace("{count}", &count.to_string())
                    .replace("{path}", &path_str)
            };
            push_success_toast(s.export_complete_toast, Some(SharedString::from(desc)), cx);
        }
        Err(err) => {
            ws.modals_ui.export_error = Some(err.clone());
            push_error_toast(s.export_failed_toast, Some(SharedString::from(err)), cx);
        }
    }
    cx.notify();
}

fn export_active_query_result(
    ws: &mut DataForgeWorkspace,
    path: &std::path::Path,
    format: ExportFormat,
    s: &Strings,
) -> Result<u64, String> {
    let tab_res = ws
        .active_tab_result()
        .ok_or_else(|| s.export_err_no_result_set.to_owned())?;
    let set = tab_res
        .accumulator
        .result_sets
        .get(tab_res.active_result_set)
        .or_else(|| tab_res.accumulator.result_sets.first())
        .ok_or_else(|| s.export_err_empty_result_set.to_owned())?;
    let options = ExportOptions {
        format,
        table: ws.controller.active_table.clone(),
        ..ExportOptions::default()
    };
    let count = export_rows_atomic(path, &set.columns, &set.rows, &options)
        .map_err(|err| err.to_string())?;
    let path_str = path.display().to_string();
    let status = if count == 1 {
        s.export_status_one_tpl.replace("{path}", &path_str)
    } else {
        s.export_status_tpl
            .replace("{count}", &count.to_string())
            .replace("{path}", &path_str)
    };
    ws.controller.last_export_status = Some(status);
    Ok(count)
}

fn should_export_query_result(ws: &DataForgeWorkspace, source: ExportSource) -> bool {
    match source {
        ExportSource::DataGrid => false,
        ExportSource::QueryResult => true,
        ExportSource::Auto => {
            let in_sql_view = matches!(
                ws.workspace_pane.sub_view,
                WorkspaceSubView::SqlEditor | WorkspaceSubView::SplitSqlAndGrid
            );
            let has_query_sets = ws
                .active_tab_result()
                .is_some_and(|r| !r.accumulator.result_sets.is_empty());
            in_sql_view && has_query_sets
        }
    }
}

fn resolve_export_dataset_label(ws: &DataForgeWorkspace, source: ExportSource) -> String {
    let s = &ws.controller.strings;
    if should_export_query_result(ws, source)
        && let Some(r) = ws.active_tab_result()
        && let Some(set) = r
            .accumulator
            .result_sets
            .get(r.active_result_set)
            .or_else(|| r.accumulator.result_sets.first())
    {
        let row_count = set.rows.len();
        let col_count = set.columns.len();
        let row_word = if row_count == 1 {
            s.row_singular
        } else {
            s.rows_suffix
        };
        let col_word = if col_count == 1 {
            s.col_singular
        } else {
            s.col_plural
        };
        return format!(
            "{} #{} · {} {} · {} {}",
            s.export_query_result_set,
            r.active_result_set + 1,
            row_count,
            row_word,
            col_count,
            col_word
        );
    }
    let table_name = ws
        .controller
        .active_table
        .as_ref()
        .map_or(s.active_table_fallback, |t| t.name.as_str());
    let row_count = ws.controller.grid_raw_rows.len();
    let col_count = ws.controller.grid_raw_columns.len();
    let row_word = if row_count == 1 {
        s.row_singular
    } else {
        s.rows_suffix
    };
    let col_word = if col_count == 1 {
        s.col_singular
    } else {
        s.col_plural
    };
    format!(
        "{} ({table_name}) · {} {} · {} {}",
        s.export_data_grid, row_count, row_word, col_count, col_word
    )
}

fn build_pending_impact_summary(ws: &DataForgeWorkspace) -> String {
    let s = &ws.controller.strings;
    let edits = &ws.controller.grid_model.edits;
    let open_tx = ws
        .controller
        .open_transactions
        .values()
        .filter(|v| **v)
        .count();
    let table_label = ws
        .controller
        .active_table
        .as_ref()
        .map_or(s.current_workspace_fallback, |t| t.name.as_str());
    s.pending_impact_summary_tpl
        .replace("{target}", table_label)
        .replace("{modified}", &edits.modified_cells.len().to_string())
        .replace("{inserted}", &edits.inserted_rows.len().to_string())
        .replace("{deleted}", &edits.deleted_rows.len().to_string())
        .replace("{tx}", &open_tx.to_string())
}

fn default_export_path(format: ExportFormat, table: Option<&str>) -> PathBuf {
    let ext = export_extension(format);
    let stem = table.unwrap_or("dataforge_export");
    std::env::temp_dir().join(format!("{stem}.{ext}"))
}

fn update_path_extension(input: &Entity<InputState>, format: ExportFormat, cx: &mut App) {
    let ext = export_extension(format);
    let current = input.read(cx).value().trim().to_owned();
    if current.is_empty() {
        return;
    }
    let mut path = PathBuf::from(current);
    path.set_extension(ext);
    input.update(cx, |s, cx| {
        s.set_value(path.display().to_string());
        cx.notify();
    });
}

const fn export_extension(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Csv => "csv",
        ExportFormat::JsonArray => "json",
        ExportFormat::JsonLines => "jsonl",
        ExportFormat::SqlInsert => "sql",
    }
}

const fn export_format_key(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Csv => "csv",
        ExportFormat::JsonArray => "json-array",
        ExportFormat::JsonLines => "json-lines",
        ExportFormat::SqlInsert => "sql-insert",
    }
}

fn parse_export_format(key: &str) -> Option<ExportFormat> {
    match key {
        "csv" => Some(ExportFormat::Csv),
        "json-array" => Some(ExportFormat::JsonArray),
        "json-lines" => Some(ExportFormat::JsonLines),
        "sql-insert" => Some(ExportFormat::SqlInsert),
        _ => None,
    }
}

const fn export_source_key(source: ExportSource) -> &'static str {
    match source {
        ExportSource::Auto => "auto",
        ExportSource::DataGrid => "grid",
        ExportSource::QueryResult => "query",
    }
}

fn parse_export_source(key: &str) -> Option<ExportSource> {
    match key {
        "auto" => Some(ExportSource::Auto),
        "grid" => Some(ExportSource::DataGrid),
        "query" => Some(ExportSource::QueryResult),
        _ => None,
    }
}

fn render_update_modal(
    ws: &DataForgeWorkspace,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    let state = ws.controller.updater.state().clone();
    let s = &ws.controller.strings;
    let (icon, icon_color) = match &state {
        UpdateState::Available { .. } => (IconName::CloudDownload, HeroColor::Accent),
        UpdateState::Downloading { .. } => (IconName::Download, HeroColor::Accent),
        UpdateState::ReadyToRestart { .. } => (IconName::CircleCheck, HeroColor::Success),
        UpdateState::Checking => (IconName::RefreshCw, HeroColor::Accent),
        UpdateState::Error(_) => (IconName::TriangleAlert, HeroColor::Danger),
        UpdateState::Idle | UpdateState::UpToDate | UpdateState::StandaloneMode => {
            (IconName::Info, HeroColor::Default)
        }
    };

    let modal = Modal::new()
        .id("modal-software-update")
        .is_open(true)
        .size(ModalSize::Md)
        .backdrop(Backdrop::Blur)
        .is_dismissible(true)
        .title(s.update_modal_title)
        .icon(icon)
        .icon_color(icon_color)
        .on_open_change(cx.listener(|ws, &open: &bool, _w, cx| {
            if !open {
                ws.modals_ui.close_update_modal();
                cx.notify();
            }
        }))
        .child(render_update_modal_body(ws, &state, cx));

    append_update_modal_footer(modal, ws, &state, cx).into_any_element()
}

fn render_update_modal_body(ws: &DataForgeWorkspace, state: &UpdateState, cx: &App) -> AnyElement {
    match state {
        UpdateState::Available { version, notes } => {
            render_update_available_body(ws, version, notes, cx)
        }
        UpdateState::Downloading { percent } => render_update_downloading_body(ws, *percent, cx),
        UpdateState::ReadyToRestart { version } => render_update_ready_body(ws, version, cx),
        UpdateState::Checking => render_update_checking_body(ws, cx),
        UpdateState::Error(msg) => render_update_error_body(ws, msg, cx),
        UpdateState::Idle | UpdateState::UpToDate | UpdateState::StandaloneMode => {
            render_update_idle_body(ws, state, cx)
        }
    }
}

fn render_update_available_body(
    ws: &DataForgeWorkspace,
    version: &str,
    notes: &str,
    cx: &App,
) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &ws.controller.strings;
    let current_ver = env!("CARGO_PKG_VERSION");

    let mut body = div()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(
            div()
                .text_xs()
                .text_color(colors.muted)
                .child(s.update_available_subtitle),
        )
        .child(
            div()
                .w_full()
                .p(px(12.0))
                .rounded(px(CONTROL_RADIUS_PX))
                .bg(muted_surface_bg(cx))
                .border_1()
                .border_color(hairline_border_color(cx))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(render_version_column(
                    s.update_current_version,
                    &format!("v{current_ver}"),
                    HeroColor::Default,
                    cx,
                ))
                .child(
                    Icon::new(IconName::ArrowRight)
                        .size(IconSize::Sm)
                        .color(colors.muted),
                )
                .child(render_version_column(
                    s.update_new_version,
                    &format!("v{version}"),
                    HeroColor::Accent,
                    cx,
                )),
        );

    if !notes.trim().is_empty() {
        body = body.child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.muted)
                        .child(s.update_release_notes),
                )
                .child(
                    div()
                        .id("update-modal-release-notes")
                        .w_full()
                        .max_h(px(180.0))
                        .overflow_y_scroll()
                        .p(px(10.0))
                        .rounded(px(CONTROL_RADIUS_PX))
                        .bg(muted_surface_bg(cx))
                        .border_1()
                        .border_color(hairline_border_color(cx))
                        .text_xs()
                        .text_color(colors.foreground)
                        .child(notes.to_owned()),
                ),
        );
    }

    body.into_any_element()
}

fn render_version_column(
    label: &str,
    version_badge: &str,
    chip_color: HeroColor,
    cx: &App,
) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .text_xs()
                .text_color(colors.muted)
                .child(label.to_owned()),
        )
        .child(
            Chip::new()
                .variant(ChipVariant::Soft)
                .color(chip_color)
                .size(HeroSize::Sm)
                .child(ChipLabel::new().child(version_badge.to_owned())),
        )
        .into_any_element()
}

fn render_update_downloading_body(ws: &DataForgeWorkspace, percent: u8, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &ws.controller.strings;
    let status_line = s
        .updates_status_downloading_tpl
        .replace("{percent}", &percent.to_string());

    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.foreground)
                        .child(s.update_downloading_progress),
                )
                .child(
                    Chip::new()
                        .variant(ChipVariant::Soft)
                        .color(HeroColor::Accent)
                        .size(HeroSize::Sm)
                        .child(ChipLabel::new().child(format!("{percent}%"))),
                ),
        )
        .child(
            ProgressBar::new("update-modal-download-progress")
                .value(f32::from(percent))
                .min_value(0.0)
                .max_value(100.0)
                .size(HeroSize::Md)
                .color(HeroColor::Accent),
        )
        .child(div().text_xs().text_color(colors.muted).child(status_line))
        .into_any_element()
}

fn render_update_ready_body(ws: &DataForgeWorkspace, version: &str, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &ws.controller.strings;

    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div().flex().items_center().gap(px(8.0)).child(
                Chip::new()
                    .variant(ChipVariant::Soft)
                    .color(HeroColor::Success)
                    .size(HeroSize::Sm)
                    .child(
                        ChipLabel::new().child(format!("{} · v{version}", s.update_ready_title)),
                    ),
            ),
        )
        .child(
            div()
                .p(px(12.0))
                .rounded(px(CONTROL_RADIUS_PX))
                .bg(muted_surface_bg(cx))
                .border_1()
                .border_color(hairline_border_color(cx))
                .text_xs()
                .text_color(colors.foreground)
                .child(s.update_ready_desc),
        )
        .into_any_element()
}

fn render_update_checking_body(ws: &DataForgeWorkspace, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &ws.controller.strings;
    let indicator = if is_reduced_motion(cx) {
        Icon::new(IconName::LoaderCircle)
            .size(IconSize::Sm)
            .color(colors.accent.color)
            .into_any_element()
    } else {
        Spinner::new("update-modal-checking-spinner")
            .size(SpinnerSize::Sm)
            .into_any_element()
    };

    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(indicator)
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.foreground)
                        .child(s.updates_status_checking),
                ),
        )
        .child(
            ProgressBar::new("update-modal-checking-progress")
                .is_indeterminate(true)
                .size(HeroSize::Sm)
                .color(HeroColor::Accent),
        )
        .into_any_element()
}

fn render_update_error_body(ws: &DataForgeWorkspace, msg: &str, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &ws.controller.strings;
    let err_text = s.updates_status_error_tpl.replace("{error}", msg);

    div()
        .p(px(12.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(with_alpha(colors.danger.color, 0.12))
        .border_1()
        .border_color(with_alpha(colors.danger.color, 0.35))
        .text_xs()
        .text_color(colors.danger.color)
        .child(err_text)
        .into_any_element()
}

fn render_update_idle_body(ws: &DataForgeWorkspace, state: &UpdateState, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &ws.controller.strings;
    let current_ver = env!("CARGO_PKG_VERSION");
    let status_text = match state {
        UpdateState::UpToDate => s.updates_status_up_to_date,
        UpdateState::StandaloneMode => s.updates_status_standalone,
        _ => s.updates_status_idle,
    };

    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(render_version_column(
            s.update_current_version,
            &format!("v{current_ver}"),
            HeroColor::Default,
            cx,
        ))
        .child(div().text_xs().text_color(colors.muted).child(status_text))
        .into_any_element()
}

fn update_dismiss_button(
    id: &'static str,
    variant: Variant,
    label: &'static str,
    cx: &mut Context<DataForgeWorkspace>,
) -> Button {
    Button::new(id)
        .variant(variant)
        .size(HeroSize::Sm)
        .on_press(cx.listener(|ws, _, _w, cx| {
            ws.modals_ui.close_update_modal();
            cx.notify();
        }))
        .child(label)
}

fn append_update_modal_footer(
    modal: Modal,
    ws: &DataForgeWorkspace,
    state: &UpdateState,
    cx: &mut Context<DataForgeWorkspace>,
) -> Modal {
    let s = &ws.controller.strings;
    match state {
        UpdateState::Available { .. } => modal
            .footer_child(update_dismiss_button(
                "update-modal-later",
                Variant::Secondary,
                s.update_later,
                cx,
            ))
            .footer_child(
                Button::new("update-modal-download")
                    .variant(Variant::Primary)
                    .size(HeroSize::Sm)
                    .on_press(cx.listener(|ws, _, _w, cx| {
                        ws.controller.download_update();
                        cx.notify();
                    }))
                    .child(s.update_download_install),
            ),
        UpdateState::Downloading { .. } => modal
            .footer_child(update_dismiss_button(
                "update-modal-bg-later",
                Variant::Ghost,
                s.update_later,
                cx,
            ))
            .footer_child(
                Button::new("update-modal-downloading")
                    .variant(Variant::Primary)
                    .size(HeroSize::Sm)
                    .is_disabled(true)
                    .child(s.updates_btn_downloading),
            ),
        UpdateState::ReadyToRestart { .. } => modal
            .footer_child(update_dismiss_button(
                "update-modal-restart-later",
                Variant::Secondary,
                s.update_later,
                cx,
            ))
            .footer_child(
                Button::new("update-modal-apply-restart")
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
                    .child(s.update_apply_restart),
            ),
        UpdateState::Checking | UpdateState::StandaloneMode => modal.footer_child(
            update_dismiss_button("update-modal-close", Variant::Ghost, s.close_label, cx),
        ),
        UpdateState::Error(_) => modal
            .footer_child(update_dismiss_button(
                "update-modal-err-close",
                Variant::Ghost,
                s.close_label,
                cx,
            ))
            .footer_child(
                Button::new("update-modal-retry")
                    .variant(Variant::Primary)
                    .size(HeroSize::Sm)
                    .on_press(cx.listener(|ws, _, _w, cx| {
                        ws.controller.check_for_updates();
                        cx.notify();
                    }))
                    .child(s.update_retry),
            ),
        UpdateState::Idle | UpdateState::UpToDate => modal
            .footer_child(update_dismiss_button(
                "update-modal-idle-close",
                Variant::Ghost,
                s.close_label,
                cx,
            ))
            .footer_child(
                Button::new("update-modal-check")
                    .variant(Variant::Primary)
                    .size(HeroSize::Sm)
                    .on_press(cx.listener(|ws, _, _w, cx| {
                        ws.controller.check_for_updates();
                        cx.notify();
                    }))
                    .child(s.updates_btn_check),
            ),
    }
}
