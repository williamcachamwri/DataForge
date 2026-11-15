//! Dialect-agnostic SQL formatter and editor find/replace helpers (Feature 3).

use std::ops::Range;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
enum TokenKind {
    Word(String),
    Literal(String),
    LineComment(String),
    BlockComment(String),
    Comma,
    Semicolon,
    OpenParen,
    CloseParen,
    Dot,
    Operator(String),
}

/// Formats SQL with uppercase keyword normalization and clean clause indentation,
/// while preserving string literals, quoted identifiers, dollar-quoted strings, and comments verbatim.
#[must_use]
pub fn format_sql(sql: &str) -> String {
    let raw = tokenize_sql(sql);
    if raw.is_empty() {
        return String::new();
    }
    let merged = merge_compound_keywords(raw);
    render_tokens(&merged)
}

/// Options for editor find/replace operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FindReplaceOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
}

/// Returns byte ranges of all non-overlapping matches of `query` in `text`.
#[must_use]
pub fn find_matches(text: &str, query: &str, options: FindReplaceOptions) -> Vec<Range<usize>> {
    if query.is_empty() || text.is_empty() {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    if options.case_sensitive {
        let mut offset = 0_usize;
        while let Some(pos) = text[offset..].find(query) {
            let start = offset + pos;
            let end = start + query.len();
            if !options.whole_word || is_whole_word(text, start, end) {
                ranges.push(start..end);
                offset = end;
            } else {
                offset = start + query.chars().next().map_or(1, char::len_utf8);
            }
        }
    } else {
        let hay_lower = text.to_lowercase();
        let needle_lower = query.to_lowercase();
        if needle_lower.is_empty() {
            return Vec::new();
        }
        let mut offset = 0_usize;
        while let Some(pos) = hay_lower[offset..].find(&needle_lower) {
            let start = offset + pos;
            let end = start + needle_lower.len();
            if text.is_char_boundary(start)
                && text.is_char_boundary(end)
                && (!options.whole_word || is_whole_word(text, start, end))
            {
                ranges.push(start..end);
                offset = end;
            } else {
                offset = start + 1;
                while offset < hay_lower.len() && !hay_lower.is_char_boundary(offset) {
                    offset += 1;
                }
            }
        }
    }
    ranges
}

/// Replaces all matches of `query` in `text` with `replacement`, returning `(updated_text, count)`.
#[must_use]
pub fn replace_all_matches(
    text: &str,
    query: &str,
    replacement: &str,
    options: FindReplaceOptions,
) -> (String, usize) {
    let matches = find_matches(text, query, options);
    if matches.is_empty() {
        return (text.to_owned(), 0);
    }
    let count = matches.len();
    let mut out = String::with_capacity(text.len());
    let mut last = 0_usize;
    for range in matches {
        out.push_str(&text[last..range.start]);
        out.push_str(replacement);
        last = range.end;
    }
    out.push_str(&text[last..]);
    (out, count)
}

fn is_whole_word(text: &str, start: usize, end: usize) -> bool {
    let prev_word = text[..start].chars().next_back().is_some_and(is_word_char);
    let next_word = text[end..].chars().next().is_some_and(is_word_char);
    !prev_word && !next_word
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn tokenize_sql(sql: &str) -> Vec<TokenKind> {
    let mut tokens = Vec::new();
    let mut i = 0_usize;
    let bytes = sql.as_bytes();
    while i < bytes.len() {
        let rest = &sql[i..];
        let ch = rest.chars().next().unwrap_or('\0');
        if ch.is_whitespace() {
            i += ch.len_utf8();
            continue;
        }
        if rest.starts_with("--") {
            let end = rest.find('\n').map_or(sql.len(), |n| i + n);
            tokens.push(TokenKind::LineComment(sql[i..end].to_owned()));
            i = end;
            continue;
        }
        if rest.starts_with("/*") {
            let end = scan_block_comment(sql, i);
            tokens.push(TokenKind::BlockComment(sql[i..end].to_owned()));
            i = end;
            continue;
        }
        if ch == '\'' || ch == '"' || ch == '`' {
            let end = scan_quoted(sql, i, ch);
            tokens.push(TokenKind::Literal(sql[i..end].to_owned()));
            i = end;
            continue;
        }
        if ch == '[' {
            let end = scan_bracketed(sql, i);
            tokens.push(TokenKind::Literal(sql[i..end].to_owned()));
            i = end;
            continue;
        }
        if ch == '$'
            && let Some(end) = scan_dollar_quote(sql, i)
        {
            tokens.push(TokenKind::Literal(sql[i..end].to_owned()));
            i = end;
            continue;
        }
        let (tok, next_i) = scan_unquoted_token(sql, i, ch);
        tokens.push(tok);
        i = next_i;
    }
    tokens
}

fn scan_block_comment(sql: &str, start: usize) -> usize {
    let mut i = start + 2;
    let mut depth = 1_usize;
    while i < sql.len() && depth > 0 {
        if sql[i..].starts_with("/*") {
            depth += 1;
            i += 2;
        } else if sql[i..].starts_with("*/") {
            depth -= 1;
            i += 2;
        } else {
            i += sql[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    i
}

fn scan_quoted(sql: &str, start: usize, quote: char) -> usize {
    let mut i = start + quote.len_utf8();
    let mut escaped = false;
    while i < sql.len() {
        let ch = sql[i..].chars().next().unwrap_or('\0');
        let len = ch.len_utf8();
        if quote == '\'' && escaped {
            escaped = false;
            i += len;
            continue;
        }
        if quote == '\'' && ch == '\\' {
            escaped = true;
            i += len;
            continue;
        }
        if ch == quote {
            i += len;
            if sql[i..].starts_with(quote) {
                i += len;
                continue;
            }
            return i;
        }
        i += len;
    }
    sql.len()
}

fn scan_bracketed(sql: &str, start: usize) -> usize {
    let mut i = start + 1;
    while i < sql.len() {
        let ch = sql[i..].chars().next().unwrap_or('\0');
        i += ch.len_utf8();
        if ch == ']' {
            if sql[i..].starts_with(']') {
                i += 1;
                continue;
            }
            return i;
        }
    }
    sql.len()
}

fn scan_dollar_quote(sql: &str, start: usize) -> Option<usize> {
    let rest = &sql[start..];
    let bytes = rest.as_bytes();
    let tag_end = if bytes.get(1).copied() == Some(b'$') {
        2
    } else {
        let first = bytes.get(1).copied()?;
        if !(first.is_ascii_alphabetic() || first == b'_') {
            return None;
        }
        let mut idx = 2_usize;
        loop {
            let b = bytes.get(idx).copied()?;
            if b == b'$' {
                break idx + 1;
            }
            if !(b.is_ascii_alphanumeric() || b == b'_') {
                return None;
            }
            idx += 1;
        }
    };
    let tag = &rest[..tag_end];
    if let Some(close) = rest[tag_end..].find(tag) {
        Some(start + tag_end + close + tag.len())
    } else {
        Some(sql.len())
    }
}

fn scan_unquoted_token(sql: &str, i: usize, ch: char) -> (TokenKind, usize) {
    match ch {
        ',' => return (TokenKind::Comma, i + 1),
        ';' => return (TokenKind::Semicolon, i + 1),
        '(' => return (TokenKind::OpenParen, i + 1),
        ')' => return (TokenKind::CloseParen, i + 1),
        '.' => return (TokenKind::Dot, i + 1),
        _ => {}
    }
    if is_word_char(ch) {
        let mut end = i + ch.len_utf8();
        while end < sql.len() {
            let next = sql[end..].chars().next().unwrap_or('\0');
            if is_word_char(next) {
                end += next.len_utf8();
            } else {
                break;
            }
        }
        let word = &sql[i..end];
        let normalized = if is_sql_keyword(word) {
            word.to_ascii_uppercase()
        } else {
            word.to_owned()
        };
        return (TokenKind::Word(normalized), end);
    }
    let rest = &sql[i..];
    for op in ["<>", "!=", "<=", ">=", "||", "::", "->>", "->"] {
        if rest.starts_with(op) {
            return (TokenKind::Operator(op.to_owned()), i + op.len());
        }
    }
    let len = ch.len_utf8();
    (TokenKind::Operator(sql[i..i + len].to_owned()), i + len)
}

fn merge_compound_keywords(tokens: Vec<TokenKind>) -> Vec<TokenKind> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0_usize;
    while i < tokens.len() {
        if let Some((merged, consumed)) = try_merge_compound(&tokens[i..]) {
            out.push(TokenKind::Word(merged.to_owned()));
            i += consumed;
        } else {
            out.push(tokens[i].clone());
            i += 1;
        }
    }
    out
}

fn try_merge_compound(slice: &[TokenKind]) -> Option<(&'static str, usize)> {
    let w0 = word_upper(slice.first()?)?;
    let w1 = word_upper(slice.get(1)?)?;
    if let Some(w2) = slice.get(2).and_then(word_upper) {
        let three = match (w0, w1, w2) {
            ("LEFT", "OUTER", "JOIN") => Some("LEFT OUTER JOIN"),
            ("RIGHT", "OUTER", "JOIN") => Some("RIGHT OUTER JOIN"),
            ("FULL", "OUTER", "JOIN") => Some("FULL OUTER JOIN"),
            ("CREATE", "UNIQUE", "INDEX") => Some("CREATE UNIQUE INDEX"),
            ("IF", "NOT", "EXISTS") => Some("IF NOT EXISTS"),
            ("IS", "NOT", "NULL") => Some("IS NOT NULL"),
            _ => None,
        };
        if let Some(m) = three {
            return Some((m, 3));
        }
    }
    let two = match (w0, w1) {
        ("INNER", "JOIN") => Some("INNER JOIN"),
        ("LEFT", "JOIN") => Some("LEFT JOIN"),
        ("RIGHT", "JOIN") => Some("RIGHT JOIN"),
        ("FULL", "JOIN") => Some("FULL JOIN"),
        ("CROSS", "JOIN") => Some("CROSS JOIN"),
        ("GROUP", "BY") => Some("GROUP BY"),
        ("ORDER", "BY") => Some("ORDER BY"),
        ("UNION", "ALL") => Some("UNION ALL"),
        ("INSERT", "INTO") => Some("INSERT INTO"),
        ("DELETE", "FROM") => Some("DELETE FROM"),
        ("CREATE", "TABLE") => Some("CREATE TABLE"),
        ("ALTER", "TABLE") => Some("ALTER TABLE"),
        ("DROP", "TABLE") => Some("DROP TABLE"),
        ("CREATE", "INDEX") => Some("CREATE INDEX"),
        ("PRIMARY", "KEY") => Some("PRIMARY KEY"),
        ("FOREIGN", "KEY") => Some("FOREIGN KEY"),
        ("NOT", "NULL") => Some("NOT NULL"),
        ("IF", "EXISTS") => Some("IF EXISTS"),
        ("IS", "NULL") => Some("IS NULL"),
        _ => None,
    };
    two.map(|m| (m, 2))
}

fn word_upper(tok: &TokenKind) -> Option<&str> {
    match tok {
        TokenKind::Word(w) if is_sql_keyword(w) => Some(w.as_str()),
        _ => None,
    }
}

struct FormatWriter {
    out: String,
    depth: usize,
    in_select_or_set: bool,
    at_line_start: bool,
    prev: Option<TokenKind>,
}

impl FormatWriter {
    fn new() -> Self {
        Self {
            out: String::new(),
            depth: 0,
            in_select_or_set: false,
            at_line_start: true,
            prev: None,
        }
    }

    fn newline(&mut self, extra_indent: usize) {
        while self.out.ends_with(' ') {
            self.out.pop();
        }
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
        let spaces = (self.depth + extra_indent) * 2;
        for _ in 0..spaces {
            self.out.push(' ');
        }
        self.at_line_start = true;
    }

    fn push_space_if_needed(&mut self, next: &TokenKind) {
        if self.at_line_start {
            return;
        }
        let needs_space = match (&self.prev, next) {
            (None, _)
            | (Some(TokenKind::OpenParen | TokenKind::Dot), _)
            | (
                _,
                TokenKind::Comma | TokenKind::Semicolon | TokenKind::CloseParen | TokenKind::Dot,
            ) => false,
            (Some(TokenKind::Word(w)), TokenKind::OpenParen) if !is_clause_or_op_keyword(w) => {
                false
            }
            (Some(TokenKind::Operator(op)), _) if op == "::" => false,
            (_, TokenKind::Operator(op)) if op == "::" => false,
            _ => true,
        };
        if needs_space {
            self.out.push(' ');
        }
    }

    fn write_token(&mut self, tok: &TokenKind, next: Option<&TokenKind>) {
        match tok {
            TokenKind::Semicolon => {
                while self.out.ends_with(' ') {
                    self.out.pop();
                }
                self.out.push(';');
                self.in_select_or_set = false;
                if next.is_some() {
                    self.out.push('\n');
                    self.newline(0);
                }
                self.prev = Some(tok.clone());
                return;
            }
            TokenKind::LineComment(c) => {
                if !self.at_line_start {
                    self.out.push(' ');
                }
                self.out.push_str(c);
                if next.is_some() {
                    self.newline(0);
                }
                self.prev = Some(tok.clone());
                return;
            }
            TokenKind::BlockComment(c) => {
                self.push_space_if_needed(tok);
                self.out.push_str(c);
                self.at_line_start = false;
                self.prev = Some(tok.clone());
                return;
            }
            TokenKind::OpenParen => {
                self.push_space_if_needed(tok);
                self.out.push('(');
                self.depth += 1;
                self.at_line_start = false;
                if matches!(next, Some(TokenKind::Word(w)) if w == "SELECT" || w == "WITH") {
                    self.newline(0);
                }
            }
            TokenKind::CloseParen => {
                self.depth = self.depth.saturating_sub(1);
                self.push_space_if_needed(tok);
                self.out.push(')');
                self.at_line_start = false;
            }
            TokenKind::Comma => {
                self.out.push(',');
                self.at_line_start = false;
                if self.in_select_or_set && self.depth == 0 {
                    self.newline(1);
                }
            }
            TokenKind::Word(w) => self.write_word(w),
            TokenKind::Literal(s) | TokenKind::Operator(s) => {
                self.push_space_if_needed(tok);
                self.out.push_str(s);
                self.at_line_start = false;
            }
            TokenKind::Dot => {
                self.out.push('.');
                self.at_line_start = false;
            }
        }
        self.prev = Some(tok.clone());
    }

    fn write_word(&mut self, w: &str) {
        if is_top_clause_keyword(w) {
            if !self.out.is_empty() && !self.at_line_start {
                self.newline(0);
            }
            self.in_select_or_set = matches!(w, "SELECT" | "SET");
            self.out.push_str(w);
            self.at_line_start = false;
            if w == "SELECT" {
                self.newline(1);
            }
            return;
        }
        if matches!(w, "AND" | "OR") && self.depth == 0 && !self.out.is_empty() {
            self.newline(1);
            self.out.push_str(w);
            self.at_line_start = false;
            return;
        }
        self.push_space_if_needed(&TokenKind::Word(w.to_owned()));
        self.out.push_str(w);
        self.at_line_start = false;
    }
}

fn render_tokens(tokens: &[TokenKind]) -> String {
    let mut writer = FormatWriter::new();
    for (idx, tok) in tokens.iter().enumerate() {
        writer.write_token(tok, tokens.get(idx + 1));
    }
    writer.out.trim().to_owned()
}

fn is_top_clause_keyword(w: &str) -> bool {
    matches!(
        w,
        "SELECT"
            | "FROM"
            | "WHERE"
            | "INNER JOIN"
            | "LEFT JOIN"
            | "RIGHT JOIN"
            | "FULL JOIN"
            | "LEFT OUTER JOIN"
            | "RIGHT OUTER JOIN"
            | "FULL OUTER JOIN"
            | "CROSS JOIN"
            | "JOIN"
            | "GROUP BY"
            | "ORDER BY"
            | "HAVING"
            | "LIMIT"
            | "OFFSET"
            | "UNION"
            | "UNION ALL"
            | "INTERSECT"
            | "EXCEPT"
            | "INSERT INTO"
            | "VALUES"
            | "UPDATE"
            | "SET"
            | "DELETE FROM"
            | "CREATE TABLE"
            | "RETURNING"
    )
}

fn is_clause_or_op_keyword(w: &str) -> bool {
    matches!(
        w,
        "IN" | "VALUES" | "AND" | "OR" | "ON" | "EXISTS" | "NOT" | "AS" | "SET" | "WHERE" | "FROM"
    ) || is_top_clause_keyword(w)
}

fn is_sql_keyword(word: &str) -> bool {
    let upper = word.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "SELECT"
            | "FROM"
            | "WHERE"
            | "INNER"
            | "LEFT"
            | "RIGHT"
            | "FULL"
            | "OUTER"
            | "CROSS"
            | "JOIN"
            | "ON"
            | "AND"
            | "OR"
            | "GROUP"
            | "ORDER"
            | "BY"
            | "HAVING"
            | "LIMIT"
            | "OFFSET"
            | "UNION"
            | "ALL"
            | "INTERSECT"
            | "EXCEPT"
            | "INSERT"
            | "INTO"
            | "VALUES"
            | "UPDATE"
            | "SET"
            | "DELETE"
            | "CREATE"
            | "ALTER"
            | "DROP"
            | "TABLE"
            | "INDEX"
            | "UNIQUE"
            | "AS"
            | "IN"
            | "IS"
            | "NOT"
            | "NULL"
            | "TRUE"
            | "FALSE"
            | "CASE"
            | "WHEN"
            | "THEN"
            | "ELSE"
            | "END"
            | "DISTINCT"
            | "ASC"
            | "DESC"
            | "BETWEEN"
            | "LIKE"
            | "ILIKE"
            | "EXISTS"
            | "PRIMARY"
            | "FOREIGN"
            | "KEY"
            | "REFERENCES"
            | "DEFAULT"
            | "IF"
            | "RETURNING"
            | "WITH"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_sql_normalizes_keywords_and_preserves_literals_and_comments() {
        let input = r#"
            -- fetch active users
            select id, name, 'select from where' as "from", `order`
            from users u
            inner join orders o on u.id = o.user_id
            where u.active = true and o.total > 10 /* keep comment */
            group by id, name
            order by id desc
            limit 25;
        "#;
        let formatted = format_sql(input);
        assert!(formatted.contains("-- fetch active users"));
        assert!(formatted.contains("'select from where'"));
        assert!(formatted.contains("\"from\""));
        assert!(formatted.contains("`order`"));
        assert!(formatted.contains("/* keep comment */"));
        assert!(formatted.contains("SELECT\n  id,\n  name,"));
        assert!(formatted.contains("\nFROM users u"));
        assert!(formatted.contains("\nINNER JOIN orders o ON u.id = o.user_id"));
        assert!(formatted.contains("\nWHERE u.active = TRUE\n  AND o.total > 10"));
        assert!(formatted.contains("\nGROUP BY id, name"));
        assert!(formatted.contains("\nORDER BY id DESC"));
        assert!(formatted.contains("\nLIMIT 25;"));
    }

    #[test]
    fn format_sql_is_idempotent() {
        let samples = [
            "select a, b from t where a = 1 and b = 2 order by a desc limit 10;",
            "insert into users (id, name) values (1, 'Alice');",
            "update users set name = 'Bob', active = false where id = 2;",
            "delete from logs where created_at < '2026-01-01';",
            "select * from (select id from users) sub;",
        ];
        for sample in samples {
            let once = format_sql(sample);
            let twice = format_sql(&once);
            assert_eq!(once, twice, "not idempotent for: {sample}");
        }
    }

    #[test]
    fn find_and_replace_respects_case_and_whole_word() {
        let sql = "SELECT id, user_id FROM users WHERE ID = 1";
        let insensitive = find_matches(
            sql,
            "id",
            FindReplaceOptions {
                case_sensitive: false,
                whole_word: true,
            },
        );
        assert_eq!(insensitive.len(), 2);

        let (replaced, count) = replace_all_matches(
            sql,
            "id",
            "account_id",
            FindReplaceOptions {
                case_sensitive: true,
                whole_word: true,
            },
        );
        assert_eq!(count, 1);
        assert_eq!(
            replaced,
            "SELECT account_id, user_id FROM users WHERE ID = 1"
        );
    }
}
