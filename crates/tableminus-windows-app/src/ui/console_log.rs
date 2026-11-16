//! TablePlus-inspired bottom SQL Console Log drawer displaying executed
//! queries, schema/PRAGMA inspections, counts, mutations, and SQL errors.

use std::time::{SystemTime, UNIX_EPOCH};

use herogpui::components::{Button, Icon, IconName, IconSize, Scrollbar, Tooltip, Variant};
use herogpui::core::Size as HeroSize;
use herogpui::gpui::{
    AnyElement, ClipboardItem, Context, Font, FontWeight, Hsla, InteractiveElement, IntoElement,
    ParentElement, ScrollHandle, StatefulInteractiveElement, Styled, Window, div, px,
};
use herogpui::theme::ActiveTheme;
use tablepro_core::sql_syntax::SqlGrammar;

use super::TableMinusWorkspace;
use super::shell_state::push_success_toast;
use super::syntax_bridge::styled_sql_block;
use super::theme::{
    HAIRLINE_BORDER_PX, MICRO_GAP_PX, TEXT_CONTROL_PX, TEXT_META_PX, editor_gpui_font,
    hairline_border_color,
};

const DEFAULT_MAX_LOG_ENTRIES: usize = 500;
const DRAWER_HEIGHT_PX: f32 = 210.0;

/// Category of an executed SQL statement recorded in [`ConsoleLogState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleLogKind {
    Query,
    Schema,
    Count,
    Mutation,
    Error,
}

impl ConsoleLogKind {
    /// Short uppercase label shown alongside the timestamp comment.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Query => "QUERY",
            Self::Schema => "SCHEMA",
            Self::Count => "COUNT",
            Self::Mutation => "MUTATION",
            Self::Error => "ERROR",
        }
    }
}

/// Single timestamped SQL statement in the Console Log drawer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleLogEntry {
    pub timestamp: String,
    pub sql: String,
    pub kind: ConsoleLogKind,
}

/// Workspace-scoped state for the bottom SQL Console Log drawer.
pub struct ConsoleLogState {
    pub visible: bool,
    pub entries: Vec<ConsoleLogEntry>,
    pub max_entries: usize,
    scroll_handle: ScrollHandle,
}

impl Default for ConsoleLogState {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleLogState {
    /// Creates a hidden console log state capped at 500 entries.
    #[must_use]
    pub fn new() -> Self {
        Self {
            visible: false,
            entries: Vec::new(),
            max_entries: DEFAULT_MAX_LOG_ENTRIES,
            scroll_handle: ScrollHandle::new(),
        }
    }

    /// Toggles visibility of the SQL Console Log drawer.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Appends a SQL statement with the current `YYYY-MM-DD HH:MM:SS.mmmm` timestamp.
    pub fn push(&mut self, sql: impl Into<String>, kind: ConsoleLogKind) {
        let sql = sql.into().trim().to_owned();
        if sql.is_empty() {
            return;
        }

        let entry = ConsoleLogEntry {
            timestamp: format_timestamp_now(),
            sql,
            kind,
        };
        self.entries.push(entry);

        if self.max_entries > 0 && self.entries.len() > self.max_entries {
            let excess = self.entries.len() - self.max_entries;
            self.entries.drain(..excess);
        }

        self.scroll_handle.scroll_to_bottom();
    }

    /// Removes all logged SQL entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Formats `SystemTime::now()` as `YYYY-MM-DD HH:MM:SS.mmmm` without panicking.
fn format_timestamp_now() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = i64::try_from(duration.as_secs()).unwrap_or(0);
    let subsec_tenths_ms = duration.subsec_micros() / 100;

    let days = total_secs.div_euclid(86_400);
    let secs_of_day = total_secs.rem_euclid(86_400);

    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3_600;
    let minute = (secs_of_day % 3_600) / 60;
    let second = secs_of_day % 60;

    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{subsec_tenths_ms:04}")
}

/// Howard Hinnant's civil-from-days conversion (days since 1970-01-01).
const fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m, d)
}

/// Renders the bottom SQL Console Log drawer.
#[must_use]
pub fn render_console_log_drawer(
    ws: &mut TableMinusWorkspace,
    _window: &mut Window,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let border_color = hairline_border_color(cx);
    let editor_font = editor_gpui_font(&ws.controller.settings);
    let grammar = ws.active_grammar();
    let entry_count = ws.console_log.entries.len();
    let scroll_handle = ws.console_log.scroll_handle.clone();

    let header = render_console_log_header(entry_count, border_color, cx);

    let mut scroll_list = div()
        .id("console-log-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&scroll_handle)
        .px(px(12.0))
        .py(px(8.0))
        .flex()
        .flex_col()
        .gap(px(8.0));

    if ws.console_log.entries.is_empty() {
        scroll_list = scroll_list.child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(TEXT_META_PX))
                .text_color(colors.muted)
                .child("No SQL statements logged yet. Run a query or open a table to inspect executed SQL."),
        );
    } else {
        for (idx, entry) in ws.console_log.entries.iter().enumerate() {
            scroll_list = scroll_list.child(render_console_log_entry(
                idx,
                entry,
                ws,
                grammar,
                editor_font.clone(),
                cx,
            ));
        }
    }

    let body = div()
        .relative()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .child(scroll_list)
        .child(Scrollbar::new("console-log-scrollbar", scroll_handle));

    div()
        .id("console-log-drawer")
        .h(px(DRAWER_HEIGHT_PX))
        .w_full()
        .min_w(px(0.0))
        .flex_shrink_0()
        .bg(ws.content_palette.editor_background)
        .border_t(px(HAIRLINE_BORDER_PX))
        .border_color(border_color)
        .flex()
        .flex_col()
        .child(header)
        .child(body)
        .into_any_element()
}

fn render_console_log_header(
    entry_count: usize,
    border_color: Hsla,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors().clone();
    let count_label = if entry_count == 1 {
        "1 entry".to_owned()
    } else {
        format!("{entry_count} entries")
    };

    div()
        .id("console-log-header")
        .h(px(30.0))
        .w_full()
        .flex_shrink_0()
        .px(px(10.0))
        .border_b(px(HAIRLINE_BORDER_PX))
        .border_color(border_color)
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    Icon::new(IconName::Terminal)
                        .size(IconSize::Xs)
                        .color(colors.muted),
                )
                .child(
                    div()
                        .text_size(px(TEXT_META_PX))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child("SQL Console"),
                )
                .child(
                    div()
                        .text_size(px(TEXT_META_PX))
                        .text_color(colors.muted)
                        .child(format!("({count_label})")),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(MICRO_GAP_PX))
                .child(
                    Tooltip::new("Clear SQL Console").child(
                        Button::new("console-log-clear")
                            .variant(Variant::Ghost)
                            .size(HeroSize::Sm)
                            .is_icon_only(true)
                            .is_disabled(entry_count == 0)
                            .aria_label("Clear SQL Console")
                            .on_press(cx.listener(|ws, _, _window, cx| {
                                ws.console_log.clear();
                                cx.notify();
                            }))
                            .child(Icon::new(IconName::Trash2).size(IconSize::Xs)),
                    ),
                )
                .child(
                    Tooltip::new("Close SQL Console").child(
                        Button::new("console-log-close")
                            .variant(Variant::Ghost)
                            .size(HeroSize::Sm)
                            .is_icon_only(true)
                            .aria_label("Close SQL Console")
                            .on_press(cx.listener(|ws, _, _window, cx| {
                                ws.console_log.visible = false;
                                cx.notify();
                            }))
                            .child(Icon::new(IconName::X).size(IconSize::Xs)),
                    ),
                ),
        )
        .into_any_element()
}

fn render_console_log_entry(
    idx: usize,
    entry: &ConsoleLogEntry,
    ws: &TableMinusWorkspace,
    grammar: SqlGrammar,
    editor_font: Font,
    cx: &mut Context<TableMinusWorkspace>,
) -> AnyElement {
    let colors = cx.colors();
    let sql_for_copy = entry.sql.clone();
    let styled_sql = styled_sql_block(&entry.sql, grammar, &ws.content_palette);
    let comment_color = ws.content_palette.editor_syntax_comment;
    let kind_color = if entry.kind == ConsoleLogKind::Error {
        colors.danger.color
    } else {
        colors.muted
    };

    div()
        .id(("console-log-entry", idx))
        .w_full()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .font(editor_font)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .text_size(px(TEXT_META_PX))
                        .child(
                            div()
                                .text_color(comment_color)
                                .child(format!("-- {}", entry.timestamp)),
                        )
                        .child(
                            div()
                                .text_color(kind_color)
                                .child(format!("[{}]", entry.kind.label())),
                        ),
                )
                .child(
                    Tooltip::new("Copy SQL").child(
                        Button::new(("console-log-copy", idx))
                            .variant(Variant::Ghost)
                            .size(HeroSize::Sm)
                            .is_icon_only(true)
                            .aria_label("Copy SQL")
                            .on_press(cx.listener(move |_ws, _, _window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    sql_for_copy.clone(),
                                ));
                                push_success_toast("Copied SQL to clipboard", None, cx);
                            }))
                            .child(Icon::new(IconName::Copy).size(IconSize::Xs)),
                    ),
                ),
        )
        .child(
            div()
                .w_full()
                .text_size(px(TEXT_CONTROL_PX))
                .text_color(ws.content_palette.editor_text)
                .child(styled_sql),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_and_default_start_hidden_and_empty() {
        let state = ConsoleLogState::new();
        assert!(!state.visible);
        assert!(state.entries.is_empty());
        assert_eq!(state.max_entries, DEFAULT_MAX_LOG_ENTRIES);

        let defaulted = ConsoleLogState::default();
        assert!(!defaulted.visible);
        assert!(defaulted.entries.is_empty());
        assert_eq!(defaulted.max_entries, DEFAULT_MAX_LOG_ENTRIES);
    }

    #[test]
    fn kind_labels_match_expected_uppercase_tags() {
        assert_eq!(ConsoleLogKind::Query.label(), "QUERY");
        assert_eq!(ConsoleLogKind::Schema.label(), "SCHEMA");
        assert_eq!(ConsoleLogKind::Count.label(), "COUNT");
        assert_eq!(ConsoleLogKind::Mutation.label(), "MUTATION");
        assert_eq!(ConsoleLogKind::Error.label(), "ERROR");
    }

    #[test]
    fn toggle_flips_visibility_and_clear_preserves_visibility_and_cap() {
        let mut state = ConsoleLogState::new();
        state.max_entries = 42;
        assert!(!state.visible);

        state.toggle();
        assert!(state.visible);
        state.push("SELECT 1;", ConsoleLogKind::Query);
        assert_eq!(state.entries.len(), 1);

        state.clear();
        assert!(state.entries.is_empty());
        assert!(state.visible);
        assert_eq!(state.max_entries, 42);

        state.toggle();
        assert!(!state.visible);
    }

    #[test]
    fn push_trims_sql_ignores_blank_and_preserves_kind() {
        let mut state = ConsoleLogState::new();
        state.push("", ConsoleLogKind::Query);
        state.push("   \n\t  ", ConsoleLogKind::Error);
        assert!(state.entries.is_empty());

        state.push("  SELECT * FROM \"users\"; \n", ConsoleLogKind::Query);
        state.push(
            "INSERT INTO \"users\" VALUES (1);",
            ConsoleLogKind::Mutation,
        );
        state.push("syntax error near FROM", ConsoleLogKind::Error);

        assert_eq!(state.entries.len(), 3);
        assert_eq!(state.entries[0].sql, "SELECT * FROM \"users\";");
        assert_eq!(state.entries[0].kind, ConsoleLogKind::Query);
        assert_eq!(state.entries[1].sql, "INSERT INTO \"users\" VALUES (1);");
        assert_eq!(state.entries[1].kind, ConsoleLogKind::Mutation);
        assert_eq!(state.entries[2].sql, "syntax error near FROM");
        assert_eq!(state.entries[2].kind, ConsoleLogKind::Error);
    }

    #[test]
    fn push_evicts_oldest_entries_when_exceeding_max_entries() {
        let mut state = ConsoleLogState::new();
        state.max_entries = 3;

        for i in 1..=5 {
            state.push(format!("SELECT {i};"), ConsoleLogKind::Query);
        }

        assert_eq!(state.entries.len(), 3);
        assert_eq!(state.entries[0].sql, "SELECT 3;");
        assert_eq!(state.entries[1].sql, "SELECT 4;");
        assert_eq!(state.entries[2].sql, "SELECT 5;");
    }

    #[test]
    fn push_with_single_entry_cap_keeps_only_latest() {
        let mut state = ConsoleLogState::new();
        state.max_entries = 1;
        state.push("SELECT 1;", ConsoleLogKind::Query);
        state.push("SELECT 2;", ConsoleLogKind::Count);

        assert_eq!(state.entries.len(), 1);
        assert_eq!(state.entries[0].sql, "SELECT 2;");
        assert_eq!(state.entries[0].kind, ConsoleLogKind::Count);
    }

    #[test]
    fn timestamp_matches_yyyy_mm_dd_hh_mm_ss_mmmm_format() {
        let ts = format_timestamp_now();
        assert_eq!(ts.len(), 24);
        let bytes = ts.as_bytes();
        for &idx in &[
            0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 20, 21, 22, 23,
        ] {
            assert!(
                bytes[idx].is_ascii_digit(),
                "expected digit at {idx} in {ts}"
            );
        }
        assert_eq!(bytes[4], b'-');
        assert_eq!(bytes[7], b'-');
        assert_eq!(bytes[10], b' ');
        assert_eq!(bytes[13], b':');
        assert_eq!(bytes[16], b':');
        assert_eq!(bytes[19], b'.');

        let month: u32 = ts[5..7].parse().expect("valid month");
        let day: u32 = ts[8..10].parse().expect("valid day");
        let hour: u32 = ts[11..13].parse().expect("valid hour");
        let minute: u32 = ts[14..16].parse().expect("valid minute");
        let second: u32 = ts[17..19].parse().expect("valid second");
        assert!((1..=12).contains(&month));
        assert!((1..=31).contains(&day));
        assert!(hour < 24);
        assert!(minute < 60);
        assert!(second < 60);
    }

    #[test]
    fn civil_from_days_handles_epoch_and_leap_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(20_773), (2026, 11, 16));
    }
}
