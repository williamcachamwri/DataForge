//! Shared SQL syntax highlighting adapter bridging `dataforge-state` / `tablepro-core`
//! into GPUI [`HighlightStyle`], [`TextRun`], and [`StyledText`] spans.

use std::ops::Range;

use dataforge_gfx::glyphs::EngineKind;
use dataforge_state::editor::{IncrementalLineHighlighter, Rope, Utf16SyntaxSpan, lex_line_utf16};
use herogpui::gpui::{
    Font, FontStyle, FontWeight, HighlightStyle, Hsla, SharedString, StyledText, TextRun,
};
use tablepro_core::sql_syntax::{SqlGrammar, SyntaxKind};

use super::theme::ContentThemePalette;

/// Maps DataForge's [`EngineKind`] to `tablepro-core`'s [`SqlGrammar`].
#[must_use]
pub const fn grammar_for_engine(engine: EngineKind) -> SqlGrammar {
    match engine {
        EngineKind::Postgresql => SqlGrammar::PostgreSql,
        EngineKind::Mysql | EngineKind::Mariadb => SqlGrammar::MySql,
        EngineKind::Sqlite => SqlGrammar::Sqlite,
        EngineKind::Duckdb => SqlGrammar::DuckDb,
        EngineKind::Mssql => SqlGrammar::MsSql,
        EngineKind::Clickhouse => SqlGrammar::ClickHouse,
        EngineKind::Cassandra | EngineKind::Scylladb => SqlGrammar::Cql,
        EngineKind::Mongodb => SqlGrammar::Mongo,
        EngineKind::Redis => SqlGrammar::Redis,
    }
}

/// Maps a [`SyntaxKind`] to a GPUI [`HighlightStyle`] backed by [`ContentThemePalette`].
#[must_use]
pub fn highlight_style_for_kind(kind: SyntaxKind, palette: &ContentThemePalette) -> HighlightStyle {
    match kind {
        SyntaxKind::Keyword => HighlightStyle {
            color: Some(palette.editor_syntax_keyword),
            font_weight: Some(FontWeight::SEMIBOLD),
            ..HighlightStyle::default()
        },
        SyntaxKind::String => HighlightStyle {
            color: Some(palette.editor_syntax_string),
            ..HighlightStyle::default()
        },
        SyntaxKind::Comment => HighlightStyle {
            color: Some(palette.editor_syntax_comment),
            font_style: Some(FontStyle::Italic),
            ..HighlightStyle::default()
        },
        SyntaxKind::Number => HighlightStyle {
            color: Some(palette.editor_syntax_number),
            ..HighlightStyle::default()
        },
        SyntaxKind::Operator => HighlightStyle {
            color: Some(palette.editor_syntax_operator),
            ..HighlightStyle::default()
        },
        SyntaxKind::Punctuation => HighlightStyle {
            color: Some(palette.editor_syntax_punctuation),
            ..HighlightStyle::default()
        },
        SyntaxKind::Identifier => HighlightStyle {
            color: Some(palette.editor_syntax_identifier),
            ..HighlightStyle::default()
        },
        SyntaxKind::QuotedIdentifier => HighlightStyle {
            color: Some(palette.editor_syntax_quoted_identifier),
            ..HighlightStyle::default()
        },
    }
}

/// Converts UTF-16 syntax spans for a single `line` into sorted, non-overlapping UTF-8
/// byte highlight ranges valid for GPUI [`StyledText`] and [`TextRun`] slicing.
#[must_use]
pub fn utf16_spans_to_byte_highlights(
    line: &str,
    spans: &[Utf16SyntaxSpan],
    palette: &ContentThemePalette,
) -> Vec<(Range<usize>, HighlightStyle)> {
    if line.is_empty() || spans.is_empty() {
        return Vec::new();
    }

    let total_utf16: usize = line.chars().map(char::len_utf16).sum();
    let mut utf16_to_byte = vec![line.len(); total_utf16 + 1];
    let mut utf16_cursor = 0usize;
    for (byte_idx, ch) in line.char_indices() {
        let units = ch.len_utf16();
        for offset in 0..units {
            if let Some(slot) = utf16_to_byte.get_mut(utf16_cursor + offset) {
                *slot = byte_idx;
            }
        }
        utf16_cursor += units;
    }

    let mut out = Vec::with_capacity(spans.len());
    let mut prev_end = 0usize;
    for span in spans {
        let start_u16 = span.start_utf16.min(total_utf16);
        let end_u16 = span.end_utf16.min(total_utf16);
        if end_u16 <= start_u16 {
            continue;
        }
        let start = utf16_to_byte[start_u16].max(prev_end);
        let end = utf16_to_byte[end_u16];
        if end <= start || !line.is_char_boundary(start) || !line.is_char_boundary(end) {
            continue;
        }
        out.push((start..end, highlight_style_for_kind(span.kind, palette)));
        prev_end = end;
    }
    out
}

/// Highlights `line_idx` inside `rope` via `highlighter` into GPUI UTF-8 byte ranges.
#[must_use]
pub fn highlight_sql_line(
    highlighter: &mut IncrementalLineHighlighter,
    rope: &Rope,
    line_idx: usize,
    palette: &ContentThemePalette,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let Some(line_text) = rope.line(line_idx) else {
        return Vec::new();
    };
    let spans = highlighter.highlight_line(rope, line_idx);
    utf16_spans_to_byte_highlights(&line_text, &spans, palette)
}

/// Highlights an arbitrary multi-line SQL string into cumulative UTF-8 byte ranges.
///
/// Suitable for AI drawer SQL code blocks, command palette previews, and history items.
#[must_use]
pub fn highlight_sql_text(
    text: &str,
    grammar: SqlGrammar,
    palette: &ContentThemePalette,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut out = Vec::new();
    let mut line_byte_offset = 0usize;
    for segment in text.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        let line = line.strip_suffix('\r').unwrap_or(line);
        let spans = lex_line_utf16(line, grammar);
        for (range, style) in utf16_spans_to_byte_highlights(line, &spans, palette) {
            out.push((
                (line_byte_offset + range.start)..(line_byte_offset + range.end),
                style,
            ));
        }
        line_byte_offset += segment.len();
    }
    out
}

/// Builds contiguous [`TextRun`] segments covering `0..line.len()` from byte highlights.
#[must_use]
pub fn build_line_text_runs(
    line: &str,
    base_font: &Font,
    highlights: &[(Range<usize>, HighlightStyle)],
    fallback_color: Hsla,
) -> Vec<TextRun> {
    if line.is_empty() {
        return Vec::new();
    }

    let mut runs = Vec::with_capacity(highlights.len().saturating_mul(2) + 1);
    let mut cursor = 0usize;
    for (range, style) in highlights {
        let start = range.start.min(line.len());
        let end = range.end.min(line.len());
        if start > cursor {
            runs.push(TextRun {
                len: start - cursor,
                font: base_font.clone(),
                color: fallback_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        }
        if end > start {
            let mut token_font = base_font.clone();
            if let Some(weight) = style.font_weight {
                token_font.weight = weight;
            }
            if let Some(font_style) = style.font_style {
                token_font.style = font_style;
            }
            runs.push(TextRun {
                len: end - start,
                font: token_font,
                color: style.color.unwrap_or(fallback_color),
                background_color: style.background_color,
                underline: style.underline,
                strikethrough: style.strikethrough,
            });
            cursor = end;
        }
    }
    if cursor < line.len() {
        runs.push(TextRun {
            len: line.len() - cursor,
            font: base_font.clone(),
            color: fallback_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        });
    }
    runs
}

/// Builds a syntax-highlighted GPUI [`StyledText`] element for a SQL snippet.
#[must_use]
pub fn styled_sql_block(
    sql: impl Into<SharedString>,
    grammar: SqlGrammar,
    palette: &ContentThemePalette,
) -> StyledText {
    let shared: SharedString = sql.into();
    let highlights = highlight_sql_text(shared.as_ref(), grammar, palette);
    StyledText::new(shared).with_highlights(highlights)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dataforge_state::theme::ThemeDefinition;
    use herogpui::gpui::font;

    #[test]
    fn grammar_for_engine_covers_all_eleven_engines() {
        assert_eq!(EngineKind::ALL.len(), 11);
        for engine in EngineKind::ALL {
            let _ = grammar_for_engine(engine);
        }
    }

    #[test]
    fn highlight_sql_text_respects_unicode_char_boundaries_and_order() {
        let palette = ContentThemePalette::from_definition(&ThemeDefinition::builtin_dark());
        let sql = "SELECT 'héllo 🚀', 42 FROM \"users\" -- comment\nWHERE id = 1;";
        let highlights = highlight_sql_text(sql, SqlGrammar::PostgreSql, &palette);
        assert!(!highlights.is_empty());

        let mut prev_end = 0usize;
        for (range, _) in &highlights {
            assert!(range.start >= prev_end, "overlapping highlights: {range:?}");
            assert!(range.end > range.start);
            assert!(sql.is_char_boundary(range.start));
            assert!(sql.is_char_boundary(range.end));
            prev_end = range.end;
        }
    }

    #[test]
    fn build_line_text_runs_covers_exact_utf8_byte_length() {
        let palette = ContentThemePalette::from_definition(&ThemeDefinition::builtin_light());
        let line = "SELECT 'café' AS label, 100";
        let spans = lex_line_utf16(line, SqlGrammar::PostgreSql);
        let highlights = utf16_spans_to_byte_highlights(line, &spans, &palette);
        let runs = build_line_text_runs(
            line,
            &font("Cascadia Mono"),
            &highlights,
            palette.editor_text,
        );
        let total_len: usize = runs.iter().map(|run| run.len).sum();
        assert_eq!(total_len, line.len());
    }
}
