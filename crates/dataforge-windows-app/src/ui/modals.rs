//! Blocking Modal & AlertDialog layer with subtle backdrop scrim strictly reserved
//! for true blocking interactions: SSH host key trust, passphrases, interactive auth,
//! Safe Mode confirmation, close-transaction guard, and real atomic row export.

use std::collections::VecDeque;
use std::path::PathBuf;

use dataforge_services::export::{ExportFormat, ExportOptions, export_rows_atomic};
use futures::channel::oneshot;
use herogpui::components::{
    AlertDialog, AlertDialogSize, Button, Chip, ChipLabel, ChipVariant, Icon, IconName, IconSize,
    Input, InputState, InputType, Modal, ModalSize, PickerItem, Select, Separator, Variant,
};
use herogpui::core::{Backdrop, Color as HeroColor, Size as HeroSize, with_alpha};
use herogpui::gpui::{
    AnyElement, App, AppContext as _, AsyncApp, Context, Entity, IntoElement, ParentElement,
    SharedString, Styled, Task, WeakEntity, Window, div, px,
};
use herogpui::theme::ActiveTheme;

use super::DataForgeWorkspace;
use super::shell_state::{WorkspaceSubView, push_error_toast, push_success_toast};
use super::theme::{
    COMPACT_GAP_PX, CONTROL_RADIUS_PX, editor_gpui_font, hairline_border_color, muted_surface_bg,
};
use crate::dialogs::DialogOutcome;

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

    None
}

fn render_queued_prompt_modal(
    ws: &DataForgeWorkspace,
    prompt: &BlockingPrompt,
    cx: &mut Context<DataForgeWorkspace>,
) -> AnyElement {
    match prompt {
        BlockingPrompt::SshHostKey {
            host,
            port,
            algorithm,
            fingerprint,
        } => render_ssh_host_key_modal(ws, host, *port, algorithm, fingerprint, cx),
        BlockingPrompt::SshPassphrase { key_label } => render_secret_prompt_modal(
            ws,
            "SSH Key Passphrase",
            &format!("Enter passphrase to unlock SSH key '{key_label}'."),
            true,
            "Passphrase",
            cx,
        ),
        BlockingPrompt::InteractiveAuth { prompt, masked } => render_secret_prompt_modal(
            ws,
            "Authentication Required",
            prompt,
            *masked,
            if *masked { "Secret / PIN" } else { "Response" },
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

    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(div().text_xs().text_color(colors.foreground).child(format!(
            "The authenticity of host '{host}:{port}' cannot be verified."
        )))
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
                        .child(ChipLabel::new().child("Unknown Host Key")),
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
    Modal::new()
        .id("modal-ssh-host-key")
        .is_open(true)
        .size(ModalSize::Md)
        .backdrop(Backdrop::Opaque)
        .is_dismissible(false)
        .title("Verify SSH Host Key")
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
                .child("Reject"),
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
                .child("Trust Once"),
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
                .child("Save Host Key"),
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
        .backdrop(Backdrop::Opaque)
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
                .child("Cancel"),
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
                .child("Continue"),
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

    Modal::new()
        .id("modal-safe-mode-prompt")
        .is_open(true)
        .size(ModalSize::Md)
        .backdrop(Backdrop::Opaque)
        .is_dismissible(false)
        .title("Safe Mode Confirmation")
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
                        .child("Safe Mode is active. Review the write impact before executing:"),
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
                .child("Cancel"),
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
                .child("Execute Write"),
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

    AlertDialog::new("Unsaved Changes & Active Transactions")
        .id("alert-close-guard")
        .is_open(true)
        .size(AlertDialogSize::Md)
        .status(HeroColor::Warning)
        .backdrop(Backdrop::Opaque)
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
                .child("Cancel"),
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
                .child("Discard & Close"),
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
                                "Commit failed before closing",
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
                    "Commit & Close"
                } else {
                    "Close Anyway"
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

    Modal::new()
        .id("modal-safe-commit")
        .is_open(true)
        .size(ModalSize::Md)
        .backdrop(Backdrop::Opaque)
        .is_dismissible(true)
        .title(if safe_mode {
            "Safe Mode: Confirm Batch Commit"
        } else {
            "Confirm Pending Grid Edits"
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
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.foreground)
                        .child(summary),
                )
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
                        .child("Pending inserts, cell updates, and row deletions will be written inside a transaction."),
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
                .child("Cancel"),
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
                    match res {
                        Ok(affected) => {
                            ws.workspace_pane.confirm_commit_open = false;
                            push_success_toast(
                                "Edits committed",
                                Some(SharedString::from(format!("{affected} rows affected"))),
                                cx,
                            );
                        }
                        Err(err) => {
                            push_error_toast(
                                "Commit failed",
                                Some(SharedString::from(err.to_string())),
                                cx,
                            );
                        }
                    }
                    cx.notify();
                }))
                .child("Commit Changes"),
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

    let format_items = vec![
        PickerItem::new("csv", "CSV (.csv)"),
        PickerItem::new("json-array", "JSON Array (.json)"),
        PickerItem::new("json-lines", "JSON Lines (.jsonl)"),
        PickerItem::new("sql-insert", "SQL INSERT Statements (.sql)"),
    ];
    let format_select = Select::new("export-format-select", format_items)
        .label("Export Format")
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
        PickerItem::new("auto", "Auto (Active View)"),
        PickerItem::new("grid", "Active Data Grid Table"),
        PickerItem::new("query", "Active Query Result Set"),
    ];
    let source_select = Select::new("export-source-select", source_items)
        .label("Data Source")
        .full_width(true)
        .value(Some(SharedString::from(export_source_key(source))))
        .on_selection_change(cx.listener(|ws, key: &Option<SharedString>, _w, cx| {
            if let Some(next_src) = key.as_deref().and_then(parse_export_source) {
                ws.modals_ui.export_source = next_src;
                cx.notify();
            }
        }));

    let mut path_input = Input::new(&ws.modals_ui.export_path_input)
        .label("Destination Path")
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
                .child("Browse..."),
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
    Modal::new()
        .id("modal-export-data")
        .is_open(true)
        .size(ModalSize::Md)
        .backdrop(Backdrop::Opaque)
        .is_dismissible(true)
        .title("Export Rows & Results")
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
                .child("Cancel"),
        )
        .footer_child(
            Button::new("export-modal-run")
                .variant(Variant::Primary)
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    execute_export(ws, cx);
                }))
                .child("Export"),
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
    let raw_path = ws
        .modals_ui
        .export_path_input
        .read(cx)
        .value()
        .trim()
        .to_owned();
    if raw_path.is_empty() {
        ws.modals_ui.export_error = Some("Choose a destination file path.".to_owned());
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
        export_active_query_result(ws, &path, format)
    } else {
        ws.controller
            .export_active_grid(&path, format)
            .map_err(|err| err.to_string())
    };

    match outcome {
        Ok(count) => {
            ws.modals_ui.export_modal_open = false;
            ws.modals_ui.export_error = None;
            push_success_toast(
                "Export complete",
                Some(SharedString::from(format!(
                    "Wrote {count} rows to {}",
                    path.display()
                ))),
                cx,
            );
        }
        Err(err) => {
            ws.modals_ui.export_error = Some(err.clone());
            push_error_toast("Export failed", Some(SharedString::from(err)), cx);
        }
    }
    cx.notify();
}

fn export_active_query_result(
    ws: &mut DataForgeWorkspace,
    path: &std::path::Path,
    format: ExportFormat,
) -> Result<u64, String> {
    let tab_res = ws
        .active_tab_result()
        .ok_or_else(|| "No active query result set to export".to_owned())?;
    let set = tab_res
        .accumulator
        .result_sets
        .get(tab_res.active_result_set)
        .or_else(|| tab_res.accumulator.result_sets.first())
        .ok_or_else(|| "Active query tab contains no result rows".to_owned())?;
    let options = ExportOptions {
        format,
        table: ws.controller.active_table.clone(),
        ..ExportOptions::default()
    };
    let count = export_rows_atomic(path, &set.columns, &set.rows, &options)
        .map_err(|err| err.to_string())?;
    ws.controller.last_export_status = Some(format!("Exported {count} rows to {}", path.display()));
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
    if should_export_query_result(ws, source)
        && let Some(r) = ws.active_tab_result()
        && let Some(set) = r
            .accumulator
            .result_sets
            .get(r.active_result_set)
            .or_else(|| r.accumulator.result_sets.first())
    {
        return format!(
            "Query Result Set #{} · {} rows · {} columns",
            r.active_result_set + 1,
            set.rows.len(),
            set.columns.len()
        );
    }
    let table_name = ws
        .controller
        .active_table
        .as_ref()
        .map_or("active table", |t| t.name.as_str());
    format!(
        "Data Grid ({table_name}) · {} rows · {} columns",
        ws.controller.grid_raw_rows.len(),
        ws.controller.grid_raw_columns.len()
    )
}

fn build_pending_impact_summary(ws: &DataForgeWorkspace) -> String {
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
        .map_or("current workspace", |t| t.name.as_str());
    format!(
        "Target: {table_label} — {} modified cells, {} inserted rows, {} deleted rows, {} open transactions.",
        edits.modified_cells.len(),
        edits.inserted_rows.len(),
        edits.deleted_rows.len(),
        open_tx
    )
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
