//! Data import wizard (CSV, JSON, NDJSON, XLSX) and SQL script statement splitter (Feature 2).

use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::ddl_builder::{ColumnSpec, CreateTableSpec, DatabaseEngine, build_create_table_sql};
use crate::error::ServiceError;

/// Supported tabular import file formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportFormat {
    Csv,
    JsonArray,
    JsonLines,
    Xlsx,
}

/// Preview metadata and sample rows parsed from an import source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedImportPreview {
    pub columns: Vec<String>,
    pub sample_rows: Vec<Vec<String>>,
    pub total_rows: usize,
    pub inferred_types: Vec<String>,
}

/// Mapping from a source column index to a destination table column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnMapping {
    pub source_index: usize,
    pub target_column: String,
    pub target_type: String,
    pub include: bool,
}

/// Infers the [`ImportFormat`] from the file extension and (for `.json`) file header bytes.
///
/// # Errors
///
/// Returns [`ServiceError::Import`] if the format is unrecognized or the file cannot be read.
pub fn detect_import_format(path: &Path) -> Result<ImportFormat, ServiceError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "csv" | "tsv" => Ok(ImportFormat::Csv),
        "xlsx" => Ok(ImportFormat::Xlsx),
        "jsonl" | "ndjson" => Ok(ImportFormat::JsonLines),
        "json" => detect_json_variant(path),
        other => Err(ServiceError::Import(format!(
            "unsupported import extension: '{other}'"
        ))),
    }
}

fn detect_json_variant(path: &Path) -> Result<ImportFormat, ServiceError> {
    let mut file = File::open(path)?;
    let mut buf = [0_u8; 4096];
    let n = file.read(&mut buf)?;
    let head = std::str::from_utf8(&buf[..n])
        .map_err(|err| ServiceError::Import(format!("invalid UTF-8 in JSON file: {err}")))?;
    match head.trim_start().chars().next() {
        Some('[') => Ok(ImportFormat::JsonArray),
        Some('{') => Ok(ImportFormat::JsonLines),
        _ => Err(ServiceError::Import(
            "JSON import file must start with '[' (array) or '{' (NDJSON)".to_owned(),
        )),
    }
}

/// Parses an import file, returning headers, up to `limit_rows` sample rows, total row count, and inferred SQL types.
///
/// # Errors
///
/// Returns [`ServiceError::Import`] or [`ServiceError::Io`] on read or parse failure.
pub fn preview_import_file(
    path: &Path,
    format_opt: Option<ImportFormat>,
    limit_rows: usize,
) -> Result<ParsedImportPreview, ServiceError> {
    let format = match format_opt {
        Some(f) => f,
        None => detect_import_format(path)?,
    };
    let (columns, rows) = read_all_import_rows(path, Some(format))?;
    let total_rows = rows.len();
    let sample_limit = if limit_rows == 0 { 0 } else { limit_rows };
    let sample_rows: Vec<Vec<String>> = rows.into_iter().take(sample_limit).collect();
    let inferred_types = infer_column_types(columns.len(), &sample_rows);
    Ok(ParsedImportPreview {
        columns,
        sample_rows,
        total_rows,
        inferred_types,
    })
}

/// Reads all header names and data rows from an import file.
///
/// # Errors
///
/// Returns [`ServiceError::Import`] or [`ServiceError::Io`] if reading or parsing fails.
pub fn read_all_import_rows(
    path: &Path,
    format_opt: Option<ImportFormat>,
) -> Result<(Vec<String>, Vec<Vec<String>>), ServiceError> {
    let format = match format_opt {
        Some(f) => f,
        None => detect_import_format(path)?,
    };
    match format {
        ImportFormat::Csv => parse_csv_file(path),
        ImportFormat::JsonArray => parse_json_array_file(path),
        ImportFormat::JsonLines => parse_json_lines_file(path),
        ImportFormat::Xlsx => parse_xlsx_file(path),
    }
}

fn parse_csv_file(path: &Path) -> Result<(Vec<String>, Vec<Vec<String>>), ServiceError> {
    let is_tsv = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("tsv"));
    let delimiter = if is_tsv { b'\t' } else { b',' };
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .from_path(path)
        .map_err(|err| ServiceError::Import(err.to_string()))?;
    let headers: Vec<String> = reader
        .headers()
        .map_err(|err| ServiceError::Import(err.to_string()))?
        .iter()
        .map(str::to_owned)
        .collect();
    let col_count = headers.len();
    let mut rows = Vec::new();
    for rec in reader.records() {
        let rec = rec.map_err(|err| ServiceError::Import(err.to_string()))?;
        let mut row: Vec<String> = rec.iter().map(str::to_owned).collect();
        row.resize(col_count, String::new());
        rows.push(row);
    }
    Ok((headers, rows))
}

fn parse_json_array_file(path: &Path) -> Result<(Vec<String>, Vec<Vec<String>>), ServiceError> {
    let file = File::open(path)?;
    let value: serde_json::Value = serde_json::from_reader(BufReader::new(file))
        .map_err(|err| ServiceError::Import(err.to_string()))?;
    let Some(items) = value.as_array() else {
        return Err(ServiceError::Import(
            "expected a JSON array at top level".to_owned(),
        ));
    };
    rows_from_json_objects(items)
}

fn parse_json_lines_file(path: &Path) -> Result<(Vec<String>, Vec<Vec<String>>), ServiceError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut items = Vec::new();
    for (idx, line) in reader.lines().enumerate() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let val: serde_json::Value = serde_json::from_str(trimmed)
            .map_err(|err| ServiceError::Import(format!("line {}: {err}", idx + 1)))?;
        items.push(val);
    }
    rows_from_json_objects(&items)
}

fn rows_from_json_objects(
    items: &[serde_json::Value],
) -> Result<(Vec<String>, Vec<Vec<String>>), ServiceError> {
    let mut columns = Vec::new();
    let mut seen = BTreeSet::new();
    for item in items {
        let serde_json::Value::Object(obj) = item else {
            return Err(ServiceError::Import(
                "JSON import elements must be JSON objects".to_owned(),
            ));
        };
        for key in obj.keys() {
            if seen.insert(key.clone()) {
                columns.push(key.clone());
            }
        }
    }
    let mut rows = Vec::with_capacity(items.len());
    for item in items {
        let serde_json::Value::Object(obj) = item else {
            continue;
        };
        let row = columns
            .iter()
            .map(|col| obj.get(col).map_or_else(String::new, json_value_to_cell))
            .collect();
        rows.push(row);
    }
    Ok((columns, rows))
}

fn json_value_to_cell(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => val.to_string(),
    }
}

fn parse_xlsx_file(path: &Path) -> Result<(Vec<String>, Vec<Vec<String>>), ServiceError> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .map_err(|err| ServiceError::Import(err.to_string()))?;
    let shared_strings = read_xlsx_shared_strings(&mut archive)?;
    let sheet_xml = read_first_xlsx_sheet_xml(&mut archive)?;
    let raw_rows = parse_xlsx_sheet_rows(&sheet_xml, &shared_strings);
    if raw_rows.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let width = raw_rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut iter = raw_rows.into_iter();
    let mut headers = iter.next().unwrap_or_default();
    headers.resize(width, String::new());
    for (idx, h) in headers.iter_mut().enumerate() {
        if h.trim().is_empty() {
            *h = format!("column_{}", idx + 1);
        }
    }
    let rows = iter
        .map(|mut r| {
            r.resize(width, String::new());
            r
        })
        .collect();
    Ok((headers, rows))
}

fn read_xlsx_shared_strings(
    archive: &mut ZipArchive<BufReader<File>>,
) -> Result<Vec<String>, ServiceError> {
    let Ok(mut entry) = archive.by_name("xl/sharedStrings.xml") else {
        return Ok(Vec::new());
    };
    let mut xml = String::new();
    entry.read_to_string(&mut xml)?;
    Ok(parse_shared_strings_xml(&xml))
}

fn read_first_xlsx_sheet_xml(
    archive: &mut ZipArchive<BufReader<File>>,
) -> Result<String, ServiceError> {
    if let Ok(mut sheet1) = archive.by_name("xl/worksheets/sheet1.xml") {
        let mut xml = String::new();
        sheet1.read_to_string(&mut xml)?;
        return Ok(xml);
    }
    let mut sheet_names: Vec<String> = (0..archive.len())
        .filter_map(|i| {
            let file = archive.by_index(i).ok()?;
            let name = file.name().ok()?.into_owned();
            (name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml")).then_some(name)
        })
        .collect();
    sheet_names.sort();
    let first = sheet_names
        .first()
        .ok_or_else(|| ServiceError::Import("XLSX archive contains no worksheet".to_owned()))?;
    let mut entry = archive
        .by_name(first)
        .map_err(|err| ServiceError::Import(err.to_string()))?;
    let mut xml = String::new();
    entry.read_to_string(&mut xml)?;
    Ok(xml)
}

fn parse_shared_strings_xml(xml: &str) -> Vec<String> {
    let mut strings = Vec::new();
    let mut cursor = xml;
    while let Some(si_start) = cursor.find("<si") {
        let after_open = &cursor[si_start..];
        let Some(si_end) = after_open.find("</si>") else {
            break;
        };
        let si_body = &after_open[..si_end];
        strings.push(extract_all_t_elements(si_body));
        cursor = &after_open[si_end + 5..];
    }
    strings
}

fn extract_all_t_elements(fragment: &str) -> String {
    let mut out = String::new();
    let mut cursor = fragment;
    while let Some(t_pos) = find_tag_start(cursor, "t") {
        let after_t = &cursor[t_pos..];
        let Some(gt) = after_t.find('>') else {
            break;
        };
        if after_t[..gt].ends_with('/') {
            cursor = &after_t[gt + 1..];
            continue;
        }
        let content_start = &after_t[gt + 1..];
        let Some(close) = content_start.find("</t>") else {
            break;
        };
        out.push_str(&unescape_xml(&content_start[..close]));
        cursor = &content_start[close + 4..];
    }
    out
}

fn find_tag_start(haystack: &str, tag: &str) -> Option<usize> {
    let pattern = format!("<{tag}");
    let mut offset = 0;
    while let Some(pos) = haystack[offset..].find(&pattern) {
        let abs = offset + pos;
        let next_char = haystack[abs + pattern.len()..].chars().next();
        if matches!(next_char, Some('>' | ' ' | '\t' | '\r' | '\n' | '/')) {
            return Some(abs);
        }
        offset = abs + pattern.len();
    }
    None
}

fn parse_xlsx_sheet_rows(xml: &str, shared_strings: &[String]) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut cursor = xml;
    while let Some(row_start) = find_tag_start(cursor, "row") {
        let after_row = &cursor[row_start..];
        let Some(gt) = after_row.find('>') else {
            break;
        };
        if after_row[..gt].ends_with('/') {
            cursor = &after_row[gt + 1..];
            continue;
        }
        let Some(row_end) = after_row.find("</row>") else {
            break;
        };
        let row_body = &after_row[gt + 1..row_end];
        let parsed_row = parse_xlsx_row_cells(row_body, shared_strings);
        if !parsed_row.is_empty() {
            rows.push(parsed_row);
        }
        cursor = &after_row[row_end + 6..];
    }
    rows
}

fn parse_xlsx_row_cells(row_body: &str, shared_strings: &[String]) -> Vec<String> {
    let mut cells: HashMap<usize, String> = HashMap::new();
    let mut max_col = 0_usize;
    let mut fallback_col = 0_usize;
    let mut cursor = row_body;
    while let Some(c_start) = find_tag_start(cursor, "c") {
        let after_c = &cursor[c_start..];
        let Some(gt) = after_c.find('>') else {
            break;
        };
        let attrs = &after_c[..gt];
        if attrs.ends_with('/') {
            fallback_col += 1;
            cursor = &after_c[gt + 1..];
            continue;
        }
        let Some(c_end) = after_c.find("</c>") else {
            break;
        };
        let body = &after_c[gt + 1..c_end];
        let col_idx = extract_xml_attr(attrs, "r")
            .and_then(|r| column_index_from_ref(&r))
            .unwrap_or(fallback_col);
        fallback_col = col_idx + 1;
        max_col = max_col.max(col_idx + 1);
        let cell_type = extract_xml_attr(attrs, "t").unwrap_or_default();
        let val = decode_xlsx_cell_value(&cell_type, body, shared_strings);
        cells.insert(col_idx, val);
        cursor = &after_c[c_end + 4..];
    }
    (0..max_col)
        .map(|i| cells.remove(&i).unwrap_or_default())
        .collect()
}

fn decode_xlsx_cell_value(cell_type: &str, body: &str, shared_strings: &[String]) -> String {
    match cell_type {
        "inlineStr" => extract_all_t_elements(body),
        "s" => {
            let idx = extract_tag_text(body, "v").and_then(|s| s.trim().parse::<usize>().ok());
            idx.and_then(|i| shared_strings.get(i))
                .cloned()
                .unwrap_or_default()
        }
        "b" => match extract_tag_text(body, "v").as_deref().map(str::trim) {
            Some("1") => "true".to_owned(),
            Some("0") => "false".to_owned(),
            Some(other) => other.to_owned(),
            None => String::new(),
        },
        _ => extract_tag_text(body, "v").unwrap_or_default(),
    }
}

fn extract_tag_text(fragment: &str, tag: &str) -> Option<String> {
    let start = find_tag_start(fragment, tag)?;
    let after = &fragment[start..];
    let gt = after.find('>')?;
    if after[..gt].ends_with('/') {
        return Some(String::new());
    }
    let content = &after[gt + 1..];
    let close_tag = format!("</{tag}>");
    let end = content.find(&close_tag)?;
    Some(unescape_xml(&content[..end]))
}

fn extract_xml_attr(attrs: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let needle = format!("{name}={quote}");
        if let Some(pos) = attrs.find(&needle) {
            let after = &attrs[pos + needle.len()..];
            if let Some(end) = after.find(quote) {
                return Some(unescape_xml(&after[..end]));
            }
        }
    }
    None
}

fn column_index_from_ref(cell_ref: &str) -> Option<usize> {
    let letters: String = cell_ref
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    if letters.is_empty() {
        return None;
    }
    let mut idx = 0_usize;
    for ch in letters.chars() {
        let digit = (ch.to_ascii_uppercase() as usize).checked_sub('A' as usize)? + 1;
        idx = idx * 26 + digit;
    }
    idx.checked_sub(1)
}

fn unescape_xml(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_owned();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after_amp = &rest[amp + 1..];
        if let Some(semi) = after_amp.find(';') {
            let entity = &after_amp[..semi];
            if let Some(ch) = decode_xml_entity(entity) {
                out.push(ch);
                rest = &after_amp[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = after_amp;
    }
    out.push_str(rest);
    out
}

fn decode_xml_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ if entity.starts_with("#x") || entity.starts_with("#X") => {
            u32::from_str_radix(&entity[2..], 16)
                .ok()
                .and_then(char::from_u32)
        }
        _ if entity.starts_with('#') => entity[1..].parse::<u32>().ok().and_then(char::from_u32),
        _ => None,
    }
}

fn infer_column_types(col_count: usize, rows: &[Vec<String>]) -> Vec<String> {
    (0..col_count)
        .map(|col_idx| {
            let values: Vec<&str> = rows
                .iter()
                .filter_map(|r| r.get(col_idx).map(String::as_str))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            infer_single_column_type(&values)
        })
        .collect()
}

fn infer_single_column_type(values: &[&str]) -> String {
    if values.is_empty() {
        return "TEXT".to_owned();
    }
    if values
        .iter()
        .all(|v| v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("false"))
    {
        return "BOOLEAN".to_owned();
    }
    if values.iter().all(|v| v.parse::<i64>().is_ok()) {
        return "INTEGER".to_owned();
    }
    if values
        .iter()
        .all(|v| v.parse::<f64>().is_ok_and(f64::is_finite))
    {
        return "REAL".to_owned();
    }
    if values.iter().all(|v| is_iso_date(v)) {
        return "DATE".to_owned();
    }
    if values.iter().all(|v| is_iso_timestamp(v)) {
        return "TIMESTAMP".to_owned();
    }
    "TEXT".to_owned()
}

fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[8..10].iter().all(u8::is_ascii_digit)
}

fn is_iso_timestamp(s: &str) -> bool {
    s.len() >= 19
        && is_iso_date(&s[..10])
        && matches!(s.as_bytes()[10], b'T' | b' ')
        && s.as_bytes()[13] == b':'
        && s.as_bytes()[16] == b':'
}

/// Generates dialect-aware `CREATE TABLE` (optional) and batched `INSERT` SQL statements.
#[must_use]
pub fn generate_import_sql_batches(
    engine: DatabaseEngine,
    schema: Option<&str>,
    table: &str,
    create_table_if_missing: bool,
    mappings: &[ColumnMapping],
    rows: &[Vec<String>],
    batch_size: usize,
) -> Vec<String> {
    let active: Vec<&ColumnMapping> = mappings.iter().filter(|m| m.include).collect();
    if active.is_empty() {
        return Vec::new();
    }
    let mut batches = Vec::new();
    if create_table_if_missing {
        let cols = active
            .iter()
            .map(|m| ColumnSpec {
                name: m.target_column.clone(),
                data_type: if m.target_type.trim().is_empty() {
                    "TEXT".to_owned()
                } else {
                    m.target_type.clone()
                },
                nullable: true,
                default_expr: None,
                is_primary_key: false,
                is_unique: false,
                auto_increment: false,
            })
            .collect();
        let spec = CreateTableSpec {
            table_name: table.to_owned(),
            columns: cols,
            foreign_keys: Vec::new(),
            indexes: Vec::new(),
            if_not_exists: true,
        };
        batches.push(build_create_table_sql(engine, schema, &spec));
    }
    if rows.is_empty() {
        return batches;
    }

    let qualified = engine.qualify_table(schema, table);
    let col_list = active
        .iter()
        .map(|m| engine.quote_identifier(&m.target_column))
        .collect::<Vec<_>>()
        .join(", ");
    let effective_batch = match engine {
        DatabaseEngine::Mssql => batch_size.clamp(1, 1000),
        _ => batch_size.max(1),
    };
    for chunk in rows.chunks(effective_batch) {
        let tuples: Vec<String> = chunk
            .iter()
            .map(|row| {
                let vals = active
                    .iter()
                    .map(|m| {
                        let raw = row.get(m.source_index).map_or("", String::as_str);
                        format_import_literal(engine, raw, &m.target_type)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({vals})")
            })
            .collect();
        batches.push(format!(
            "INSERT INTO {qualified} ({col_list}) VALUES\n  {};",
            tuples.join(",\n  ")
        ));
    }
    batches
}

fn format_import_literal(engine: DatabaseEngine, raw: &str, target_type: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "NULL".to_owned();
    }
    let ty = target_type.trim().to_ascii_uppercase();
    if (ty.contains("INT")
        || ty.contains("REAL")
        || ty.contains("FLOAT")
        || ty.contains("DOUBLE")
        || ty.contains("NUMERIC")
        || ty.contains("DECIMAL")
        || ty.contains("NUMBER"))
        && (trimmed.parse::<i64>().is_ok() || trimmed.parse::<f64>().is_ok_and(f64::is_finite))
    {
        return trimmed.to_owned();
    }
    if ty.contains("BOOL") {
        let bool_val = match trimmed.to_ascii_lowercase().as_str() {
            "true" | "1" | "t" | "yes" => Some(true),
            "false" | "0" | "f" | "no" => Some(false),
            _ => None,
        };
        if let Some(b) = bool_val {
            return match engine {
                DatabaseEngine::Sqlite | DatabaseEngine::Mssql => {
                    if b { "1" } else { "0" }.to_owned()
                }
                _ => if b { "TRUE" } else { "FALSE" }.to_owned(),
            };
        }
    }
    engine.quote_literal(raw)
}

/// Splits a SQL script into individual statements, preserving string literals, comments, and PostgreSQL `$$...$$` dollar-quoted blocks.
#[must_use]
pub fn split_sql_script_statements(script: &str) -> Vec<String> {
    let mut splitter = SqlScriptSplitter::default();
    splitter.push_str(script);
    splitter.finish()
}

/// Streaming variant of [`split_sql_script_statements`] reading from any [`BufRead`].
///
/// # Errors
///
/// Returns [`ServiceError::Io`] if reading fails.
pub fn split_sql_script_reader<R: BufRead>(mut reader: R) -> Result<Vec<String>, ServiceError> {
    let mut splitter = SqlScriptSplitter::default();
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            break;
        }
        splitter.push_str(&line);
    }
    Ok(splitter.finish())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LexState {
    Normal,
    SingleQuote { escaped: bool },
    DoubleQuote,
    Backtick,
    LineComment,
    BlockComment { depth: usize },
    DollarQuote { tag: String },
}

struct SqlScriptSplitter {
    state: LexState,
    current: String,
    has_code: bool,
    statements: Vec<String>,
}

impl Default for SqlScriptSplitter {
    fn default() -> Self {
        Self {
            state: LexState::Normal,
            current: String::new(),
            has_code: false,
            statements: Vec::new(),
        }
    }
}

impl SqlScriptSplitter {
    fn push_str(&mut self, chunk: &str) {
        let bytes = chunk.as_bytes();
        let mut i = 0_usize;
        while i < bytes.len() {
            let ch = chunk[i..].chars().next().unwrap_or('\0');
            let ch_len = ch.len_utf8();
            match &mut self.state {
                LexState::Normal => i += self.step_normal(chunk, i, ch, ch_len),
                LexState::SingleQuote { escaped } => {
                    self.current.push(ch);
                    if *escaped {
                        *escaped = false;
                    } else if ch == '\\' {
                        *escaped = true;
                    } else if ch == '\'' {
                        if chunk[i + ch_len..].starts_with('\'') {
                            self.current.push('\'');
                            i += ch_len;
                        } else {
                            self.state = LexState::Normal;
                        }
                    }
                    i += ch_len;
                }
                LexState::DoubleQuote => {
                    self.current.push(ch);
                    if ch == '"' {
                        if chunk[i + ch_len..].starts_with('"') {
                            self.current.push('"');
                            i += ch_len;
                        } else {
                            self.state = LexState::Normal;
                        }
                    }
                    i += ch_len;
                }
                LexState::Backtick => {
                    self.current.push(ch);
                    if ch == '`' {
                        if chunk[i + ch_len..].starts_with('`') {
                            self.current.push('`');
                            i += ch_len;
                        } else {
                            self.state = LexState::Normal;
                        }
                    }
                    i += ch_len;
                }
                LexState::LineComment => {
                    self.current.push(ch);
                    if ch == '\n' {
                        self.state = LexState::Normal;
                    }
                    i += ch_len;
                }
                LexState::BlockComment { depth } => {
                    if chunk[i..].starts_with("/*") {
                        self.current.push_str("/*");
                        *depth += 1;
                        i += 2;
                    } else if chunk[i..].starts_with("*/") {
                        self.current.push_str("*/");
                        *depth -= 1;
                        if *depth == 0 {
                            self.state = LexState::Normal;
                        }
                        i += 2;
                    } else {
                        self.current.push(ch);
                        i += ch_len;
                    }
                }
                LexState::DollarQuote { tag } => {
                    if chunk[i..].starts_with(tag.as_str()) {
                        self.current.push_str(tag);
                        let len = tag.len();
                        self.state = LexState::Normal;
                        i += len;
                    } else {
                        self.current.push(ch);
                        i += ch_len;
                    }
                }
            }
        }
    }

    fn step_normal(&mut self, chunk: &str, i: usize, ch: char, ch_len: usize) -> usize {
        let rest = &chunk[i..];
        if rest.starts_with("--") {
            self.current.push_str("--");
            self.state = LexState::LineComment;
            return 2;
        }
        if rest.starts_with("/*") {
            self.current.push_str("/*");
            self.state = LexState::BlockComment { depth: 1 };
            return 2;
        }
        if ch == '$'
            && let Some(tag) = parse_dollar_tag(rest)
        {
            self.has_code = true;
            self.current.push_str(&tag);
            let len = tag.len();
            self.state = LexState::DollarQuote { tag };
            return len;
        }
        match ch {
            ';' => {
                self.flush_statement();
                ch_len
            }
            '\'' => {
                self.has_code = true;
                self.current.push(ch);
                self.state = LexState::SingleQuote { escaped: false };
                ch_len
            }
            '"' => {
                self.has_code = true;
                self.current.push(ch);
                self.state = LexState::DoubleQuote;
                ch_len
            }
            '`' => {
                self.has_code = true;
                self.current.push(ch);
                self.state = LexState::Backtick;
                ch_len
            }
            c if c.is_whitespace() => {
                self.current.push(c);
                ch_len
            }
            _ => {
                self.has_code = true;
                self.current.push(ch);
                ch_len
            }
        }
    }

    fn flush_statement(&mut self) {
        let trimmed = self.current.trim();
        if self.has_code && !trimmed.is_empty() {
            self.statements.push(trimmed.to_owned());
        }
        self.current.clear();
        self.has_code = false;
    }

    fn finish(mut self) -> Vec<String> {
        self.flush_statement();
        self.statements
    }
}

fn parse_dollar_tag(rest: &str) -> Option<String> {
    let bytes = rest.as_bytes();
    if bytes.first().copied() != Some(b'$') {
        return None;
    }
    if bytes.get(1).copied() == Some(b'$') {
        return Some("$$".to_owned());
    }
    let first = bytes.get(1).copied()?;
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return None;
    }
    let mut idx = 2_usize;
    while let Some(&b) = bytes.get(idx) {
        if b == b'$' {
            return Some(rest[..=idx].to_owned());
        }
        if !(b.is_ascii_alphanumeric() || b == b'_') {
            return None;
        }
        idx += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    #[test]
    fn preview_csv_and_infer_types() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("users.csv");
        std::fs::write(
            &path,
            "id,name,active,score,joined\n1,Alice,true,98.5,2026-01-15\n2,Bob,false,87.0,2026-02-20\n",
        )?;
        let preview = preview_import_file(&path, None, 1)?;
        assert_eq!(
            preview.columns,
            vec!["id", "name", "active", "score", "joined"]
        );
        assert_eq!(preview.sample_rows.len(), 1);
        assert_eq!(preview.total_rows, 2);
        assert_eq!(
            preview.inferred_types,
            vec!["INTEGER", "TEXT", "BOOLEAN", "REAL", "DATE"]
        );
        Ok(())
    }

    #[test]
    fn preview_json_array_and_ndjson() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let json_path = dir.path().join("items.json");
        std::fs::write(
            &json_path,
            r#"[{"id": 1, "title": "A"}, {"id": 2, "title": "B", "extra": true}]"#,
        )?;
        let p1 = preview_import_file(&json_path, None, 10)?;
        assert_eq!(p1.columns, vec!["id", "title", "extra"]);
        assert_eq!(p1.total_rows, 2);

        let ndjson_path = dir.path().join("events.ndjson");
        std::fs::write(&ndjson_path, "{\"ev\":\"login\"}\n{\"ev\":\"click\"}\n")?;
        let p2 = preview_import_file(&ndjson_path, None, 10)?;
        assert_eq!(p2.columns, vec!["ev"]);
        assert_eq!(p2.total_rows, 2);
        Ok(())
    }

    #[test]
    fn preview_xlsx_reads_shared_strings_and_inline_cells() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::tempdir()?;
        let xlsx_path = dir.path().join("book.xlsx");
        let file = File::create(&xlsx_path)?;
        let mut writer = zip::ZipWriter::new(file);
        let opts = SimpleFileOptions::default();

        writer.start_file("xl/sharedStrings.xml", opts)?;
        writer.write_all(
            br#"<sst><si><t>id</t></si><si><t>name &amp; role</t></si><si><t>Alice</t></si></sst>"#,
        )?;
        writer.start_file("xl/worksheets/sheet1.xml", opts)?;
        writer.write_all(
            br#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row><row r="2"><c r="A2"><v>42</v></c><c r="B2" t="s"><v>2</v></c></row></sheetData></worksheet>"#,
        )?;
        writer.finish()?;

        let preview = preview_import_file(&xlsx_path, None, 10)?;
        assert_eq!(preview.columns, vec!["id", "name & role"]);
        assert_eq!(preview.total_rows, 1);
        assert_eq!(preview.sample_rows[0], vec!["42", "Alice"]);
        assert_eq!(preview.inferred_types, vec!["INTEGER", "TEXT"]);
        Ok(())
    }

    #[test]
    fn generate_import_batches_creates_table_and_inserts() {
        let mappings = vec![
            ColumnMapping {
                source_index: 0,
                target_column: "id".to_owned(),
                target_type: "INTEGER".to_owned(),
                include: true,
            },
            ColumnMapping {
                source_index: 1,
                target_column: "name".to_owned(),
                target_type: "TEXT".to_owned(),
                include: true,
            },
        ];
        let rows = vec![
            vec!["1".to_owned(), "O'Brien".to_owned()],
            vec!["2".to_owned(), String::new()],
        ];
        let stmts = generate_import_sql_batches(
            DatabaseEngine::Postgres,
            Some("public"),
            "users",
            true,
            &mappings,
            &rows,
            1,
        );
        assert_eq!(stmts.len(), 3);
        assert!(stmts[0].contains("CREATE TABLE IF NOT EXISTS \"public\".\"users\""));
        assert!(stmts[1].contains("(1, 'O''Brien')"));
        assert!(stmts[2].contains("(2, NULL)"));
    }

    #[test]
    fn split_sql_script_handles_quotes_comments_and_dollar_quotes() {
        let script = r#"
            -- leading comment only; ignored if standalone
            SELECT 'a;b' AS txt;
            /* block ; comment /* nested ; */ */
            CREATE FUNCTION demo() RETURNS text AS $fn$
            BEGIN
                RETURN 'x;y';
            END;
            $fn$ LANGUAGE plpgsql;
            INSERT INTO logs(msg) VALUES ('done');
        "#;
        let stmts = split_sql_script_statements(script);
        assert_eq!(stmts.len(), 3);
        assert!(stmts[0].contains("SELECT 'a;b' AS txt"));
        assert!(stmts[1].contains("CREATE FUNCTION demo()"));
        assert!(stmts[1].contains("$fn$ LANGUAGE plpgsql"));
        assert!(stmts[2].contains("INSERT INTO logs(msg) VALUES ('done')"));
    }
}
