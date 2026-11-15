//! Non-blocking right-side Row & Column Inspector drawer displaying full
//! untruncated cell values, Rich Value Viewer (`Text`, `JSON Tree`, `Hex`, `Image`),
//! schema constraints, inline typed editing, and FK jumps.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::hash::{Hash as _, Hasher as _};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tableminus_gfx::grid_renderer::{CellKind, RowState};
use tableminus_state::connection_window::WorkspaceId;
use tableminus_state::data_grid::RowID;
use herogpui::components::anim::{Curve, Edge, Motion, entering_from};
use herogpui::components::{
    Button, Chip, ChipLabel, ChipVariant, Icon, IconName, IconSize, Input, InputState, Scrollbar,
    Separator, Switch, TextArea, Tooltip, Variant,
};
use herogpui::core::{Color as HeroColor, Size as HeroSize, with_alpha};
use herogpui::gpui::prelude::FluentBuilder as _;
use herogpui::gpui::{
    AnyElement, App, AppContext as _, AsyncApp, ClipboardItem, Context, Entity, FontWeight, Hsla,
    Image, ImageFormat, ImageSource, InteractiveElement, IntoElement, ObjectFit, ParentElement,
    ScrollHandle, SharedString, StatefulInteractiveElement, Styled, StyledImage as _, Task,
    WeakEntity, Window, div, img, px,
};
use herogpui::theme::ActiveTheme;
use tablepro_core::browse::RowReference;
use tablepro_core::column::{ColumnDefault, ColumnInfo, ColumnType, ReadForm, ResultColumn};
use tablepro_core::ddl::OutboundForeignKey;
use tablepro_core::edit::{EditContext, EditText, edit_text, parse_edit_text};
use tablepro_core::export::value_text;
use tablepro_core::filter::FilterValue;
use tablepro_core::meta::TableRef;
use tablepro_core::value::Value;
use tablepro_session::operation::{OperationError, OperationKind};

use super::TableMinusWorkspace;
use super::shell_state::{push_error_toast, push_success_toast};
use super::theme::{
    COMPACT_GAP_PX, CONTROL_RADIUS_PX, HAIRLINE_BORDER_PX, MICRO_GAP_PX, drawer_duration,
    hairline_border_color, muted_surface_bg, panel_surface_bg, value_gpui_font,
};

const HEX_PREVIEW_BYTE_LIMIT: usize = 4096;
const JSON_TREE_RENDER_LIMIT: usize = 500;

/// Active presentation tab inside the Rich Value Viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RichViewerMode {
    #[default]
    Text,
    JsonTree,
    Hex,
    Image,
}

impl RichViewerMode {
    const ALL: [Self; 4] = [Self::Text, Self::JsonTree, Self::Hex, Self::Image];

    const fn label(self, s: &crate::i18n::Strings) -> &'static str {
        match self {
            Self::Text => s.inspector_tab_text,
            Self::JsonTree => s.inspector_tab_json,
            Self::Hex => s.inspector_tab_hex,
            Self::Image => s.inspector_tab_image,
        }
    }
}

/// Segment of a path inside a [`serde_json::Value`] tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonPathSegment {
    Key(String),
    Index(usize),
}

/// Typed summary of a node in a flattened JSON tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonNodeKind {
    Object { len: usize },
    Array { len: usize },
    String(String),
    Number(String),
    Bool(bool),
    Null,
}

impl JsonNodeKind {
    const fn is_container(&self) -> bool {
        matches!(self, Self::Object { .. } | Self::Array { .. })
    }
}

/// Single flattened row in the collapsible JSON Tree viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatJsonNode {
    pub path: String,
    pub segments: Vec<JsonPathSegment>,
    pub depth: usize,
    pub key_label: Option<String>,
    pub kind: JsonNodeKind,
    pub is_expanded: bool,
}

/// Origin of bytes shown in the Hex viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HexSourceKind {
    RawBytes,
    DecodedHexString,
    Utf8Bytes,
}

impl HexSourceKind {
    const fn badge(self, s: &crate::i18n::Strings) -> &'static str {
        match self {
            Self::RawBytes => s.raw_mode,
            Self::DecodedHexString => s.inspector_tab_hex,
            Self::Utf8Bytes => "UTF-8",
        }
    }
}

/// Single aligned row in the Hex dump table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HexRow {
    pub offset: usize,
    pub offset_hex: String,
    pub hex_bytes: String,
    pub ascii: String,
}

/// Detected image format from magic bytes or SVG markup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectedImageFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
    Bmp,
    Svg,
}

impl DetectedImageFormat {
    const fn badge(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Gif => "GIF",
            Self::Webp => "WEBP",
            Self::Bmp => "BMP",
            Self::Svg => "SVG",
        }
    }

    const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
            Self::Bmp => "bmp",
            Self::Svg => "svg",
        }
    }

    const fn gpui_format(self) -> ImageFormat {
        match self {
            Self::Png => ImageFormat::Png,
            Self::Jpeg => ImageFormat::Jpeg,
            Self::Gif => ImageFormat::Gif,
            Self::Webp => ImageFormat::Webp,
            Self::Bmp => ImageFormat::Bmp,
            Self::Svg => ImageFormat::Svg,
        }
    }
}

/// Parsed image header dimensions and payload size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageMetadata {
    pub format: DetectedImageFormat,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub byte_len: usize,
}

/// Resolved snapshot of the currently focused DataGrid cell and its row context.
struct InspectedCell {
    row_idx: usize,
    col_idx: usize,
    col_name: String,
    col_type: ColumnType,
    col_info: ColumnInfo,
    value: Value,
    original_value: Value,
    full_text: String,
    is_null: bool,
    is_read_only: bool,
    read_only_hint: Option<String>,
    is_modified: bool,
    is_inserted: bool,
    is_deleted: bool,
    edit_context: EditContext,
    outbound_fk: Option<OutboundForeignKey>,
    is_fk_cell: bool,
    row_columns: Vec<ResultColumn>,
    row_values: Vec<Value>,
}

/// Persistent UI state for the right-hand Row Inspector drawer.
pub struct InspectorViewState {
    pub raw_pretty_mode: bool,
    pub viewer_mode: RichViewerMode,
    user_mode_override: Option<(usize, RichViewerMode)>,
    collapsed_json_paths: BTreeSet<String>,
    json_editing_path: Option<(String, Vec<JsonPathSegment>)>,
    json_node_input: Entity<InputState>,
    json_node_error: Option<String>,
    hex_bytes_per_row: usize,
    cached_preview_image: Option<(u64, Arc<Image>)>,
    export_binary_task: Option<Task<()>>,
    editor_input: Entity<InputState>,
    scroll_handle: ScrollHandle,
    last_cell_coords: Option<(usize, usize)>,
    last_sync_sig: Option<(usize, usize, String, bool)>,
    parse_error: Option<String>,
    copied_feedback: bool,
    copied_gen: u64,
    copy_reset_task: Option<Task<()>>,
    cached_table_key: Option<(WorkspaceId, Option<TableRef>)>,
    metadata_loading: bool,
    cached_columns: Vec<ColumnInfo>,
    cached_fks: Vec<OutboundForeignKey>,
    metadata_task: Option<Task<()>>,
}

impl InspectorViewState {
    /// Initializes inspector drawer state owned by [`TableMinusWorkspace`].
    #[must_use]
    pub fn new(_window: &mut Window, cx: &mut Context<TableMinusWorkspace>) -> Self {
        let editor_input = cx.new(|cx| InputState::new(cx));
        let json_node_input = cx.new(|cx| InputState::new(cx));
        Self {
            raw_pretty_mode: false,
            viewer_mode: RichViewerMode::Text,
            user_mode_override: None,
            collapsed_json_paths: BTreeSet::new(),
            json_editing_path: None,
            json_node_input,
            json_node_error: None,
            hex_bytes_per_row: 16,
            cached_preview_image: None,
            export_binary_task: None,
            editor_input,
            scroll_handle: ScrollHandle::new(),
            last_cell_coords: None,
            last_sync_sig: None,
            parse_error: None,
            copied_feedback: false,
            copied_gen: 0,
            copy_reset_task: None,
            cached_table_key: None,
            metadata_loading: false,
            cached_columns: Vec::new(),
            cached_fks: Vec::new(),
            metadata_task: None,
        }
    }

    fn sync_editor_if_needed(&mut self, cell: &InspectedCell, cx: &mut App) {
        let coords = (cell.row_idx, cell.col_idx);
        let cell_changed = self.last_cell_coords != Some(coords);
        if cell_changed {
            self.last_cell_coords = Some(coords);
            self.collapsed_json_paths.clear();
            self.json_editing_path = None;
            self.json_node_error = None;
            self.cached_preview_image = None;
            if let Some((override_col, mode)) = self.user_mode_override {
                if override_col == cell.col_idx {
                    self.viewer_mode = mode;
                } else {
                    self.user_mode_override = None;
                    self.viewer_mode = detect_recommended_mode(&cell.value, &cell.full_text);
                }
            } else {
                self.viewer_mode = detect_recommended_mode(&cell.value, &cell.full_text);
            }
        }

        let sig = (
            cell.row_idx,
            cell.col_idx,
            cell.full_text.clone(),
            cell.is_null,
        );
        if self.last_sync_sig.as_ref() == Some(&sig) {
            return;
        }
        self.last_sync_sig = Some(sig);
        self.parse_error = None;
        self.json_editing_path = None;
        self.json_node_error = None;
        let next_text = if cell.is_null {
            String::new()
        } else {
            cell.full_text.clone()
        };
        self.editor_input.update(cx, |input, cx| {
            input.set_value(next_text);
            cx.notify();
        });
    }

    fn ensure_preview_image(&mut self, format: DetectedImageFormat, bytes: &[u8]) -> Arc<Image> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        format.extension().hash(&mut hasher);
        bytes.hash(&mut hasher);
        let digest = hasher.finish();
        if let Some((cached_digest, ref image)) = self.cached_preview_image
            && cached_digest == digest
        {
            return Arc::clone(image);
        }
        let image = Arc::new(Image::from_bytes(format.gpui_format(), bytes.to_vec()));
        self.cached_preview_image = Some((digest, Arc::clone(&image)));
        image
    }

    fn trigger_copy_feedback(&mut self, text: String, cx: &mut Context<TableMinusWorkspace>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied_gen = self.copied_gen.wrapping_add(1);
        let generation = self.copied_gen;
        self.copied_feedback = true;
        self.copy_reset_task = Some(cx.spawn(
            async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
                cx.background_executor()
                    .timer(Duration::from_millis(1400))
                    .await;
                let _ = this.update(cx, |ws, cx| {
                    if ws.inspector_ui.copied_gen == generation {
                        ws.inspector_ui.copied_feedback = false;
                        cx.notify();
                    }
                });
            },
        ));
        cx.notify();
    }
}

fn ensure_metadata_sync(ws: &mut TableMinusWorkspace, cx: &mut Context<TableMinusWorkspace>) {
    let key = (
        ws.controller.active_workspace,
        ws.controller.active_table.clone(),
    );
    if ws.inspector_ui.cached_table_key.as_ref() == Some(&key) {
        return;
    }
    ws.inspector_ui.cached_table_key = Some(key.clone());
    ws.inspector_ui.cached_columns.clear();
    ws.inspector_ui.cached_fks.clear();
    ws.inspector_ui.metadata_task = None;

    let (workspace_id, Some(table)) = key else {
        ws.inspector_ui.metadata_loading = false;
        return;
    };
    let Some(services) = ws.controller.services.clone() else {
        ws.inspector_ui.metadata_loading = false;
        return;
    };

    ws.inspector_ui.metadata_loading = true;
    let runtime = services.runtime.clone();
    let sessions = services.sessions.clone();
    ws.inspector_ui.metadata_task = Some(cx.spawn(
        async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
            let result = runtime
                .spawn(async move {
                    let session = {
                        let guard = sessions.read().await;
                        guard.session(workspace_id)
                    };
                    let Some(session) = session else {
                        return (Vec::new(), Vec::new());
                    };
                    let cols = session
                        .columns(table.clone())
                        .await
                        .map(|arc| arc.to_vec())
                        .unwrap_or_default();
                    let fk_table = table.clone();
                    let fks = session
                        .run(OperationKind::Metadata, move |conn, opts| {
                            let t = fk_table.clone();
                            async move {
                                conn.fetch_outbound_foreign_keys(&t, opts)
                                    .await
                                    .map_err(OperationError::from)
                            }
                        })
                        .await
                        .unwrap_or_default();
                    (cols, fks)
                })
                .await
                .unwrap_or_default();

            let _ = this.update(cx, |ws, cx| {
                ws.inspector_ui.cached_columns = result.0;
                ws.inspector_ui.cached_fks = result.1;
                ws.inspector_ui.metadata_loading = false;
                cx.notify();
            });
        },
    ));
}

/// Renders the non-blocking `300px` right-hand Row Inspector drawer.
#[must_use]
pub fn render_inspector(
    ws: &mut TableMinusWorkspace,
    _window: &mut Window,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    ensure_metadata_sync(ws, cx);
    let inspected = resolve_inspected_cell(ws);
    if let Some(cell) = inspected.as_ref() {
        ws.inspector_ui.sync_editor_if_needed(cell, cx);
    } else {
        ws.inspector_ui.last_sync_sig = None;
        ws.inspector_ui.last_cell_coords = None;
    }

    let header = render_header(ws, inspected.is_some(), cx);
    let body = match inspected {
        Some(ref cell) => render_cell_body(ws, cell, cx),
        None => render_empty_state(&ws.controller.strings, cx),
    };

    let drawer = div()
        .id("workspace-inspector-panel")
        .w(px(300.0))
        .h_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .bg(panel_surface_bg(cx))
        .border_l(px(HAIRLINE_BORDER_PX))
        .border_color(hairline_border_color(cx))
        .child(header)
        .child(Separator::new())
        .child(body);

    let ms = u64::try_from(drawer_duration(cx).as_millis()).unwrap_or(0);
    let motion = Motion {
        ms,
        scale: 1.0,
        curve: Curve::OutFluid,
    };
    entering_from(
        drawer,
        "workspace-inspector-slide",
        Edge::Right,
        px(28.0),
        motion,
        cx,
    )
}

fn render_header(
    ws: &TableMinusWorkspace,
    has_cell: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let table_label = ws.controller.active_table.as_ref().map(|t| t.name.clone());

    let mut row = div()
        .h(px(40.0))
        .px(px(12.0))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(COMPACT_GAP_PX))
        .child(
            div()
                .min_w(px(0.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(ws.controller.strings.inspector_short),
                )
                .when_some(table_label, |el, tbl| {
                    el.child(
                        Chip::new()
                            .variant(ChipVariant::Soft)
                            .color(HeroColor::Default)
                            .size(HeroSize::Sm)
                            .child(ChipLabel::new().child(tbl)),
                    )
                }),
        );

    let mut actions = div().flex().items_center().gap(px(MICRO_GAP_PX));
    if has_cell {
        let pretty = ws.inspector_ui.raw_pretty_mode;
        let mode_text = if pretty {
            ws.controller.strings.pretty_mode
        } else {
            ws.controller.strings.raw_mode
        };
        actions = actions.child(
            Button::new("inspector-pretty-toggle")
                .variant(if pretty {
                    Variant::Secondary
                } else {
                    Variant::Ghost
                })
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _window, cx| {
                    ws.inspector_ui.raw_pretty_mode = !ws.inspector_ui.raw_pretty_mode;
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(Icon::new(IconName::Code).size(IconSize::Xs))
                        .child(mode_text),
                ),
        );
    }

    actions = actions.child(
        Button::new("inspector-close")
            .variant(Variant::Ghost)
            .size(HeroSize::Sm)
            .is_icon_only(true)
            .aria_label(ws.controller.strings.close_inspector_aria)
            .on_press(cx.listener(|ws, _, _window, cx| {
                ws.controller.layout_config.inspector_visible = false;
                ws.controller.toolbar_status.inspector_open = false;
                ws.controller.sync_layout_counts();
                cx.notify();
            }))
            .child(Icon::new(IconName::X).size(IconSize::Sm)),
    );

    row = row.child(actions);
    row.into_any_element()
}

fn render_empty_state(s: &crate::i18n::Strings, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .flex_1()
        .min_h(px(0.0))
        .p(px(20.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .child(
            Icon::new(IconName::Rows2)
                .size(IconSize::Md)
                .color(colors.muted),
        )
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.foreground)
                .text_center()
                .child(s.inspector_empty_title),
        )
        .child(
            div()
                .text_xs()
                .text_color(colors.muted)
                .text_center()
                .child(s.inspector_empty_desc),
        )
        .into_any_element()
}

fn render_cell_body(
    ws: &mut TableMinusWorkspace,
    cell: &InspectedCell,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let scroll_handle = ws.inspector_ui.scroll_handle.clone();
    let header_block = render_column_header_block(cell, &ws.controller.strings, cx);
    let viewer_section = render_rich_value_viewer(ws, cell, cx);
    let fk_row = render_fk_jump_row(ws, cell, cx);
    let editor_section = render_inline_editor_section(ws, cell, cx);
    let row_summary = render_row_summary_list(ws, cell, cx);

    let content = div()
        .w_full()
        .p(px(12.0))
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(header_block)
        .child(viewer_section)
        .when_some(fk_row, |el, fk| el.child(fk))
        .child(editor_section)
        .child(Separator::new())
        .child(row_summary);

    div()
        .relative()
        .flex_1()
        .min_h(px(0.0))
        .child(
            div()
                .id("inspector-scroll-body")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&scroll_handle)
                .child(content),
        )
        .child(Scrollbar::new("inspector-scrollbar", scroll_handle))
        .into_any_element()
}

fn build_column_chips(cell: &InspectedCell, s: &crate::i18n::Strings) -> AnyElement {
    let mut chips = div().flex().flex_wrap().gap(px(MICRO_GAP_PX));
    let type_label = if cell.col_type.name.is_empty() {
        s.tx_unknown.to_owned()
    } else {
        cell.col_type.name.clone()
    };
    chips = chips.child(meta_chip(type_label, HeroColor::Default));
    chips = chips.child(meta_chip(
        if cell.col_info.nullable {
            s.nullable_badge
        } else {
            s.not_null_badge
        },
        if cell.col_info.nullable {
            HeroColor::Default
        } else {
            HeroColor::Warning
        },
    ));
    if cell.col_info.primary_key {
        chips = chips.child(meta_chip("PK", HeroColor::Accent));
    }
    if cell.outbound_fk.is_some() || cell.is_fk_cell {
        chips = chips.child(meta_chip("FK", HeroColor::Accent));
    }
    if cell.is_modified {
        chips = chips.child(meta_chip(s.cell_state_modified, HeroColor::Warning));
    } else if cell.is_inserted {
        chips = chips.child(meta_chip(s.cell_state_inserted, HeroColor::Success));
    } else if cell.is_deleted {
        chips = chips.child(meta_chip(s.cell_state_deleted, HeroColor::Danger));
    }
    chips.into_any_element()
}

fn meta_chip(label: impl Into<SharedString>, color: HeroColor) -> AnyElement {
    Chip::new()
        .variant(ChipVariant::Soft)
        .color(color)
        .size(HeroSize::Sm)
        .child(ChipLabel::new().child(label.into()))
        .into_any_element()
}

fn render_column_header_block(
    cell: &InspectedCell,
    s: &crate::i18n::Strings,
    cx: &App,
) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(6.0))
                .child(
                    div()
                        .min_w(px(0.0))
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(cell.col_name.clone()),
                )
                .child(
                    div().text_xs().text_color(colors.muted).child(
                        s.row_number_tpl
                            .replace("{num}", &(cell.row_idx + 1).to_string()),
                    ),
                ),
        )
        .child(build_column_chips(cell, s))
        .into_any_element()
}

fn render_mode_segmented_strip(
    active_mode: RichViewerMode,
    col_idx: usize,
    s: &crate::i18n::Strings,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let mut strip = div()
        .w_full()
        .p(px(2.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(muted_surface_bg(cx))
        .border_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .items_center()
        .gap(px(2.0));

    for (idx, mode) in RichViewerMode::ALL.into_iter().enumerate() {
        let is_active = mode == active_mode;
        let bg = if is_active {
            panel_surface_bg(cx)
        } else {
            with_alpha(colors.foreground, 0.0)
        };
        let border = if is_active {
            hairline_border_color(cx)
        } else {
            with_alpha(colors.foreground, 0.0)
        };
        let text_color = if is_active {
            colors.foreground
        } else {
            colors.muted
        };

        strip = strip.child(
            div()
                .id(("inspector-mode-tab", idx))
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .py(px(3.0))
                .px(px(4.0))
                .rounded(px(4.0))
                .bg(bg)
                .border_1()
                .border_color(border)
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.0))
                .font_weight(if is_active {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                })
                .text_color(text_color)
                .when(!is_active, |el| {
                    el.hover(|s| {
                        s.bg(with_alpha(colors.foreground, 0.05))
                            .text_color(colors.foreground)
                    })
                })
                .on_click(cx.listener(move |ws, _, _window, cx| {
                    ws.inspector_ui.viewer_mode = mode;
                    ws.inspector_ui.user_mode_override = Some((col_idx, mode));
                    cx.notify();
                }))
                .child(mode.label(s)),
        );
    }

    strip.into_any_element()
}

fn render_copy_button(
    id: &'static str,
    copied: bool,
    payload: String,
    s: &crate::i18n::Strings,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    Button::new(id)
        .variant(if copied {
            Variant::Secondary
        } else {
            Variant::Ghost
        })
        .size(HeroSize::Sm)
        .on_press(cx.listener(move |ws, _, _window, cx| {
            ws.inspector_ui.trigger_copy_feedback(payload.clone(), cx);
        }))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
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
                .child(if copied { s.copied } else { s.copy_label }),
        )
        .into_any_element()
}

fn render_rich_value_viewer(
    ws: &mut TableMinusWorkspace,
    cell: &InspectedCell,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let active_mode = ws.inspector_ui.viewer_mode;
    let strip = render_mode_segmented_strip(active_mode, cell.col_idx, &ws.controller.strings, cx);

    let mode_body = match active_mode {
        RichViewerMode::Text => render_text_mode_panel(ws, cell, cx),
        RichViewerMode::JsonTree => render_json_tree_panel(ws, cell, cx),
        RichViewerMode::Hex => render_hex_mode_panel(ws, cell, cx),
        RichViewerMode::Image => render_image_mode_panel(ws, cell, cx),
    };

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(strip)
        .child(mode_body)
        .into_any_element()
}

fn render_text_mode_panel(
    ws: &TableMinusWorkspace,
    cell: &InspectedCell,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let value_font = value_gpui_font(&ws.controller.settings);
    let display_text = format_display_value(cell, ws.inspector_ui.raw_pretty_mode);
    let copy_payload = if cell.is_null {
        "NULL".to_owned()
    } else {
        display_text.clone()
    };
    let copy_button = render_copy_button(
        "inspector-copy-value",
        ws.inspector_ui.copied_feedback,
        copy_payload,
        &ws.controller.strings,
        cx,
    );

    let value_char_count = if cell.is_null {
        0
    } else {
        cell.full_text.chars().count()
    };

    let value_surface = div()
        .id("inspector-value-scroll")
        .w_full()
        .max_h(px(230.0))
        .overflow_y_scroll()
        .p(px(8.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(ws.content_palette.editor_background)
        .border_1()
        .border_color(hairline_border_color(cx))
        .font(value_font)
        .text_xs()
        .text_color(if cell.is_null {
            ws.content_palette.editor_syntax_null
        } else {
            ws.content_palette.editor_text
        })
        .when(cell.is_null, |el| el.italic())
        .child(if cell.is_null {
            "NULL".to_owned()
        } else if display_text.is_empty() {
            ws.controller.strings.empty_string_placeholder.to_owned()
        } else {
            display_text
        });

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.muted)
                        .child(
                            ws.controller
                                .strings
                                .inspector_value_chars_tpl
                                .replace("{count}", &value_char_count.to_string()),
                        ),
                )
                .child(copy_button),
        )
        .child(value_surface)
        .into_any_element()
}

fn render_json_tree_panel(
    ws: &TableMinusWorkspace,
    cell: &InspectedCell,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let parsed = parse_json_value(&cell.value, &cell.full_text);
    let Some(root) = parsed else {
        return render_viewer_fallback_panel(
            if cell.is_null {
                "NULL"
            } else {
                ws.controller.strings.inspector_invalid_json
            },
            cell.col_idx,
            &ws.controller.strings,
            !cell.is_null,
            cx,
        );
    };

    let toolbar = render_json_toolbar(ws, cell, &root, cx);
    let nodes = flatten_json_tree(&root, &ws.inspector_ui.collapsed_json_paths);
    let total_nodes = nodes.len();
    let editable = !cell.is_read_only && !cell.is_null && !cell.is_deleted;
    let value_font = value_gpui_font(&ws.controller.settings);

    let mut tree_list = div()
        .id("inspector-json-tree-scroll")
        .w_full()
        .max_h(px(260.0))
        .overflow_y_scroll()
        .p(px(6.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(ws.content_palette.editor_background)
        .border_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .flex_col()
        .gap(px(1.0));

    for (idx, node) in nodes.into_iter().take(JSON_TREE_RENDER_LIMIT).enumerate() {
        let is_editing = ws
            .inspector_ui
            .json_editing_path
            .as_ref()
            .is_some_and(|(p, _)| p == &node.path);
        tree_list = tree_list.child(render_json_tree_row(
            ws,
            cell,
            &node,
            idx,
            editable,
            value_font.clone(),
            cx,
        ));
        if is_editing {
            tree_list = tree_list.child(render_json_inline_node_editor(
                ws,
                cell,
                &root,
                value_font.clone(),
                cx,
            ));
        }
    }

    if total_nodes > JSON_TREE_RENDER_LIMIT {
        let colors = cx.colors().clone();
        let s = &ws.controller.strings;
        tree_list = tree_list.child(
            div()
                .px(px(6.0))
                .py(px(4.0))
                .text_size(px(11.0))
                .text_color(colors.muted)
                .child(format!(
                    "{} {JSON_TREE_RENDER_LIMIT} {} {total_nodes} {}",
                    s.showing_prefix, s.of_prep, s.explain_nodes_suffix
                )),
        );
    }

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(toolbar)
        .child(tree_list)
        .into_any_element()
}

fn render_json_format_apply_btn(
    cell: &InspectedCell,
    pretty_for_apply: String,
    apply_label: &'static str,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let row_idx = cell.row_idx;
    let col_idx = cell.col_idx;
    let col_info = cell.col_info.clone();
    let edit_ctx = cell.edit_context;
    let orig_val = cell.original_value.clone();

    Tooltip::new(apply_label)
        .child(
            Button::new("inspector-json-format-apply")
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .is_icon_only(true)
                .aria_label(apply_label)
                .on_press(cx.listener(move |ws, _, _w, cx| {
                    let next_val = match parse_edit_text(
                        &pretty_for_apply,
                        &col_info,
                        edit_ctx,
                        Some(&orig_val),
                    ) {
                        Ok(cell_in) => cell_in
                            .value()
                            .cloned()
                            .unwrap_or_else(|| Value::Text(pretty_for_apply.clone())),
                        Err(_) => Value::Text(pretty_for_apply.clone()),
                    };
                    ws.inspector_ui.raw_pretty_mode = true;
                    apply_inspector_cell_edit(ws, row_idx, col_idx, next_val);
                    cx.notify();
                }))
                .child(Icon::new(IconName::WandSparkles).size(IconSize::Xs)),
        )
        .into_any_element()
}

fn render_json_toolbar(
    ws: &TableMinusWorkspace,
    cell: &InspectedCell,
    root: &serde_json::Value,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let s = &ws.controller.strings;
    let pretty = ws.inspector_ui.raw_pretty_mode;
    let formatted_json = if pretty {
        serde_json::to_string_pretty(root).unwrap_or_else(|_| cell.full_text.clone())
    } else {
        serde_json::to_string(root).unwrap_or_else(|_| cell.full_text.clone())
    };
    let all_containers = collect_container_paths(root);
    let editable = !cell.is_read_only && !cell.is_null && !cell.is_deleted;
    let pretty_for_apply =
        serde_json::to_string_pretty(root).unwrap_or_else(|_| cell.full_text.clone());

    let mut controls = div().flex().items_center().gap(px(2.0));
    controls = controls
        .child(
            Tooltip::new(s.show_all).child(
                Button::new("inspector-json-expand-all")
                    .variant(Variant::Ghost)
                    .size(HeroSize::Sm)
                    .is_icon_only(true)
                    .aria_label(s.show_all)
                    .on_press(cx.listener(|ws, _, _w, cx| {
                        ws.inspector_ui.collapsed_json_paths.clear();
                        cx.notify();
                    }))
                    .child(Icon::new(IconName::Expand).size(IconSize::Xs)),
            ),
        )
        .child(
            Button::new("inspector-json-collapse-all")
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .is_icon_only(true)
                .aria_label(s.inspector_tab_json)
                .on_press(cx.listener(move |ws, _, _w, cx| {
                    ws.inspector_ui.collapsed_json_paths = all_containers.clone();
                    cx.notify();
                }))
                .child(Icon::new(IconName::Minimize2).size(IconSize::Xs)),
        );

    if editable {
        controls = controls.child(render_json_format_apply_btn(
            cell,
            pretty_for_apply,
            s.inspector_apply_json,
            cx,
        ));
    }

    controls = controls.child(render_copy_button(
        "inspector-json-copy",
        ws.inspector_ui.copied_feedback,
        formatted_json,
        s,
        cx,
    ));

    let pretty_toggle = Button::new("inspector-json-pretty-toggle")
        .variant(if pretty {
            Variant::Secondary
        } else {
            Variant::Ghost
        })
        .size(HeroSize::Sm)
        .on_press(cx.listener(|ws, _, _w, cx| {
            ws.inspector_ui.raw_pretty_mode = !ws.inspector_ui.raw_pretty_mode;
            cx.notify();
        }))
        .child(if pretty { s.pretty_mode } else { s.raw_mode });

    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(4.0))
        .child(pretty_toggle)
        .child(controls)
        .into_any_element()
}

fn render_json_indent_guides(depth: usize, guide_color: Hsla) -> AnyElement {
    let mut guides = div().flex().items_center().flex_shrink_0();
    for _ in 0..depth {
        guides = guides.child(
            div()
                .w(px(10.0))
                .h(px(20.0))
                .border_l_1()
                .border_color(guide_color),
        );
    }
    guides.into_any_element()
}

fn render_json_chevron(
    node: &FlatJsonNode,
    idx: usize,
    muted_color: Hsla,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    if !node.kind.is_container() {
        return div()
            .w(px(14.0))
            .h(px(18.0))
            .flex_shrink_0()
            .into_any_element();
    }
    let path_for_toggle = node.path.clone();
    div()
        .id(("json-node-toggle", idx))
        .w(px(14.0))
        .h(px(18.0))
        .flex_shrink_0()
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .on_click(cx.listener(move |ws, _, _w, cx| {
            if ws
                .inspector_ui
                .collapsed_json_paths
                .contains(&path_for_toggle)
            {
                ws.inspector_ui
                    .collapsed_json_paths
                    .remove(&path_for_toggle);
            } else {
                ws.inspector_ui
                    .collapsed_json_paths
                    .insert(path_for_toggle.clone());
            }
            cx.notify();
        }))
        .child(
            Icon::new(if node.is_expanded {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .size(IconSize::Xs)
            .color(muted_color),
        )
        .into_any_element()
}

fn render_json_tree_row(
    ws: &TableMinusWorkspace,
    cell: &InspectedCell,
    node: &FlatJsonNode,
    idx: usize,
    editable: bool,
    value_font: herogpui::gpui::Font,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let guides = render_json_indent_guides(node.depth, with_alpha(colors.border, 0.75));
    let chevron = render_json_chevron(node, idx, colors.muted, cx);

    let mut content = div()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .items_center()
        .gap(px(4.0))
        .font(value_font)
        .text_size(px(11.5));

    if let Some(key) = node.key_label.as_ref() {
        content = content
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(ws.content_palette.editor_syntax_identifier)
                    .child(key.clone()),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(ws.content_palette.editor_syntax_punctuation)
                    .child(":"),
            );
    }

    let val_el = render_json_node_value_el(ws, cell, node, idx, editable, cx);
    content = content.child(val_el);

    let copy_path = node.path.clone();
    let path_btn = Tooltip::new(format!(
        "{} ({})",
        ws.controller.strings.inspector_copy_path, copy_path
    ))
    .child(
        div()
            .id(("json-copy-path", idx))
            .px(px(3.0))
            .py(px(2.0))
            .rounded(px(3.0))
            .cursor_pointer()
            .opacity(0.65)
            .hover(|s| s.opacity(1.0).bg(with_alpha(colors.foreground, 0.08)))
            .on_click(cx.listener(move |ws, _, _w, cx| {
                ws.inspector_ui.trigger_copy_feedback(copy_path.clone(), cx);
            }))
            .child(
                Icon::new(IconName::Link2)
                    .size(IconSize::Xs)
                    .color(colors.muted),
            ),
    );

    div()
        .id(("json-tree-row", idx))
        .w_full()
        .min_h(px(20.0))
        .px(px(2.0))
        .rounded(px(3.0))
        .flex()
        .items_center()
        .gap(px(2.0))
        .hover(|s| s.bg(with_alpha(colors.foreground, 0.04)))
        .child(guides)
        .child(chevron)
        .child(content)
        .child(path_btn)
        .into_any_element()
}

fn render_json_node_value_el(
    ws: &TableMinusWorkspace,
    _cell: &InspectedCell,
    node: &FlatJsonNode,
    idx: usize,
    editable: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let (display_text, color, is_italic) = json_node_display(&node.kind, &ws.content_palette);
    let base = div()
        .flex_1()
        .min_w(px(0.0))
        .truncate()
        .text_color(color)
        .when(is_italic, |el| el.italic())
        .child(display_text);

    if node.kind.is_container() || !editable {
        return base.into_any_element();
    }

    let edit_prefill = json_kind_to_edit_string(&node.kind);
    let path_str = node.path.clone();
    let segments = node.segments.clone();
    let colors = cx.colors().clone();
    div()
        .id(("json-leaf-edit", idx))
        .flex_1()
        .min_w(px(0.0))
        .px(px(3.0))
        .rounded(px(3.0))
        .cursor_pointer()
        .hover(|s| s.bg(with_alpha(colors.accent.color, 0.12)))
        .on_click(cx.listener(move |ws, _, _w, cx| {
            ws.inspector_ui.json_node_error = None;
            ws.inspector_ui.json_editing_path = Some((path_str.clone(), segments.clone()));
            let prefill = edit_prefill.clone();
            ws.inspector_ui.json_node_input.update(cx, |input, cx| {
                input.set_value(prefill);
                cx.notify();
            });
            cx.notify();
        }))
        .child(base)
        .into_any_element()
}

fn render_json_inline_node_editor(
    ws: &TableMinusWorkspace,
    cell: &InspectedCell,
    root: &serde_json::Value,
    value_font: herogpui::gpui::Font,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let Some((path_str, segments)) = ws.inspector_ui.json_editing_path.clone() else {
        return div().into_any_element();
    };
    let err_msg = ws.inspector_ui.json_node_error.clone();
    let entity_submit = cx.entity().clone();
    let submit_root = root.clone();
    let submit_segments = segments.clone();
    let submit_col_info = cell.col_info.clone();
    let submit_orig = cell.original_value.clone();
    let submit_row_idx = cell.row_idx;
    let submit_col_idx = cell.col_idx;
    let submit_edit_ctx = cell.edit_context;

    let mut input = Input::new(&ws.inspector_ui.json_node_input)
        .height(px(24.0))
        .text_size(px(11.5))
        .font_family(value_font.family)
        .placeholder(ws.controller.strings.edit_cell)
        .full_width()
        .is_invalid(err_msg.is_some())
        .on_submit(move |text, _w, cx| {
            let draft = text.to_owned();
            let next_root = submit_root.clone();
            let segs = submit_segments.clone();
            let col_info = submit_col_info.clone();
            let orig = submit_orig.clone();
            entity_submit.update(cx, |ws, cx| {
                commit_json_node_edit(
                    ws,
                    next_root,
                    &segs,
                    &draft,
                    submit_row_idx,
                    submit_col_idx,
                    &col_info,
                    submit_edit_ctx,
                    &orig,
                    cx,
                );
            });
        });
    if let Some(ref err) = err_msg {
        input = input.error_message(err.clone());
    }

    let actions = render_json_inline_editor_actions(ws, cell, root, segments, cx);

    div()
        .my(px(4.0))
        .p(px(6.0))
        .rounded(px(4.0))
        .bg(muted_surface_bg(cx))
        .border_l(px(2.0))
        .border_color(if err_msg.is_some() {
            colors.danger.color
        } else {
            colors.accent.color
        })
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .text_size(px(10.5))
                .text_color(colors.muted)
                .truncate()
                .child(path_str),
        )
        .child(input)
        .when_some(err_msg, |el, _| {
            el.child(
                div()
                    .px(px(6.0))
                    .py(px(4.0))
                    .rounded(px(4.0))
                    .bg(with_alpha(colors.danger.color, 0.08))
                    .border_1()
                    .border_color(with_alpha(colors.danger.color, 0.35))
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .size(IconSize::Xs)
                            .color(colors.danger.color),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.danger.color)
                            .child(ws.controller.strings.inspector_invalid_json),
                    ),
            )
        })
        .child(actions)
        .into_any_element()
}

fn render_json_inline_editor_actions(
    ws: &TableMinusWorkspace,
    cell: &InspectedCell,
    root: &serde_json::Value,
    segments: Vec<JsonPathSegment>,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let btn_root = root.clone();
    let btn_col_info = cell.col_info.clone();
    let btn_orig = cell.original_value.clone();
    let btn_row_idx = cell.row_idx;
    let btn_col_idx = cell.col_idx;
    let btn_edit_ctx = cell.edit_context;

    div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(
            Button::new("inspector-json-node-apply")
                .variant(Variant::Primary)
                .size(HeroSize::Sm)
                .on_press(cx.listener(move |ws, _, _w, cx| {
                    let draft = ws.inspector_ui.json_node_input.read(cx).value().to_owned();
                    commit_json_node_edit(
                        ws,
                        btn_root.clone(),
                        &segments,
                        &draft,
                        btn_row_idx,
                        btn_col_idx,
                        &btn_col_info,
                        btn_edit_ctx,
                        &btn_orig,
                        cx,
                    );
                }))
                .child(ws.controller.strings.inspector_apply_json),
        )
        .child(
            Button::new("inspector-json-node-cancel")
                .variant(Variant::Ghost)
                .size(HeroSize::Sm)
                .on_press(cx.listener(|ws, _, _w, cx| {
                    ws.inspector_ui.json_editing_path = None;
                    ws.inspector_ui.json_node_error = None;
                    cx.notify();
                }))
                .child(ws.controller.strings.cancel_label),
        )
        .into_any_element()
}

#[expect(
    clippy::too_many_arguments,
    reason = "explicit cell context avoids wrapping single-use edit struct"
)]
fn commit_json_node_edit(
    ws: &mut TableMinusWorkspace,
    mut root: serde_json::Value,
    segments: &[JsonPathSegment],
    draft: &str,
    row_idx: usize,
    col_idx: usize,
    col_info: &ColumnInfo,
    edit_ctx: EditContext,
    orig_val: &Value,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let parsed_val = match parse_json_node_edit(draft) {
        Ok(v) => v,
        Err(err) => {
            let invalid_label = ws.controller.strings.inspector_invalid_json;
            ws.inspector_ui.json_node_error = Some(if err.is_empty() {
                invalid_label.to_owned()
            } else {
                format!("{invalid_label}: {err}")
            });
            cx.notify();
            return;
        }
    };
    if let Err(err) = set_json_value_at_path(&mut root, segments, parsed_val) {
        ws.inspector_ui.json_node_error = Some(err);
        cx.notify();
        return;
    }
    let serialized = if ws.inspector_ui.raw_pretty_mode {
        serde_json::to_string_pretty(&root)
    } else {
        serde_json::to_string(&root)
    };
    let Ok(next_json_text) = serialized else {
        ws.inspector_ui.json_node_error =
            Some(ws.controller.strings.inspector_invalid_json.to_owned());
        cx.notify();
        return;
    };

    match parse_edit_text(&next_json_text, col_info, edit_ctx, Some(orig_val)) {
        Ok(cell_in) => {
            let next_value = cell_in
                .value()
                .cloned()
                .unwrap_or_else(|| Value::Text(next_json_text.clone()));
            apply_inspector_cell_edit(ws, row_idx, col_idx, next_value);
            ws.inspector_ui.json_editing_path = None;
            ws.inspector_ui.json_node_error = None;
            ws.inspector_ui.parse_error = None;
            ws.inspector_ui.last_sync_sig = None;
            cx.notify();
        }
        Err(err) => {
            ws.inspector_ui.json_node_error = Some(err.to_string());
            cx.notify();
        }
    }
}

fn render_hex_mode_panel(
    ws: &TableMinusWorkspace,
    cell: &InspectedCell,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let Some((bytes, source_kind)) = extract_bytes_for_hex(&cell.value, &cell.full_text) else {
        return render_viewer_fallback_panel(
            "NULL",
            cell.col_idx,
            &ws.controller.strings,
            false,
            cx,
        );
    };

    let bpr = if ws.inspector_ui.hex_bytes_per_row == 8 {
        8
    } else {
        16
    };
    let toolbar = render_hex_toolbar(ws, cell, &bytes, source_kind, bpr, cx);
    let hex_table = render_hex_dump_table(ws, &bytes, bpr, cx);

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(toolbar)
        .child(hex_table)
        .into_any_element()
}

fn render_hex_toolbar(
    ws: &TableMinusWorkspace,
    cell: &InspectedCell,
    bytes: &[u8],
    source_kind: HexSourceKind,
    bpr: usize,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &ws.controller.strings;
    let hex_copy_text = format_hex_dump_text(bytes, bpr, Some(HEX_PREVIEW_BYTE_LIMIT));
    let export_bytes = bytes.to_vec();
    let suggested_name = build_export_filename(&cell.col_name, None, &export_bytes);

    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(4.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(meta_chip(source_kind.badge(s), HeroColor::Default))
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(colors.muted)
                        .child(format_byte_size(bytes.len())),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(2.0))
                .child(
                    Button::new("inspector-hex-width")
                        .variant(Variant::Ghost)
                        .size(HeroSize::Sm)
                        .on_press(cx.listener(|ws, _, _w, cx| {
                            ws.inspector_ui.hex_bytes_per_row =
                                if ws.inspector_ui.hex_bytes_per_row == 16 {
                                    8
                                } else {
                                    16
                                };
                            cx.notify();
                        }))
                        .child(format!("{bpr}B")),
                )
                .child(
                    Tooltip::new(s.inspector_export_binary).child(
                        Button::new("inspector-hex-export")
                            .variant(Variant::Ghost)
                            .size(HeroSize::Sm)
                            .is_icon_only(true)
                            .aria_label(s.inspector_export_binary)
                            .on_press(cx.listener(move |ws, _, _w, cx| {
                                trigger_export_binary(
                                    ws,
                                    export_bytes.clone(),
                                    suggested_name.clone(),
                                    cx,
                                );
                            }))
                            .child(Icon::new(IconName::Download).size(IconSize::Xs)),
                    ),
                )
                .child(render_copy_button(
                    "inspector-hex-copy",
                    ws.inspector_ui.copied_feedback,
                    hex_copy_text,
                    s,
                    cx,
                )),
        )
        .into_any_element()
}

fn render_hex_dump_table(
    ws: &TableMinusWorkspace,
    bytes: &[u8],
    bpr: usize,
    cx: &App,
) -> AnyElement {
    let colors = cx.colors().clone();
    let s = &ws.controller.strings;
    let value_font = value_gpui_font(&ws.controller.settings);
    let rows = format_hex_dump_rows(bytes, bpr, Some(HEX_PREVIEW_BYTE_LIMIT));

    let mut hex_table = div()
        .id("inspector-hex-scroll")
        .w_full()
        .max_h(px(240.0))
        .overflow_y_scroll()
        .when(bpr == 16, |el| el.overflow_x_scroll())
        .p(px(8.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(ws.content_palette.editor_background)
        .border_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .flex_col()
        .gap(px(1.0));

    if rows.is_empty() {
        hex_table = hex_table.child(
            div()
                .font(value_font)
                .text_xs()
                .text_color(colors.muted)
                .child(s.empty_string_placeholder),
        );
        return hex_table.into_any_element();
    }

    for row in rows {
        hex_table = hex_table.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .font(value_font.clone())
                .text_size(px(10.5))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(ws.content_palette.editor_gutter_text)
                        .child(row.offset_hex),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(ws.content_palette.editor_syntax_number)
                        .child(row.hex_bytes),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(ws.content_palette.editor_syntax_string)
                        .child(row.ascii),
                ),
        );
    }

    if bytes.len() > HEX_PREVIEW_BYTE_LIMIT {
        hex_table = hex_table.child(
            div()
                .pt(px(4.0))
                .text_size(px(10.5))
                .text_color(colors.muted)
                .child(format!(
                    "{} {} {} {}",
                    s.showing_prefix,
                    format_byte_size(HEX_PREVIEW_BYTE_LIMIT),
                    s.of_prep,
                    format_byte_size(bytes.len())
                )),
        );
    }

    hex_table.into_any_element()
}

fn render_image_mode_panel(
    ws: &mut TableMinusWorkspace,
    cell: &InspectedCell,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let Some(bytes) = extract_binary_or_image_payload(&cell.value, &cell.full_text) else {
        let fallback_label = if cell.is_null {
            "NULL"
        } else if cell.full_text.trim().is_empty() {
            ws.controller.strings.empty_string_placeholder
        } else {
            ws.controller.strings.inspector_tab_image
        };
        return render_viewer_fallback_panel(
            fallback_label,
            cell.col_idx,
            &ws.controller.strings,
            false,
            cx,
        );
    };

    let meta = detect_image_metadata(&bytes);
    let suggested_name = build_export_filename(&cell.col_name, meta, &bytes);
    let header_bar = render_image_header_bar(
        meta,
        &bytes,
        suggested_name,
        ws.controller.strings.inspector_export_binary,
        cx,
    );
    let preview_box = render_image_preview_surface(ws, meta, &bytes, cx);
    let metadata_rows = render_image_metadata_rows(
        meta,
        bytes.len(),
        ws.controller.strings.inspector_image_dimensions,
        cx,
    );

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(header_bar)
        .child(preview_box)
        .child(metadata_rows)
        .into_any_element()
}

fn render_image_header_bar(
    meta: Option<ImageMetadata>,
    bytes: &[u8],
    suggested_name: String,
    export_label: &'static str,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let export_bytes = bytes.to_vec();
    let format_chip = if let Some(m) = meta {
        meta_chip(m.format.badge(), HeroColor::Accent)
    } else {
        meta_chip(detect_binary_signature_label(bytes), HeroColor::Default)
    };

    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(4.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(format_chip)
                .when_some(meta.and_then(|m| m.width.zip(m.height)), |el, (w, h)| {
                    el.child(
                        div()
                            .text_size(px(11.0))
                            .text_color(colors.foreground)
                            .child(format!("{w} × {h}")),
                    )
                })
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(colors.muted)
                        .child(format_byte_size(bytes.len())),
                ),
        )
        .child(
            Button::new("inspector-image-export")
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .on_press(cx.listener(move |ws, _, _w, cx| {
                    trigger_export_binary(ws, export_bytes.clone(), suggested_name.clone(), cx);
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(Icon::new(IconName::Download).size(IconSize::Xs))
                        .child(export_label),
                ),
        )
        .into_any_element()
}

fn render_image_preview_surface(
    ws: &mut TableMinusWorkspace,
    meta: Option<ImageMetadata>,
    bytes: &[u8],
    cx: &App,
) -> AnyElement {
    let Some(m) = meta else {
        return render_binary_header_summary(
            bytes,
            &ws.content_palette,
            &ws.controller.settings,
            cx,
        );
    };

    let fallback_muted = cx.colors().muted;
    let fallback_text = format!("{} · {}", m.format.badge(), format_byte_size(bytes.len()));
    let image_handle = ws.inspector_ui.ensure_preview_image(m.format, bytes);
    div()
        .w_full()
        .min_h(px(120.0))
        .max_h(px(210.0))
        .p(px(8.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(ws.content_palette.editor_background)
        .border_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .items_center()
        .justify_center()
        .child(
            img(ImageSource::Image(image_handle))
                .max_w_full()
                .max_h(px(190.0))
                .object_fit(ObjectFit::Contain)
                .with_fallback(move || {
                    div()
                        .text_xs()
                        .text_color(fallback_muted)
                        .text_center()
                        .child(fallback_text.clone())
                        .into_any_element()
                }),
        )
        .into_any_element()
}

fn render_image_metadata_rows(
    meta: Option<ImageMetadata>,
    byte_len: usize,
    dimensions_label: &'static str,
    cx: &App,
) -> AnyElement {
    let dims = match meta.and_then(|m| m.width.zip(m.height)) {
        Some((w, h)) => format!(
            "{w} × {h} px · {} ({byte_len} B)",
            format_byte_size(byte_len)
        ),
        None => format!("— · {} ({byte_len} B)", format_byte_size(byte_len)),
    };

    div()
        .w_full()
        .px(px(8.0))
        .py(px(6.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(muted_surface_bg(cx))
        .border_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(kv_info_row(dimensions_label, dims, cx))
        .into_any_element()
}

fn render_binary_header_summary(
    bytes: &[u8],
    content_palette: &super::theme::ContentThemePalette,
    settings: &tableminus_windows_platform::AppSettings,
    cx: &App,
) -> AnyElement {
    let colors = cx.colors().clone();
    let value_font = value_gpui_font(settings);
    let preview_len = bytes.len().min(16);
    let mut magic_hex = String::new();
    for (i, b) in bytes[..preview_len].iter().enumerate() {
        if i > 0 {
            magic_hex.push(' ');
        }
        let _ = write!(magic_hex, "{b:02X}");
    }

    div()
        .w_full()
        .p(px(10.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(content_palette.editor_background)
        .border_1()
        .border_color(hairline_border_color(cx))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(6.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            Icon::new(IconName::File)
                                .size(IconSize::Sm)
                                .color(colors.muted),
                        )
                        .child(meta_chip(
                            detect_binary_signature_label(bytes),
                            HeroColor::Default,
                        )),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(colors.muted)
                        .child(format_byte_size(bytes.len())),
                ),
        )
        .when(!magic_hex.is_empty(), |el| {
            el.child(
                div()
                    .font(value_font)
                    .text_size(px(11.0))
                    .text_color(content_palette.editor_syntax_number)
                    .child(magic_hex),
            )
        })
        .into_any_element()
}

fn kv_info_row(label: &str, value: String, cx: &App) -> AnyElement {
    let colors = cx.colors().clone();
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .text_size(px(11.0))
        .child(div().text_color(colors.muted).child(label.to_owned()))
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.foreground)
                .child(value),
        )
        .into_any_element()
}

fn render_viewer_fallback_panel(
    message: &'static str,
    col_idx: usize,
    s: &crate::i18n::Strings,
    is_alert: bool,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let (bg, border, text_color) = if is_alert {
        (
            with_alpha(colors.danger.color, 0.08),
            with_alpha(colors.danger.color, 0.35),
            colors.danger.color,
        )
    } else {
        (
            muted_surface_bg(cx),
            hairline_border_color(cx),
            colors.muted,
        )
    };

    div()
        .w_full()
        .p(px(12.0))
        .rounded(px(CONTROL_RADIUS_PX))
        .bg(bg)
        .border_1()
        .border_color(border)
        .flex()
        .flex_col()
        .items_start()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .when(is_alert, |el| {
                    el.child(
                        Icon::new(IconName::TriangleAlert)
                            .size(IconSize::Xs)
                            .color(colors.danger.color),
                    )
                })
                .child(
                    div()
                        .text_xs()
                        .when(is_alert, |el| el.font_weight(FontWeight::MEDIUM))
                        .text_color(text_color)
                        .child(message),
                ),
        )
        .child(
            Button::new("inspector-fallback-text")
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .on_press(cx.listener(move |ws, _, _w, cx| {
                    ws.inspector_ui.viewer_mode = RichViewerMode::Text;
                    ws.inspector_ui.user_mode_override = Some((col_idx, RichViewerMode::Text));
                    cx.notify();
                }))
                .child(s.inspector_tab_text),
        )
        .into_any_element()
}

fn trigger_export_binary(
    ws: &mut TableMinusWorkspace,
    bytes: Vec<u8>,
    suggested_name: String,
    cx: &mut Context<TableMinusWorkspace>,
) {
    let start_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let rx = cx.prompt_for_new_path(&start_dir, Some(&suggested_name));
    ws.inspector_ui.export_binary_task = Some(cx.spawn(
        async move |this: WeakEntity<TableMinusWorkspace>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(path))) = rx.await else {
                return;
            };
            match std::fs::write(&path, &bytes) {
                Ok(()) => {
                    let _ = this.update(cx, |ws, cx| {
                        push_success_toast(
                            ws.controller.strings.export_complete_toast,
                            Some(SharedString::from(path.display().to_string())),
                            cx,
                        );
                    });
                }
                Err(err) => {
                    let _ = this.update(cx, |ws, cx| {
                        push_error_toast(
                            ws.controller.strings.export_failed_toast,
                            Some(SharedString::from(err.to_string())),
                            cx,
                        );
                    });
                }
            }
        },
    ));
}

fn render_fk_jump_row(
    ws: &TableMinusWorkspace,
    cell: &InspectedCell,
    cx: &mut Context<TableMinusWorkspace>,
) -> Option<AnyElement> {
    if let Some(fk) = cell.outbound_fk.as_ref() {
        let target_label = if let Some(schema) = fk.key.ref_schema.as_ref() {
            format!("{schema}.{}", fk.key.ref_table)
        } else {
            fk.key.ref_table.clone()
        };
        let source_table = ws.controller.active_table.clone()?;
        let fk_clone = fk.clone();
        let row_cols = cell.row_columns.clone();
        let row_vals = cell.row_values.clone();
        let can_jump = !cell.is_null;

        return Some(
            Button::new("inspector-fk-jump")
                .variant(Variant::Secondary)
                .size(HeroSize::Sm)
                .full_width(true)
                .is_disabled(!can_jump)
                .on_press(cx.listener(move |ws, _, _window, cx| {
                    jump_to_foreign_key(ws, &source_table, &fk_clone, &row_cols, &row_vals, cx);
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(6.0))
                        .child(Icon::new(IconName::Link2).size(IconSize::Xs))
                        .child(if can_jump {
                            ws.controller
                                .strings
                                .fk_jump_to_tpl
                                .replace("{target}", &target_label)
                        } else {
                            ws.controller
                                .strings
                                .fk_null_target_tpl
                                .replace("{target}", &target_label)
                        }),
                )
                .into_any_element(),
        );
    }

    if cell.is_fk_cell {
        let colors = cx.colors().clone();
        return Some(
            div()
                .px(px(10.0))
                .py(px(6.0))
                .rounded(px(CONTROL_RADIUS_PX))
                .bg(muted_surface_bg(cx))
                .text_xs()
                .text_color(colors.muted)
                .child(if ws.inspector_ui.metadata_loading {
                    ws.controller.strings.fk_resolving_metadata
                } else {
                    ws.controller.strings.fk_metadata_unavailable_adhoc
                })
                .into_any_element(),
        );
    }

    None
}

fn jump_to_foreign_key(
    ws: &mut TableMinusWorkspace,
    source_table: &TableRef,
    fk: &OutboundForeignKey,
    columns: &[ResultColumn],
    values: &[Value],
    cx: &mut Context<TableMinusWorkspace>,
) {
    let reference = match RowReference::referenced(source_table, fk, columns, values) {
        Ok(r) => r,
        Err(err) => {
            push_error_toast(
                ws.controller.strings.fk_jump_unavailable_toast,
                Some(SharedString::from(err.to_string())),
                cx,
            );
            return;
        }
    };
    if let Err(err) = ws
        .controller
        .open_table_tab_blocking(reference.table.clone())
    {
        push_error_toast(
            ws.controller.strings.fk_open_failed_toast,
            Some(SharedString::from(err.to_string())),
            cx,
        );
        return;
    }
    for rule in reference.filter.rules {
        let val = match rule.value {
            Some(FilterValue::Single(v)) => Some(v),
            Some(FilterValue::Pair(a, _)) => Some(a),
            Some(FilterValue::List(items)) => items.into_iter().next(),
            None => None,
        };
        if let Err(err) = ws
            .controller
            .add_grid_filter_blocking(&rule.column, rule.op, val)
        {
            push_error_toast(
                ws.controller.strings.fk_filter_failed_toast,
                Some(SharedString::from(err.to_string())),
                cx,
            );
            return;
        }
    }
    push_success_toast(
        ws.controller.strings.fk_opened_row_toast,
        Some(SharedString::from(reference.table.name)),
        cx,
    );
    cx.notify();
}

fn render_inline_editor_section(
    ws: &TableMinusWorkspace,
    cell: &InspectedCell,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let row_idx = cell.row_idx;
    let col_idx = cell.col_idx;
    let col_info = cell.col_info.clone();
    let edit_ctx = cell.edit_context;
    let orig_val = cell.original_value.clone();
    let value_font = value_gpui_font(&ws.controller.settings);

    let null_switch = Switch::new("inspector-null-switch")
        .size(HeroSize::Sm)
        .is_selected(cell.is_null)
        .is_disabled(!cell.col_info.nullable || cell.is_read_only || cell.is_deleted)
        .label(ws.controller.strings.set_null)
        .on_change(cx.listener(move |ws, &make_null: &bool, _window, cx| {
            let next_val = if make_null {
                Value::Null
            } else {
                let draft = ws.inspector_ui.editor_input.read(cx).value().to_owned();
                match parse_edit_text(&draft, &col_info, edit_ctx, Some(&orig_val)) {
                    Ok(cell_in) => cell_in.value().cloned().unwrap_or(Value::Text(draft)),
                    Err(_) => Value::Text(String::new()),
                }
            };
            apply_inspector_cell_edit(ws, row_idx, col_idx, next_val);
            ws.inspector_ui.parse_error = None;
            cx.notify();
        }));

    let err_msg = ws.inspector_ui.parse_error.clone();
    let mut area = TextArea::new(&ws.inspector_ui.editor_input)
        .rows(4)
        .font_family(value_font.family)
        .full_width()
        .is_read_only(cell.is_read_only || cell.is_null || cell.is_deleted)
        .is_invalid(err_msg.is_some());
    if let Some(hint) = cell.read_only_hint.as_ref() {
        area = area.description(hint.clone());
    }
    if let Some(msg) = err_msg {
        area = area.error_message(msg);
    }

    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.muted)
                        .child(ws.controller.strings.edit_cell),
                )
                .child(null_switch),
        )
        .child(area)
        .child(render_inline_editor_actions(ws, cell, cx))
        .into_any_element()
}

fn render_inline_editor_actions(
    ws: &TableMinusWorkspace,
    cell: &InspectedCell,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let row_idx = cell.row_idx;
    let col_idx = cell.col_idx;
    let edit_ctx = cell.edit_context;
    let apply_col = cell.col_info.clone();
    let apply_orig = cell.original_value.clone();
    let apply_btn = Button::new("inspector-apply-edit")
        .variant(Variant::Primary)
        .size(HeroSize::Sm)
        .is_disabled(cell.is_read_only || cell.is_null || cell.is_deleted)
        .on_press(cx.listener(move |ws, _, _window, cx| {
            let draft = ws.inspector_ui.editor_input.read(cx).value().to_owned();
            match parse_edit_text(&draft, &apply_col, edit_ctx, Some(&apply_orig)) {
                Ok(cell_input) => {
                    let next_value = cell_input.value().cloned().unwrap_or(Value::Text(draft));
                    apply_inspector_cell_edit(ws, row_idx, col_idx, next_value);
                    ws.inspector_ui.parse_error = None;
                    cx.notify();
                }
                Err(err) => {
                    ws.inspector_ui.parse_error = Some(err.to_string());
                    cx.notify();
                }
            }
        }))
        .child(ws.controller.strings.apply_edit_label);

    let revert_orig = cell.original_value.clone();
    let show_revert = cell.is_modified || ws.inspector_ui.parse_error.is_some();

    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(apply_btn)
        .when(show_revert, |el| {
            el.child(
                Button::new("inspector-revert-edit")
                    .variant(Variant::Ghost)
                    .size(HeroSize::Sm)
                    .on_press(cx.listener(move |ws, _, _window, cx| {
                        apply_inspector_cell_edit(ws, row_idx, col_idx, revert_orig.clone());
                        ws.inspector_ui.parse_error = None;
                        ws.inspector_ui.last_sync_sig = None;
                        cx.notify();
                    }))
                    .child(ws.controller.strings.revert_label),
            )
        })
        .into_any_element()
}

fn render_row_summary_list(
    ws: &TableMinusWorkspace,
    active: &InspectedCell,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let value_font = value_gpui_font(&ws.controller.settings);
    let mut list = div().flex().flex_col().gap(px(2.0)).child(
        div()
            .pb(px(4.0))
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(colors.muted)
            .child(
                ws.controller
                    .strings
                    .inspector_row_fields_tpl
                    .replace("{count}", &active.row_columns.len().to_string()),
            ),
    );

    for (idx, (col, val)) in active
        .row_columns
        .iter()
        .zip(active.row_values.iter())
        .enumerate()
    {
        let is_selected = idx == active.col_idx;
        let is_null = matches!(val, Value::Null);
        let preview = if is_null {
            "NULL".to_owned()
        } else {
            value_text(val, &col.column_type).unwrap_or_else(|| format!("{val:?}"))
        };
        let row_idx = active.row_idx;
        let bg = if is_selected {
            with_alpha(colors.accent.color, 0.12)
        } else {
            with_alpha(colors.foreground, 0.0)
        };
        let left_accent = if is_selected {
            colors.accent.color
        } else {
            hairline_border_color(cx)
        };

        list = list.child(
            div()
                .id(("inspector-row-field", idx))
                .cursor_pointer()
                .px(px(8.0))
                .py(px(5.0))
                .rounded(px(4.0))
                .bg(bg)
                .border_l(px(if is_selected { 2.0 } else { 1.0 }))
                .border_color(left_accent)
                .when(!is_selected, |el| {
                    el.hover(|s| s.bg(with_alpha(colors.foreground, 0.04)))
                })
                .on_click(cx.listener(move |ws, _, _window, cx| {
                    ws.controller.select_grid_cell(row_idx, idx);
                    ws.inspector_ui.parse_error = None;
                    cx.notify();
                }))
                .flex()
                .flex_col()
                .gap(px(1.0))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.foreground)
                        .child(col.name.clone()),
                )
                .child(
                    div()
                        .font(value_font.clone())
                        .text_xs()
                        .text_color(if is_null {
                            ws.content_palette.editor_syntax_null
                        } else {
                            colors.muted
                        })
                        .when(is_null, |el| el.italic())
                        .truncate()
                        .child(preview),
                ),
        );
    }

    list.into_any_element()
}

fn apply_inspector_cell_edit(
    ws: &mut TableMinusWorkspace,
    row_idx: usize,
    col_idx: usize,
    new_value: Value,
) {
    let raw_len = ws.controller.grid_raw_rows.len();
    if row_idx < raw_len {
        ws.controller.edit_grid_cell(row_idx, col_idx, new_value);
        return;
    }
    let inserted_offset = row_idx - raw_len;
    if let Some((_row_id, values)) = ws
        .controller
        .grid_model
        .edits
        .inserted_rows
        .iter_mut()
        .nth(inserted_offset)
        && let Some(slot) = values.get_mut(col_idx)
    {
        *slot = new_value;
        ws.controller.sync_grid_view_from_model();
    }
}

fn format_display_value(cell: &InspectedCell, pretty: bool) -> String {
    if !pretty || cell.is_null {
        return cell.full_text.clone();
    }
    serde_json::from_str::<serde_json::Value>(&cell.full_text)
        .and_then(|v| serde_json::to_string_pretty(&v))
        .unwrap_or_else(|_| cell.full_text.clone())
}

fn resolve_inspected_cell(ws: &TableMinusWorkspace) -> Option<InspectedCell> {
    let (row_idx, mut col_idx) = ws.controller.grid_selection.focused?;
    let total_rows =
        ws.controller.grid_raw_rows.len() + ws.controller.grid_model.edits.inserted_rows.len();
    if row_idx >= total_rows {
        return None;
    }

    let row_columns = build_result_columns(ws);
    if row_columns.is_empty() {
        return None;
    }
    if col_idx >= row_columns.len() {
        col_idx = 0;
    }

    let (row_values, original_row, is_inserted, is_deleted, row_id) =
        extract_row_values(ws, row_idx, row_columns.len());
    let value = row_values.get(col_idx).cloned().unwrap_or(Value::Null);
    let original_value = original_row.get(col_idx).cloned().unwrap_or(Value::Null);

    let col_id = ws
        .controller
        .grid_model
        .columns
        .columns()
        .get(col_idx)
        .map(|c| c.id);
    let is_modified = matches!((row_id, col_id), (Some(rid), Some(cid))
        if ws.controller.grid_model.edits.modified_cells.contains_key(&(rid, cid)));

    let rcol = &row_columns[col_idx];
    let col_info = resolve_column_info(ws, &rcol.name, &rcol.column_type);
    let edit_txt = edit_text(&value, &rcol.column_type);
    let (full_text, type_read_only_hint) = match edit_txt {
        EditText::Editable(s) => (s, None),
        EditText::ReadOnly(reason) => (
            value_text(&value, &rcol.column_type).unwrap_or_else(|| format!("{value:?}")),
            Some(
                ws.controller
                    .strings
                    .read_only_reason_tpl
                    .replace("{reason}", &format!("{reason:?}")),
            ),
        ),
    };
    let generated_hint =
        (!col_info.is_writable()).then(|| ws.controller.strings.generated_col_readonly.to_owned());
    let read_only_hint = type_read_only_hint.or(generated_hint);
    let is_read_only = read_only_hint.is_some();

    let outbound_fk = ws
        .inspector_ui
        .cached_fks
        .iter()
        .find(|fk| fk.key.columns.iter().any(|c| c == &rcol.name))
        .cloned();
    let is_fk_cell = ws
        .controller
        .grid_rows
        .get(row_idx)
        .and_then(|r| r.cells.get(col_idx))
        .is_some_and(|c| matches!(c.kind, CellKind::ForeignKey))
        || ws
            .controller
            .grid_rows
            .get(row_idx)
            .is_some_and(|r| r.state == RowState::Modified && outbound_fk.is_some());

    Some(InspectedCell {
        row_idx,
        col_idx,
        col_name: rcol.name.clone(),
        col_type: rcol.column_type.clone(),
        col_info,
        is_null: matches!(value, Value::Null),
        value,
        original_value,
        full_text,
        is_read_only,
        read_only_hint,
        is_modified,
        is_inserted,
        is_deleted,
        edit_context: if is_inserted {
            EditContext::InsertDraft
        } else {
            EditContext::UpdateRow
        },
        outbound_fk,
        is_fk_cell,
        row_columns,
        row_values,
    })
}

fn build_result_columns(ws: &TableMinusWorkspace) -> Vec<ResultColumn> {
    if !ws.controller.grid_raw_columns.is_empty() {
        return ws.controller.grid_raw_columns.clone();
    }
    ws.controller
        .grid_columns
        .iter()
        .map(|col| ResultColumn::new(&col.title, ColumnType::unknown()))
        .collect()
}

fn extract_row_values(
    ws: &TableMinusWorkspace,
    row_idx: usize,
    col_len: usize,
) -> (Vec<Value>, Vec<Value>, bool, bool, Option<RowID>) {
    let raw_len = ws.controller.grid_raw_rows.len();
    if row_idx < raw_len {
        let orig = ws
            .controller
            .grid_raw_rows
            .get(row_idx)
            .cloned()
            .unwrap_or_default();
        let row_id = ws.controller.grid_model.row_order.row_id(row_idx);
        let is_deleted =
            row_id.is_some_and(|id| ws.controller.grid_model.edits.deleted_rows.contains(&id));
        let mut effective = Vec::with_capacity(col_len);
        for c_idx in 0..col_len {
            let col_id = ws
                .controller
                .grid_model
                .columns
                .columns()
                .get(c_idx)
                .map(|c| c.id);
            let val = match (row_id, col_id) {
                (Some(rid), Some(cid)) => ws
                    .controller
                    .grid_model
                    .edits
                    .modified_cells
                    .get(&(rid, cid))
                    .cloned()
                    .or_else(|| orig.get(c_idx).cloned())
                    .unwrap_or(Value::Null),
                _ => orig.get(c_idx).cloned().unwrap_or(Value::Null),
            };
            effective.push(val);
        }
        return (effective, orig, false, is_deleted, row_id);
    }

    let inserted = ws
        .controller
        .grid_model
        .edits
        .inserted_rows
        .iter()
        .nth(row_idx - raw_len)
        .map(|(id, vals)| (*id, vals.clone()));
    match inserted {
        Some((rid, vals)) => (vals.clone(), vals, true, false, Some(rid)),
        None => (vec![Value::Null; col_len], Vec::new(), true, false, None),
    }
}

fn resolve_column_info(ws: &TableMinusWorkspace, name: &str, rcol_type: &ColumnType) -> ColumnInfo {
    if let Some(info) = ws
        .inspector_ui
        .cached_columns
        .iter()
        .find(|c| c.name == name)
    {
        return info.clone();
    }
    if let Some(sc) = ws
        .controller
        .structure_columns
        .iter()
        .find(|c| c.name == name)
    {
        return ColumnInfo {
            name: sc.name.clone(),
            column_type: if rcol_type.name.is_empty() {
                ColumnType {
                    name: sc.data_type.clone(),
                    kind: rcol_type.kind.clone(),
                    catalog: rcol_type.catalog.clone(),
                    dynamic_storage: rcol_type.dynamic_storage,
                    read_form: ReadForm::Native,
                }
            } else {
                rcol_type.clone()
            },
            nullable: sc.nullable,
            primary_key: sc.is_primary_key,
            is_auto_increment: false,
            is_generated: false,
            partition_scoped: false,
            default: ColumnDefault::None,
            comment: None,
        };
    }
    ColumnInfo {
        name: name.to_owned(),
        column_type: rcol_type.clone(),
        nullable: true,
        primary_key: false,
        is_auto_increment: false,
        is_generated: false,
        partition_scoped: false,
        default: ColumnDefault::None,
        comment: None,
    }
}

// ============================================================================
// Pure Rich Value Viewer Helpers (Mode Detection, JSON Tree, Hex, Image/Binary)
// ============================================================================

/// Chooses the most informative Rich Value Viewer tab when focusing a new cell.
#[must_use]
pub fn detect_recommended_mode(value: &Value, full_text: &str) -> RichViewerMode {
    if matches!(value, Value::Null) {
        return RichViewerMode::Text;
    }
    if let Some(bytes) = extract_binary_or_image_payload(value, full_text)
        && detect_image_metadata(&bytes).is_some()
    {
        return RichViewerMode::Image;
    }
    if matches!(value, Value::Bytes(_)) {
        return RichViewerMode::Hex;
    }
    if matches!(value, Value::Json(_)) {
        return RichViewerMode::JsonTree;
    }
    if let Some(json) = parse_json_value(value, full_text)
        && matches!(
            json,
            serde_json::Value::Object(_) | serde_json::Value::Array(_)
        )
    {
        return RichViewerMode::JsonTree;
    }
    RichViewerMode::Text
}

/// Parses a JSON document from `value` or `full_text` if valid.
#[must_use]
pub fn parse_json_value(value: &Value, full_text: &str) -> Option<serde_json::Value> {
    match value {
        Value::Null => None,
        Value::Json(jt) => serde_json::from_str(jt.as_str()).ok(),
        Value::Bytes(bytes) => std::str::from_utf8(bytes)
            .ok()
            .and_then(|s| serde_json::from_str(s.trim()).ok()),
        _ => {
            let trimmed = full_text.trim();
            if trimmed.is_empty() {
                None
            } else {
                serde_json::from_str(trimmed).ok()
            }
        }
    }
}

/// Formats `segments` into canonical JSONPath notation (`$.foo[0]["bar baz"]`).
#[must_use]
pub fn format_json_path(segments: &[JsonPathSegment]) -> String {
    let mut out = String::from("$");
    for seg in segments {
        match seg {
            JsonPathSegment::Key(key) => {
                let is_simple_ident = !key.is_empty()
                    && key.chars().enumerate().all(|(i, c)| {
                        if i == 0 {
                            c.is_ascii_alphabetic() || c == '_'
                        } else {
                            c.is_ascii_alphanumeric() || c == '_'
                        }
                    });
                if is_simple_ident {
                    out.push('.');
                    out.push_str(key);
                } else {
                    let escaped = key.replace('\\', "\\\\").replace('"', "\\\"");
                    let _ = write!(out, "[\"{escaped}\"]");
                }
            }
            JsonPathSegment::Index(idx) => {
                let _ = write!(out, "[{idx}]");
            }
        }
    }
    out
}

/// Flattens a [`serde_json::Value`] tree respecting collapsed container paths.
#[must_use]
pub fn flatten_json_tree(
    root: &serde_json::Value,
    collapsed: &BTreeSet<String>,
) -> Vec<FlatJsonNode> {
    let mut out = Vec::new();
    let mut segments = Vec::new();
    flatten_json_node(root, None, 0, &mut segments, collapsed, &mut out);
    out
}

fn flatten_json_node(
    value: &serde_json::Value,
    key_label: Option<String>,
    depth: usize,
    segments: &mut Vec<JsonPathSegment>,
    collapsed: &BTreeSet<String>,
    out: &mut Vec<FlatJsonNode>,
) {
    let path = format_json_path(segments);
    let kind = match value {
        serde_json::Value::Object(map) => JsonNodeKind::Object { len: map.len() },
        serde_json::Value::Array(items) => JsonNodeKind::Array { len: items.len() },
        serde_json::Value::String(s) => JsonNodeKind::String(s.clone()),
        serde_json::Value::Number(n) => JsonNodeKind::Number(n.to_string()),
        serde_json::Value::Bool(b) => JsonNodeKind::Bool(*b),
        serde_json::Value::Null => JsonNodeKind::Null,
    };
    let is_expanded = !collapsed.contains(&path);

    out.push(FlatJsonNode {
        path,
        segments: segments.clone(),
        depth,
        key_label,
        kind,
        is_expanded,
    });

    if !is_expanded {
        return;
    }

    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                segments.push(JsonPathSegment::Key(k.clone()));
                flatten_json_node(v, Some(k.clone()), depth + 1, segments, collapsed, out);
                segments.pop();
            }
        }
        serde_json::Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                segments.push(JsonPathSegment::Index(i));
                flatten_json_node(
                    v,
                    Some(format!("[{i}]")),
                    depth + 1,
                    segments,
                    collapsed,
                    out,
                );
                segments.pop();
            }
        }
        _ => {}
    }
}

/// Collects all container node paths (`Object` / `Array`) for `Collapse All`.
#[must_use]
pub fn collect_container_paths(root: &serde_json::Value) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    let mut segments = Vec::new();
    collect_container_paths_rec(root, &mut segments, &mut paths);
    paths
}

fn collect_container_paths_rec(
    value: &serde_json::Value,
    segments: &mut Vec<JsonPathSegment>,
    out: &mut BTreeSet<String>,
) {
    match value {
        serde_json::Value::Object(map) => {
            out.insert(format_json_path(segments));
            for (k, v) in map {
                segments.push(JsonPathSegment::Key(k.clone()));
                collect_container_paths_rec(v, segments, out);
                segments.pop();
            }
        }
        serde_json::Value::Array(items) => {
            out.insert(format_json_path(segments));
            for (i, v) in items.iter().enumerate() {
                segments.push(JsonPathSegment::Index(i));
                collect_container_paths_rec(v, segments, out);
                segments.pop();
            }
        }
        _ => {}
    }
}

fn json_kind_to_edit_string(kind: &JsonNodeKind) -> String {
    match kind {
        JsonNodeKind::String(s) => serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\"")),
        JsonNodeKind::Number(n) => n.clone(),
        JsonNodeKind::Bool(b) => b.to_string(),
        JsonNodeKind::Null => "null".to_owned(),
        JsonNodeKind::Object { .. } => "{}".to_owned(),
        JsonNodeKind::Array { .. } => "[]".to_owned(),
    }
}

fn json_node_display(
    kind: &JsonNodeKind,
    palette: &super::theme::ContentThemePalette,
) -> (String, Hsla, bool) {
    match kind {
        JsonNodeKind::Object { len } => (
            format!("{{{len}}}"),
            palette.editor_syntax_punctuation,
            false,
        ),
        JsonNodeKind::Array { len } => {
            (format!("[{len}]"), palette.editor_syntax_punctuation, false)
        }
        JsonNodeKind::String(s) => (format!("\"{s}\""), palette.editor_syntax_string, false),
        JsonNodeKind::Number(n) => (n.clone(), palette.editor_syntax_number, false),
        JsonNodeKind::Bool(b) => (b.to_string(), palette.editor_syntax_keyword, false),
        JsonNodeKind::Null => ("null".to_owned(), palette.editor_syntax_null, true),
    }
}

/// Parses an inline JSON node edit string into a [`serde_json::Value`].
pub fn parse_json_node_edit(raw: &str) -> Result<serde_json::Value, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Enter a JSON literal (e.g. \"text\", 42, true, null)".to_owned());
    }
    serde_json::from_str::<serde_json::Value>(trimmed).map_err(|err| err.to_string())
}

/// Updates the node at `path` inside `root` with `new_val`.
pub fn set_json_value_at_path(
    root: &mut serde_json::Value,
    path: &[JsonPathSegment],
    new_val: serde_json::Value,
) -> Result<(), String> {
    let mut cursor = root;
    for seg in path {
        match (cursor, seg) {
            (serde_json::Value::Object(map), JsonPathSegment::Key(key)) => {
                cursor = map
                    .get_mut(key)
                    .ok_or_else(|| format!("Key '{key}' not found in JSON object"))?;
            }
            (serde_json::Value::Array(items), JsonPathSegment::Index(idx)) => {
                cursor = items
                    .get_mut(*idx)
                    .ok_or_else(|| format!("Index [{idx}] out of bounds"))?;
            }
            _ => return Err("JSON path does not match node container".to_owned()),
        }
    }
    *cursor = new_val;
    Ok(())
}

/// Extracts bytes for the Hex viewer from `Value::Bytes`, prefixed hex literals, or UTF-8 text.
#[must_use]
pub fn extract_bytes_for_hex(value: &Value, full_text: &str) -> Option<(Vec<u8>, HexSourceKind)> {
    match value {
        Value::Null => None,
        Value::Bytes(bytes) => Some((bytes.clone(), HexSourceKind::RawBytes)),
        _ => {
            if let Some(decoded) = decode_prefixed_hex_literal(full_text) {
                return Some((decoded, HexSourceKind::DecodedHexString));
            }
            Some((full_text.as_bytes().to_vec(), HexSourceKind::Utf8Bytes))
        }
    }
}

fn decode_prefixed_hex_literal(raw: &str) -> Option<Vec<u8>> {
    let trimmed = raw.trim();
    let hex_digits = if let Some(rest) = trimmed
        .strip_prefix("\\x")
        .or_else(|| trimmed.strip_prefix("\\X"))
        .or_else(|| trimmed.strip_prefix("0x"))
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        rest
    } else {
        trimmed
            .strip_prefix("X'")
            .or_else(|| trimmed.strip_prefix("x'"))
            .and_then(|s| s.strip_suffix('\''))?
    };
    decode_hex_pairs(hex_digits)
}

fn decode_hex_pairs(hex: &str) -> Option<Vec<u8>> {
    let cleaned: String = hex.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if cleaned.is_empty() || !cleaned.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(cleaned.len() / 2);
    let (pairs, _) = cleaned.as_bytes().as_chunks::<2>();
    for pair in pairs {
        let hi = hex_nibble(pair[0])?;
        let lo = hex_nibble(pair[1])?;
        out.push((hi << 4) | lo);
    }
    Some(out)
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Formats `bytes` into aligned 8-byte or 16-byte [`HexRow`] entries.
#[must_use]
pub fn format_hex_dump_rows(
    bytes: &[u8],
    bytes_per_row: usize,
    max_bytes: Option<usize>,
) -> Vec<HexRow> {
    let width = if bytes_per_row == 16 { 16 } else { 8 };
    let limit = max_bytes.map_or(bytes.len(), |m| m.min(bytes.len()));
    let slice = &bytes[..limit];
    let mut rows = Vec::with_capacity(slice.len().div_ceil(width));

    for (chunk_idx, chunk) in slice.chunks(width).enumerate() {
        let offset = chunk_idx * width;
        let offset_hex = format!("{offset:08X}");
        let mut hex_bytes = String::with_capacity(width * 3 + 1);
        let mut ascii = String::with_capacity(width);

        for i in 0..width {
            if i > 0 {
                hex_bytes.push(' ');
                if width == 16 && i == 8 {
                    hex_bytes.push(' ');
                }
            }
            if let Some(&b) = chunk.get(i) {
                let _ = write!(hex_bytes, "{b:02X}");
                let ch = if (0x20..=0x7E).contains(&b) {
                    char::from(b)
                } else {
                    '.'
                };
                ascii.push(ch);
            } else {
                hex_bytes.push_str("  ");
            }
        }

        rows.push(HexRow {
            offset,
            offset_hex,
            hex_bytes,
            ascii,
        });
    }
    rows
}

/// Formats `bytes` into a plain-text aligned hex dump string for clipboard copy.
#[must_use]
pub fn format_hex_dump_text(
    bytes: &[u8],
    bytes_per_row: usize,
    max_bytes: Option<usize>,
) -> String {
    let rows = format_hex_dump_rows(bytes, bytes_per_row, max_bytes);
    let mut out = String::new();
    for (idx, row) in rows.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        let _ = write!(out, "{}  {}  {}", row.offset_hex, row.hex_bytes, row.ascii);
    }
    out
}

/// Extracts raw binary/image bytes from `Value::Bytes`, data URIs, SVG XML, hex literals, or base64.
#[must_use]
pub fn extract_binary_or_image_payload(value: &Value, full_text: &str) -> Option<Vec<u8>> {
    match value {
        Value::Null => None,
        Value::Bytes(bytes) => Some(bytes.clone()),
        _ => {
            let trimmed = full_text.trim();
            if trimmed.is_empty() {
                return None;
            }
            if let Some(data_uri_bytes) = decode_data_uri(trimmed) {
                return Some(data_uri_bytes);
            }
            if looks_like_svg(trimmed.as_bytes()) {
                return Some(trimmed.as_bytes().to_vec());
            }
            if let Some(hex_bytes) = decode_prefixed_hex_literal(trimmed) {
                return Some(hex_bytes);
            }
            if let Some(b64_bytes) = decode_base64_str(trimmed)
                && detect_image_metadata(&b64_bytes).is_some()
            {
                return Some(b64_bytes);
            }
            None
        }
    }
}

fn decode_data_uri(input: &str) -> Option<Vec<u8>> {
    let rest = input.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    if meta.to_ascii_lowercase().contains(";base64") {
        return decode_base64_str(payload);
    }
    if meta.to_ascii_lowercase().contains("image/svg+xml") {
        return Some(decode_percent_encoded_utf8(payload));
    }
    None
}

fn decode_percent_encoded_utf8(input: &str) -> Vec<u8> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Pure-Rust RFC 4648 base64 decoder (standard and URL-safe alphabets, whitespace tolerant).
#[must_use]
pub fn decode_base64_str(input: &str) -> Option<Vec<u8>> {
    let filtered: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if filtered.len() < 4 {
        return None;
    }
    let mut out = Vec::with_capacity((filtered.len() / 4) * 3);
    let mut buf = [0u8; 4];
    let mut buf_len = 0usize;
    let mut pad_count = 0usize;

    for &b in &filtered {
        if b == b'=' {
            pad_count += 1;
            buf[buf_len] = 0;
            buf_len += 1;
        } else {
            if pad_count > 0 {
                return None;
            }
            buf[buf_len] = base64_sextet(b)?;
            buf_len += 1;
        }
        if buf_len == 4 {
            out.push((buf[0] << 2) | (buf[1] >> 4));
            if pad_count < 2 {
                out.push((buf[1] << 4) | (buf[2] >> 2));
            }
            if pad_count == 0 {
                out.push((buf[2] << 6) | buf[3]);
            }
            buf_len = 0;
            pad_count = 0;
        }
    }

    if buf_len == 1 {
        return None;
    }
    if buf_len == 2 {
        out.push((buf[0] << 2) | (buf[1] >> 4));
    } else if buf_len == 3 {
        out.push((buf[0] << 2) | (buf[1] >> 4));
        out.push((buf[1] << 4) | (buf[2] >> 2));
    }
    Some(out)
}

const fn base64_sextet(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    }
}

/// Detects PNG, JPEG, GIF, WEBP, BMP, or SVG magic bytes and extracts image dimensions.
#[must_use]
pub fn detect_image_metadata(bytes: &[u8]) -> Option<ImageMetadata> {
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        let (width, height) = parse_png_dimensions(bytes);
        return Some(ImageMetadata {
            format: DetectedImageFormat::Png,
            width,
            height,
            byte_len: bytes.len(),
        });
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        let (width, height) = parse_jpeg_dimensions(bytes);
        return Some(ImageMetadata {
            format: DetectedImageFormat::Jpeg,
            width,
            height,
            byte_len: bytes.len(),
        });
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        let (width, height) = parse_gif_dimensions(bytes);
        return Some(ImageMetadata {
            format: DetectedImageFormat::Gif,
            width,
            height,
            byte_len: bytes.len(),
        });
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        let (width, height) = parse_webp_dimensions(bytes);
        return Some(ImageMetadata {
            format: DetectedImageFormat::Webp,
            width,
            height,
            byte_len: bytes.len(),
        });
    }
    if is_valid_bmp_header(bytes) {
        let (width, height) = parse_bmp_dimensions(bytes);
        return Some(ImageMetadata {
            format: DetectedImageFormat::Bmp,
            width,
            height,
            byte_len: bytes.len(),
        });
    }
    if looks_like_svg(bytes) {
        let (width, height) = parse_svg_dimensions(bytes);
        return Some(ImageMetadata {
            format: DetectedImageFormat::Svg,
            width,
            height,
            byte_len: bytes.len(),
        });
    }
    None
}

fn parse_png_dimensions(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
        return (None, None);
    }
    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    (Some(w), Some(h))
}

fn parse_jpeg_dimensions(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    let mut i = 2usize;
    while i + 1 < bytes.len() {
        while i < bytes.len() && bytes[i] != 0xFF {
            i += 1;
        }
        while i < bytes.len() && bytes[i] == 0xFF {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let marker = bytes[i];
        i += 1;
        if marker == 0x00 || marker == 0x01 || marker == 0xD8 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if marker == 0xD9 || marker == 0xDA {
            break;
        }
        if i + 1 >= bytes.len() {
            break;
        }
        let seg_len = u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
        if seg_len < 2 || i + seg_len > bytes.len() {
            break;
        }
        let is_sof = matches!(
            marker,
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF
        );
        if is_sof && seg_len >= 7 {
            let h = u32::from(u16::from_be_bytes([bytes[i + 3], bytes[i + 4]]));
            let w = u32::from(u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]));
            return (Some(w), Some(h));
        }
        i += seg_len;
    }
    (None, None)
}

fn parse_gif_dimensions(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    if bytes.len() < 10 {
        return (None, None);
    }
    let w = u32::from(u16::from_le_bytes([bytes[6], bytes[7]]));
    let h = u32::from(u16::from_le_bytes([bytes[8], bytes[9]]));
    (Some(w), Some(h))
}

fn parse_webp_dimensions(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    if bytes.len() < 16 {
        return (None, None);
    }
    match &bytes[12..16] {
        b"VP8 " if bytes.len() >= 30 && bytes[23..26] == [0x9D, 0x01, 0x2A] => {
            let w = u32::from(u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3FFF);
            let h = u32::from(u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3FFF);
            (Some(w), Some(h))
        }
        b"VP8L" if bytes.len() >= 25 && bytes[20] == 0x2F => {
            let bits = u32::from_le_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]);
            let w = (bits & 0x3FFF) + 1;
            let h = ((bits >> 14) & 0x3FFF) + 1;
            (Some(w), Some(h))
        }
        b"VP8X" if bytes.len() >= 30 => {
            let w = 1
                + (u32::from(bytes[24])
                    | (u32::from(bytes[25]) << 8)
                    | (u32::from(bytes[26]) << 16));
            let h = 1
                + (u32::from(bytes[27])
                    | (u32::from(bytes[28]) << 8)
                    | (u32::from(bytes[29]) << 16));
            (Some(w), Some(h))
        }
        _ => (None, None),
    }
}

fn is_valid_bmp_header(bytes: &[u8]) -> bool {
    if bytes.len() < 26 || !bytes.starts_with(b"BM") {
        return false;
    }
    let dib_size = u32::from_le_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]);
    matches!(dib_size, 12 | 40 | 52 | 56 | 64 | 108 | 124)
}

fn parse_bmp_dimensions(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    if bytes.len() < 22 {
        return (None, None);
    }
    let dib_size = u32::from_le_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]);
    if dib_size == 12 {
        let w = u32::from(u16::from_le_bytes([bytes[18], bytes[19]]));
        let h = u32::from(u16::from_le_bytes([bytes[20], bytes[21]]));
        return (Some(w), Some(h));
    }
    if bytes.len() >= 26 {
        let w = i32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]).unsigned_abs();
        let h = i32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]).unsigned_abs();
        return (Some(w), Some(h));
    }
    (None, None)
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let trimmed = text.trim_start_matches('\u{FEFF}').trim_start();
    let lower = trimmed
        .chars()
        .take(512)
        .collect::<String>()
        .to_ascii_lowercase();
    lower.starts_with("<svg")
        || ((lower.starts_with("<?xml") || lower.starts_with("<!doctype svg"))
            && lower.contains("<svg"))
}

fn parse_svg_dimensions(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return (None, None);
    };
    let lower = text.to_ascii_lowercase();
    let Some(svg_start) = lower.find("<svg") else {
        return (None, None);
    };
    let tag_slice = &text[svg_start..];
    let tag_end = tag_slice.find('>').unwrap_or(tag_slice.len());
    let opening_tag = &tag_slice[..tag_end];

    let w = extract_xml_attr(opening_tag, "width").and_then(parse_svg_length_px);
    let h = extract_xml_attr(opening_tag, "height").and_then(parse_svg_length_px);
    if let (Some(width), Some(height)) = (w, h) {
        return (Some(width), Some(height));
    }

    if let Some(view_box) = extract_xml_attr(opening_tag, "viewbox") {
        let nums: Vec<f64> = view_box
            .split(|c: char| c.is_ascii_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<f64>().ok())
            .collect();
        if nums.len() == 4 && nums[2] > 0.0 && nums[3] > 0.0 {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "SVG viewBox dimensions are validated positive before rounding to u32 pixels"
            )]
            return (Some(nums[2].round() as u32), Some(nums[3].round() as u32));
        }
    }

    (w, h)
}

fn extract_xml_attr<'a>(tag: &'a str, attr_name_lower: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();
    let mut search_from = 0usize;
    while let Some(rel_idx) = lower[search_from..].find(attr_name_lower) {
        let idx = search_from + rel_idx;
        let preceded_ok = idx == 0 || !tag.as_bytes()[idx - 1].is_ascii_alphanumeric();
        let after_name = idx + attr_name_lower.len();
        if preceded_ok {
            let rest = tag[after_name..].trim_start();
            if let Some(after_eq) = rest.strip_prefix('=') {
                let val_part = after_eq.trim_start();
                if let Some(quote) = val_part.chars().next().filter(|c| *c == '"' || *c == '\'') {
                    let inner = &val_part[1..];
                    if let Some(end_q) = inner.find(quote) {
                        return Some(&inner[..end_q]);
                    }
                }
            }
        }
        search_from = after_name;
    }
    None
}

fn parse_svg_length_px(raw: &str) -> Option<u32> {
    let trimmed = raw.trim();
    if trimmed.ends_with('%') {
        return None;
    }
    let numeric = trimmed
        .strip_suffix("px")
        .or_else(|| trimmed.strip_suffix("PX"))
        .unwrap_or(trimmed)
        .trim();
    let val = numeric.parse::<f64>().ok()?;
    if val > 0.0 && val.is_finite() {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "SVG length is validated finite and positive before rounding to u32 pixels"
        )]
        Some(val.round() as u32)
    } else {
        None
    }
}

fn detect_binary_signature_label(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"%PDF-") {
        "PDF"
    } else if bytes.starts_with(b"PK\x03\x04") {
        "ZIP"
    } else if bytes.starts_with(&[0x1F, 0x8B]) {
        "GZIP"
    } else if bytes.starts_with(b"SQLite format 3\0") {
        "SQLite 3"
    } else if bytes.starts_with(b"MZ") {
        "PE / EXE"
    } else if bytes.starts_with(b"\x7FELF") {
        "ELF"
    } else {
        "BIN"
    }
}

fn format_byte_size(len: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    #[expect(
        clippy::cast_precision_loss,
        reason = "display formatting of byte lengths accepts f64 precision above 2^53"
    )]
    let f = len as f64;
    if len < 1024 {
        format!("{len} B")
    } else if len < 1024 * 1024 {
        format!("{:.1} KB", f / KB)
    } else {
        format!("{:.2} MB", f / MB)
    }
}

fn build_export_filename(col_name: &str, meta: Option<ImageMetadata>, bytes: &[u8]) -> String {
    let stem: String = col_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let clean_stem = if stem.is_empty() { "value" } else { &stem };
    let ext = meta.map_or_else(
        || {
            if bytes.starts_with(b"%PDF-") {
                "pdf"
            } else if bytes.starts_with(b"PK\x03\x04") {
                "zip"
            } else if bytes.starts_with(&[0x1F, 0x8B]) {
                "gz"
            } else {
                "bin"
            }
        },
        |m| m.format.extension(),
    );
    format!("{clean_stem}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tablepro_core::value::JsonText;

    #[test]
    fn json_tree_flattens_collapses_and_formats_paths() {
        let doc: serde_json::Value = serde_json::from_str(
            r#"{"user":{"first name":"Alice","age":30,"active":true,"bio":null},"tags":["sql","db"]}"#,
        )
        .unwrap_or(serde_json::Value::Null);

        let empty_collapsed = BTreeSet::new();
        let flat = flatten_json_tree(&doc, &empty_collapsed);
        let paths: Vec<&str> = flat.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "$",
                "$.user",
                r#"$.user["first name"]"#,
                "$.user.age",
                "$.user.active",
                "$.user.bio",
                "$.tags",
                "$.tags[0]",
                "$.tags[1]",
            ]
        );

        let mut collapsed = BTreeSet::new();
        collapsed.insert("$.user".to_owned());
        let flat_collapsed = flatten_json_tree(&doc, &collapsed);
        let collapsed_paths: Vec<&str> = flat_collapsed.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(
            collapsed_paths,
            vec!["$", "$.user", "$.tags", "$.tags[0]", "$.tags[1]"]
        );
    }

    #[test]
    fn json_tree_edits_leaf_value_and_validates_errors() {
        let mut doc: serde_json::Value =
            serde_json::from_str(r#"{"user":{"age":30},"tags":["a","b"]}"#)
                .unwrap_or(serde_json::Value::Null);

        let parsed = parse_json_node_edit("42").unwrap_or(serde_json::Value::Null);
        assert_eq!(parsed, serde_json::json!(42));
        assert!(
            set_json_value_at_path(
                &mut doc,
                &[
                    JsonPathSegment::Key("user".to_owned()),
                    JsonPathSegment::Key("age".to_owned()),
                ],
                parsed,
            )
            .is_ok()
        );
        assert_eq!(doc["user"]["age"], serde_json::json!(42));

        let str_edit = parse_json_node_edit(r#""rust""#).unwrap_or(serde_json::Value::Null);
        assert!(
            set_json_value_at_path(
                &mut doc,
                &[
                    JsonPathSegment::Key("tags".to_owned()),
                    JsonPathSegment::Index(1),
                ],
                str_edit,
            )
            .is_ok()
        );
        assert_eq!(doc["tags"][1], serde_json::json!("rust"));

        assert!(parse_json_node_edit("").is_err());
        assert!(parse_json_node_edit("{bad").is_err());
    }

    #[test]
    fn hex_dump_formats_8_and_16_byte_aligned_rows() {
        let bytes = b"Hello!\x00\xFFAB".to_vec();
        let rows_8 = format_hex_dump_rows(&bytes, 8, None);
        assert_eq!(rows_8.len(), 2);
        assert_eq!(rows_8[0].offset_hex, "00000000");
        assert_eq!(rows_8[0].hex_bytes, "48 65 6C 6C 6F 21 00 FF");
        assert_eq!(rows_8[0].ascii, "Hello!..".to_owned());
        assert_eq!(rows_8[1].offset_hex, "00000008");
        assert_eq!(rows_8[1].hex_bytes, "41 42                  ");
        assert_eq!(rows_8[1].ascii, "AB");

        let rows_16 = format_hex_dump_rows(&bytes, 16, None);
        assert_eq!(rows_16.len(), 1);
        assert_eq!(rows_16[0].offset_hex, "00000000");
        assert_eq!(
            rows_16[0].hex_bytes,
            "48 65 6C 6C 6F 21 00 FF  41 42                  "
        );
        assert_eq!(rows_16[0].ascii, "Hello!..AB");

        let from_pg_hex =
            extract_bytes_for_hex(&Value::Text("\\x89504E47".to_owned()), "\\x89504E47");
        assert_eq!(
            from_pg_hex,
            Some((
                vec![0x89, 0x50, 0x4E, 0x47],
                HexSourceKind::DecodedHexString
            ))
        );
    }

    #[test]
    fn image_magic_bytes_and_dimensions_parse_for_png_jpeg_gif_bmp_svg() {
        // PNG 320x180
        let mut png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&320u32.to_be_bytes());
        png.extend_from_slice(&180u32.to_be_bytes());
        assert_eq!(
            detect_image_metadata(&png),
            Some(ImageMetadata {
                format: DetectedImageFormat::Png,
                width: Some(320),
                height: Some(180),
                byte_len: 24,
            })
        );

        // JPEG SOF0 640x480
        let jpeg = vec![
            0xFF, 0xD8, // SOI
            0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, // APP0
            0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x01, 0xE0, 0x02, 0x80, 0x03, 0x01, 0x11,
            0x00, // SOF0
            0xFF, 0xD9, // EOI
        ];
        assert_eq!(
            detect_image_metadata(&jpeg),
            Some(ImageMetadata {
                format: DetectedImageFormat::Jpeg,
                width: Some(640),
                height: Some(480),
                byte_len: jpeg.len(),
            })
        );

        // GIF89a 128x96
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&128u16.to_le_bytes());
        gif.extend_from_slice(&96u16.to_le_bytes());
        assert_eq!(
            detect_image_metadata(&gif),
            Some(ImageMetadata {
                format: DetectedImageFormat::Gif,
                width: Some(128),
                height: Some(96),
                byte_len: 10,
            })
        );

        // BMP top-down (negative height) 200x100
        let mut bmp = vec![0u8; 26];
        bmp[0..2].copy_from_slice(b"BM");
        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&200i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&(-100i32).to_le_bytes());
        assert_eq!(
            detect_image_metadata(&bmp),
            Some(ImageMetadata {
                format: DetectedImageFormat::Bmp,
                width: Some(200),
                height: Some(100),
                byte_len: 26,
            })
        );

        // SVG with viewBox
        let svg_doc =
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 36"><rect/></svg>"#;
        assert_eq!(
            detect_image_metadata(svg_doc),
            Some(ImageMetadata {
                format: DetectedImageFormat::Svg,
                width: Some(24),
                height: Some(36),
                byte_len: svg_doc.len(),
            })
        );
    }

    #[test]
    fn base64_data_uri_and_recommended_mode_detection() {
        // 1x1 PNG encoded as base64 data URI
        let png_b64 = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
        let payload = extract_binary_or_image_payload(&Value::Text(png_b64.to_owned()), png_b64);
        assert!(payload.is_some());
        let meta = payload.as_deref().and_then(detect_image_metadata);
        assert_eq!(
            meta.map(|m| (m.format, m.width, m.height)),
            Some((DetectedImageFormat::Png, Some(1), Some(1)))
        );
        assert_eq!(
            detect_recommended_mode(&Value::Text(png_b64.to_owned()), png_b64),
            RichViewerMode::Image
        );

        let json_val = JsonText::parse(r#"{"k":1}"#.to_owned()).ok();
        if let Some(jt) = json_val {
            assert_eq!(
                detect_recommended_mode(&Value::Json(jt), r#"{"k":1}"#),
                RichViewerMode::JsonTree
            );
        }
        assert_eq!(
            detect_recommended_mode(&Value::Bytes(vec![1, 2, 3, 4]), "\\x01020304"),
            RichViewerMode::Hex
        );
        assert_eq!(
            detect_recommended_mode(&Value::Text("42".to_owned()), "42"),
            RichViewerMode::Text
        );
    }

    #[test]
    fn webp_svg_dimensions_hex_text_and_path_errors_behave_accurately() {
        // WEBP VP8X 400x250
        let mut webp = Vec::from(&b"RIFF\x00\x00\x00\x00WEBPVP8X"[..]);
        webp.extend_from_slice(&[0u8; 8]); // flags + reserved
        let w_m1: u32 = 399;
        let h_m1: u32 = 249;
        webp.push((w_m1 & 0xFF) as u8);
        webp.push(((w_m1 >> 8) & 0xFF) as u8);
        webp.push(((w_m1 >> 16) & 0xFF) as u8);
        webp.push((h_m1 & 0xFF) as u8);
        webp.push(((h_m1 >> 8) & 0xFF) as u8);
        webp.push(((h_m1 >> 16) & 0xFF) as u8);
        assert_eq!(
            detect_image_metadata(&webp),
            Some(ImageMetadata {
                format: DetectedImageFormat::Webp,
                width: Some(400),
                height: Some(250),
                byte_len: 30,
            })
        );

        // SVG with explicit pixel width/height and SVG data URI
        let svg_px =
            br#"<svg width="120px" height="80px" xmlns="http://www.w3.org/2000/svg"></svg>"#;
        assert_eq!(
            detect_image_metadata(svg_px),
            Some(ImageMetadata {
                format: DetectedImageFormat::Svg,
                width: Some(120),
                height: Some(80),
                byte_len: svg_px.len(),
            })
        );
        let svg_uri =
            "data:image/svg+xml;utf8,%3Csvg%20viewBox%3D%220%200%2016%2016%22%3E%3C/svg%3E";
        let svg_bytes = extract_binary_or_image_payload(&Value::Text(svg_uri.to_owned()), svg_uri)
            .unwrap_or_default();
        assert_eq!(
            detect_image_metadata(&svg_bytes).map(|m| (m.format, m.width, m.height)),
            Some((DetectedImageFormat::Svg, Some(16), Some(16)))
        );

        // 16-byte aligned plain-text hex dump formatting
        let dump = format_hex_dump_text(b"0123456789ABCDEF!", 16, None);
        let lines: Vec<&str> = dump.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with(
            "00000000  30 31 32 33 34 35 36 37  38 39 41 42 43 44 45 46  0123456789ABCDEF"
        ));
        assert!(lines[1].starts_with("00000010  21"));

        // JSON path error paths
        let mut doc: serde_json::Value =
            serde_json::from_str(r#"{"a":[1,2]}"#).unwrap_or(serde_json::Value::Null);
        assert!(
            set_json_value_at_path(
                &mut doc,
                &[JsonPathSegment::Key("missing".to_owned())],
                serde_json::json!(9),
            )
            .is_err()
        );
        assert!(
            set_json_value_at_path(
                &mut doc,
                &[
                    JsonPathSegment::Key("a".to_owned()),
                    JsonPathSegment::Index(5),
                ],
                serde_json::json!(9),
            )
            .is_err()
        );

        // Localized tab labels match i18n Strings
        let en = crate::i18n::Strings::for_locale(crate::i18n::Locale::EnUs);
        assert_eq!(RichViewerMode::Text.label(&en), en.inspector_tab_text);
        assert_eq!(RichViewerMode::JsonTree.label(&en), en.inspector_tab_json);
        assert_eq!(RichViewerMode::Hex.label(&en), en.inspector_tab_hex);
        assert_eq!(RichViewerMode::Image.label(&en), en.inspector_tab_image);
    }
}
