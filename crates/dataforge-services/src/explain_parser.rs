//! Visual EXPLAIN plan parser (Feature 6) and Visual Query Builder SQL generator (Feature 8).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::ddl_builder::DatabaseEngine;

/// Normalized execution plan tree node for visual EXPLAIN rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExplainNode {
    pub id: usize,
    pub label: String,
    pub detail: String,
    pub operation: String,
    pub relation: Option<String>,
    pub cost: Option<f64>,
    pub estimated_rows: Option<f64>,
    pub actual_time_ms: Option<f64>,
    pub actual_rows: Option<u64>,
    pub is_seq_scan_warning: bool,
    pub children: Vec<ExplainNode>,
}

/// Parses EXPLAIN / EXPLAIN ANALYZE / EXPLAIN QUERY PLAN output into a tree of [`ExplainNode`]s.
#[must_use]
pub fn parse_explain_output(
    engine: DatabaseEngine,
    columns: &[String],
    rows: &[Vec<String>],
) -> Vec<ExplainNode> {
    if rows.is_empty() {
        return Vec::new();
    }
    if let Some(first_cell) = rows.first().and_then(|r| r.first()).map(|s| s.trim())
        && (first_cell.starts_with('[') || first_cell.starts_with('{'))
    {
        let joined = collect_single_column_text(rows);
        if let Some(nodes) = try_parse_json_plan(&joined) {
            return nodes;
        }
    }

    match engine {
        DatabaseEngine::Sqlite => parse_sqlite_explain(columns, rows),
        DatabaseEngine::Mysql => parse_mysql_explain(columns, rows),
        DatabaseEngine::Postgres => parse_postgres_text_explain(rows),
        DatabaseEngine::Duckdb
        | DatabaseEngine::Clickhouse
        | DatabaseEngine::Mssql
        | DatabaseEngine::Generic => {
            if is_mysql_like_columns(columns) {
                parse_mysql_explain(columns, rows)
            } else if is_sqlite_eqp_columns(columns) {
                parse_sqlite_explain(columns, rows)
            } else {
                parse_generic_explain_rows(columns, rows)
            }
        }
    }
}

fn collect_single_column_text(rows: &[Vec<String>]) -> String {
    rows.iter()
        .filter_map(|r| r.first())
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n")
}

fn try_parse_json_plan(raw: &str) -> Option<Vec<ExplainNode>> {
    let val: serde_json::Value = serde_json::from_str(raw).ok()?;
    let mut next_id = 1_usize;
    if let Some(arr) = val.as_array() {
        let mut nodes = Vec::new();
        for item in arr {
            if let Some(plan) = item.get("Plan") {
                nodes.push(parse_pg_json_node(plan, &mut next_id));
            } else if item.is_object() {
                nodes.push(parse_generic_json_node(item, &mut next_id));
            }
        }
        if !nodes.is_empty() {
            return Some(nodes);
        }
    }
    if let Some(plan) = val.get("Plan") {
        return Some(vec![parse_pg_json_node(plan, &mut next_id)]);
    }
    if val.is_object() {
        return Some(vec![parse_generic_json_node(&val, &mut next_id)]);
    }
    None
}

fn parse_pg_json_node(val: &serde_json::Value, next_id: &mut usize) -> ExplainNode {
    let id = *next_id;
    *next_id += 1;
    let operation = val
        .get("Node Type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Plan")
        .to_owned();
    let relation = val
        .get("Relation Name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let cost = val.get("Total Cost").and_then(serde_json::Value::as_f64);
    let estimated_rows = val.get("Plan Rows").and_then(serde_json::Value::as_f64);
    let actual_time_ms = val
        .get("Actual Total Time")
        .and_then(serde_json::Value::as_f64);
    let actual_rows = val.get("Actual Rows").and_then(serde_json::Value::as_u64);
    let label = match &relation {
        Some(rel) => format!("{operation} on {rel}"),
        None => operation.clone(),
    };
    let detail = val
        .get("Filter")
        .or_else(|| val.get("Index Cond"))
        .or_else(|| val.get("Hash Cond"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_owned();
    let children = val
        .get("Plans")
        .and_then(serde_json::Value::as_array)
        .map(|arr| arr.iter().map(|c| parse_pg_json_node(c, next_id)).collect())
        .unwrap_or_default();
    let is_seq_scan_warning = operation.eq_ignore_ascii_case("Seq Scan");

    ExplainNode {
        id,
        label,
        detail,
        operation,
        relation,
        cost,
        estimated_rows,
        actual_time_ms,
        actual_rows,
        is_seq_scan_warning,
        children,
    }
}

fn parse_generic_json_node(val: &serde_json::Value, next_id: &mut usize) -> ExplainNode {
    let id = *next_id;
    *next_id += 1;
    let operation = val
        .get("operator_name")
        .or_else(|| val.get("name"))
        .or_else(|| val.get("Node Type"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Node")
        .to_owned();
    let relation = val
        .get("table")
        .or_else(|| val.get("relation"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let estimated_rows = val
        .get("estimated_cardinality")
        .or_else(|| val.get("cardinality"))
        .and_then(serde_json::Value::as_f64);
    let actual_rows = val
        .get("operator_cardinality")
        .and_then(serde_json::Value::as_u64);
    let detail = val
        .get("extra_info")
        .and_then(|v| {
            v.as_str()
                .map(str::to_owned)
                .or_else(|| Some(v.to_string()))
        })
        .unwrap_or_default();
    let children = val
        .get("children")
        .and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|c| parse_generic_json_node(c, next_id))
                .collect()
        })
        .unwrap_or_default();
    let upper = operation.to_ascii_uppercase();
    let is_seq_scan_warning = upper.contains("SEQ_SCAN") || upper.contains("SEQ SCAN");

    ExplainNode {
        id,
        label: match &relation {
            Some(r) => format!("{operation} ({r})"),
            None => operation.clone(),
        },
        detail,
        operation,
        relation,
        cost: None,
        estimated_rows,
        actual_time_ms: None,
        actual_rows,
        is_seq_scan_warning,
        children,
    }
}

fn is_sqlite_eqp_columns(columns: &[String]) -> bool {
    columns.len() >= 3
        && columns[0].eq_ignore_ascii_case("id")
        && columns[1].eq_ignore_ascii_case("parent")
}

fn parse_sqlite_explain(columns: &[String], rows: &[Vec<String>]) -> Vec<ExplainNode> {
    if rows
        .iter()
        .all(|r| r.len() >= 4 && r[0].trim().parse::<usize>().is_ok())
    {
        return build_sqlite_eqp_tree(rows);
    }
    if is_sqlite_eqp_columns(columns) {
        return build_sqlite_eqp_tree(rows);
    }
    parse_generic_explain_rows(columns, rows)
}

fn build_sqlite_eqp_tree(rows: &[Vec<String>]) -> Vec<ExplainNode> {
    let mut nodes: Vec<(usize, usize, ExplainNode)> = Vec::with_capacity(rows.len());
    for (idx, row) in rows.iter().enumerate() {
        let id = row
            .first()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(idx + 1);
        let parent = row
            .get(1)
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(0);
        let detail = row.last().map_or("", String::as_str).trim().to_owned();
        let (operation, relation) = parse_sqlite_eqp_detail(&detail);
        let is_seq_scan_warning = detail.to_ascii_uppercase().starts_with("SCAN ")
            && !detail.to_ascii_uppercase().contains("USING INDEX")
            && !detail.to_ascii_uppercase().contains("USING COVERING INDEX");
        nodes.push((
            id,
            parent,
            ExplainNode {
                id,
                label: detail.clone(),
                detail,
                operation,
                relation,
                cost: None,
                estimated_rows: None,
                actual_time_ms: None,
                actual_rows: None,
                is_seq_scan_warning,
                children: Vec::new(),
            },
        ));
    }

    let mut children_map: HashMap<usize, Vec<ExplainNode>> = HashMap::new();
    let ids: Vec<usize> = nodes.iter().map(|(id, _, _)| *id).collect();
    for (id, parent, mut node) in nodes.into_iter().rev() {
        if let Some(kids) = children_map.remove(&id) {
            node.children = kids;
        }
        let target_parent = if ids.contains(&parent) { parent } else { 0 };
        children_map
            .entry(target_parent)
            .or_default()
            .insert(0, node);
    }
    children_map.remove(&0).unwrap_or_default()
}

fn parse_sqlite_eqp_detail(detail: &str) -> (String, Option<String>) {
    let upper = detail.to_ascii_uppercase();
    if upper.starts_with("SCAN ") {
        let rel = detail[5..].split_whitespace().next().map(str::to_owned);
        return ("SCAN".to_owned(), rel);
    }
    if upper.starts_with("SEARCH ") {
        let rel = detail[7..].split_whitespace().next().map(str::to_owned);
        return ("SEARCH".to_owned(), rel);
    }
    let op = detail
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");
    (if op.is_empty() { "STEP".to_owned() } else { op }, None)
}

fn is_mysql_like_columns(columns: &[String]) -> bool {
    columns
        .iter()
        .any(|c| c.eq_ignore_ascii_case("select_type"))
        || (columns.iter().any(|c| c.eq_ignore_ascii_case("type"))
            && columns.iter().any(|c| c.eq_ignore_ascii_case("table")))
}

fn parse_mysql_explain(columns: &[String], rows: &[Vec<String>]) -> Vec<ExplainNode> {
    let col_idx = |name: &str| columns.iter().position(|c| c.eq_ignore_ascii_case(name));
    let table_i = col_idx("table");
    let type_i = col_idx("type");
    let rows_i = col_idx("rows");
    let key_i = col_idx("key");
    let extra_i = col_idx("Extra");
    let select_type_i = col_idx("select_type");

    rows.iter()
        .enumerate()
        .map(|(idx, row)| {
            let get = |opt: Option<usize>| -> String {
                opt.and_then(|i| row.get(i))
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("NULL"))
                    .unwrap_or_default()
            };
            let table = get(table_i);
            let access = get(type_i);
            let sel_type = get(select_type_i);
            let key = get(key_i);
            let extra = get(extra_i);
            let est_rows = get(rows_i).parse::<f64>().ok();
            let op = if access.is_empty() {
                sel_type
            } else {
                access.clone()
            };
            let label = if table.is_empty() {
                op.clone()
            } else {
                format!("{op} on {table}")
            };
            let mut detail_parts = Vec::new();
            if !key.is_empty() {
                detail_parts.push(format!("key={key}"));
            }
            if !extra.is_empty() {
                detail_parts.push(extra);
            }
            ExplainNode {
                id: idx + 1,
                label,
                detail: detail_parts.join("; "),
                operation: op,
                relation: (!table.is_empty()).then_some(table),
                cost: None,
                estimated_rows: est_rows,
                actual_time_ms: None,
                actual_rows: None,
                is_seq_scan_warning: access.eq_ignore_ascii_case("ALL"),
                children: Vec::new(),
            }
        })
        .collect()
}

fn parse_postgres_text_explain(rows: &[Vec<String>]) -> Vec<ExplainNode> {
    let mut stack: Vec<(usize, ExplainNode)> = Vec::new();
    let mut roots: Vec<ExplainNode> = Vec::new();
    let mut next_id = 1_usize;

    for row in rows {
        let line = row.first().map_or("", String::as_str);
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("Planning Time:")
            || trimmed.starts_with("Execution Time:")
            || trimmed.starts_with("Triggers:")
        {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let clean = trimmed.strip_prefix("->").map_or(trimmed, str::trim);
        if !trimmed.starts_with("->") && !stack.is_empty() && !clean.contains("(cost=") {
            if let Some((_, last)) = stack.last_mut() {
                if !last.detail.is_empty() {
                    last.detail.push_str("; ");
                }
                last.detail.push_str(clean);
            }
            continue;
        }

        let node = parse_pg_text_line(clean, next_id);
        next_id += 1;
        while let Some((top_indent, _)) = stack.last() {
            if *top_indent >= indent {
                if let Some((_, finished)) = stack.pop() {
                    attach_pg_node(finished, &mut stack, &mut roots);
                }
            } else {
                break;
            }
        }
        stack.push((indent, node));
    }

    while let Some((_, finished)) = stack.pop() {
        attach_pg_node(finished, &mut stack, &mut roots);
    }
    roots
}

fn attach_pg_node(
    node: ExplainNode,
    stack: &mut [(usize, ExplainNode)],
    roots: &mut Vec<ExplainNode>,
) {
    if let Some((_, parent)) = stack.last_mut() {
        parent.children.push(node);
    } else {
        roots.push(node);
    }
}

fn parse_pg_text_line(line: &str, id: usize) -> ExplainNode {
    let (head, metrics) = match line.find('(') {
        Some(pos) => (line[..pos].trim(), &line[pos..]),
        None => (line.trim(), ""),
    };
    let (operation, relation) = if let Some(idx) = head.find(" on ") {
        let op = head[..idx].trim().to_owned();
        let rel = head[idx + 4..].split_whitespace().next().map(str::to_owned);
        (op, rel)
    } else {
        (head.to_owned(), None)
    };
    let cost = extract_metric_end_f64(metrics, "cost=");
    let estimated_rows = extract_metric_f64(metrics, "rows=");
    let actual_time_ms = extract_actual_metric_end_f64(metrics, "actual time=");
    let actual_rows = extract_actual_metric_u64(metrics, "rows=");
    let is_seq_scan_warning = operation.eq_ignore_ascii_case("Seq Scan");

    ExplainNode {
        id,
        label: head.to_owned(),
        detail: metrics.trim().to_owned(),
        operation,
        relation,
        cost,
        estimated_rows,
        actual_time_ms,
        actual_rows,
        is_seq_scan_warning,
        children: Vec::new(),
    }
}

fn extract_metric_end_f64(metrics: &str, key: &str) -> Option<f64> {
    let pos = metrics.find(key)?;
    let rest = &metrics[pos + key.len()..];
    let token = rest.split([' ', ')']).next()?;
    let end = token.split("..").last()?;
    end.parse::<f64>().ok()
}

fn extract_metric_f64(metrics: &str, key: &str) -> Option<f64> {
    let pos = metrics.find(key)?;
    let rest = &metrics[pos + key.len()..];
    rest.split([' ', ')']).next()?.parse::<f64>().ok()
}

fn extract_actual_metric_end_f64(metrics: &str, key: &str) -> Option<f64> {
    let actual_pos = metrics.find("(actual ")?;
    extract_metric_end_f64(&metrics[actual_pos..], key)
}

fn extract_actual_metric_u64(metrics: &str, key: &str) -> Option<u64> {
    let actual_pos = metrics.find("(actual ")?;
    let pos = metrics[actual_pos..].find(key)?;
    let rest = &metrics[actual_pos + pos + key.len()..];
    rest.split([' ', ')']).next()?.parse::<u64>().ok()
}

fn parse_generic_explain_rows(columns: &[String], rows: &[Vec<String>]) -> Vec<ExplainNode> {
    rows.iter()
        .enumerate()
        .map(|(idx, row)| {
            let text = if row.len() == 1 {
                row[0].trim().to_owned()
            } else {
                columns
                    .iter()
                    .zip(row.iter())
                    .map(|(c, v)| format!("{c}={v}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let upper = text.to_ascii_uppercase();
            let is_seq_scan_warning = upper.contains("SEQ_SCAN")
                || upper.contains("SEQ SCAN")
                || upper.contains("TABLE SCAN");
            let op = text.split_whitespace().next().unwrap_or("STEP").to_owned();
            ExplainNode {
                id: idx + 1,
                label: text.clone(),
                detail: text,
                operation: op,
                relation: None,
                cost: None,
                estimated_rows: None,
                actual_time_ms: None,
                actual_rows: None,
                is_seq_scan_warning,
                children: Vec::new(),
            }
        })
        .collect()
}

/// Join type for the visual query builder (Feature 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JoinKind {
    Inner,
    Left,
    Right,
    Full,
    Cross,
}

impl JoinKind {
    /// SQL keyword representation of the join type.
    #[must_use]
    pub const fn as_sql(self) -> &'static str {
        match self {
            Self::Inner => "INNER JOIN",
            Self::Left => "LEFT JOIN",
            Self::Right => "RIGHT JOIN",
            Self::Full => "FULL JOIN",
            Self::Cross => "CROSS JOIN",
        }
    }
}

/// A `JOIN` clause in [`VisualQuerySpec`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinClause {
    pub kind: JoinKind,
    pub table: String,
    pub alias: Option<String>,
    pub left_col: String,
    pub right_col: String,
}

/// A `WHERE` predicate in [`VisualQuerySpec`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhereClause {
    pub column: String,
    pub operator: String,
    pub value: String,
    pub logical_op: String,
}

/// Declarative model for the Visual Query Builder SQL generator (Feature 8).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct VisualQuerySpec {
    pub table: String,
    pub schema: Option<String>,
    pub selected_columns: Vec<String>,
    pub joins: Vec<JoinClause>,
    pub filters: Vec<WhereClause>,
    pub group_by: Vec<String>,
    pub order_by: Vec<(String, bool)>,
    pub limit: Option<usize>,
}

/// Generates dialect-aware `SELECT` SQL from a [`VisualQuerySpec`].
#[must_use]
pub fn build_visual_query_sql(engine: DatabaseEngine, spec: &VisualQuerySpec) -> String {
    let cols_sql = if spec.selected_columns.is_empty() {
        "*".to_owned()
    } else {
        spec.selected_columns
            .iter()
            .map(|c| quote_qualified_col(engine, c))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let top_prefix = if engine == DatabaseEngine::Mssql {
        spec.limit.map_or_else(String::new, |n| format!("TOP {n} "))
    } else {
        String::new()
    };

    let base_table = engine.qualify_table(spec.schema.as_deref(), &spec.table);
    let mut lines = vec![
        format!("SELECT {top_prefix}{cols_sql}"),
        format!("FROM {base_table}"),
    ];

    for join in &spec.joins {
        lines.push(format_join_clause(engine, spec.schema.as_deref(), join));
    }

    if !spec.filters.is_empty() {
        lines.push(format_where_clauses(engine, &spec.filters));
    }

    if !spec.group_by.is_empty() {
        let grp = spec
            .group_by
            .iter()
            .map(|c| quote_qualified_col(engine, c))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("GROUP BY {grp}"));
    }

    if !spec.order_by.is_empty() {
        let ord = spec
            .order_by
            .iter()
            .map(|(col, asc)| {
                let dir = if *asc { "ASC" } else { "DESC" };
                format!("{} {dir}", quote_qualified_col(engine, col))
            })
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("ORDER BY {ord}"));
    }

    if engine != DatabaseEngine::Mssql
        && let Some(limit) = spec.limit
    {
        lines.push(format!("LIMIT {limit}"));
    }

    format!("{};", lines.join("\n"))
}

fn quote_qualified_col(engine: DatabaseEngine, expr: &str) -> String {
    let trimmed = expr.trim();
    if trimmed == "*" || trimmed.contains('(') || trimmed.contains(' ') {
        return trimmed.to_owned();
    }
    if let Some((tbl, col)) = trimmed.split_once('.') {
        if col == "*" {
            return format!("{}.*", engine.quote_identifier(tbl));
        }
        return format!(
            "{}.{}",
            engine.quote_identifier(tbl),
            engine.quote_identifier(col)
        );
    }
    engine.quote_identifier(trimmed)
}

fn format_join_clause(engine: DatabaseEngine, schema: Option<&str>, join: &JoinClause) -> String {
    let target = engine.qualify_table(schema, &join.table);
    let alias_part = join
        .alias
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map_or_else(String::new, |a| {
            format!(" AS {}", engine.quote_identifier(a))
        });
    if join.kind == JoinKind::Cross {
        return format!("{} {target}{alias_part}", join.kind.as_sql());
    }
    format!(
        "{} {target}{alias_part} ON {} = {}",
        join.kind.as_sql(),
        quote_qualified_col(engine, &join.left_col),
        quote_qualified_col(engine, &join.right_col),
    )
}

fn format_where_clauses(engine: DatabaseEngine, filters: &[WhereClause]) -> String {
    let mut sql = String::from("WHERE ");
    for (idx, f) in filters.iter().enumerate() {
        if idx > 0 {
            let logic = match f.logical_op.trim().to_ascii_uppercase().as_str() {
                "OR" => "OR",
                _ => "AND",
            };
            sql.push_str(&format!("\n  {logic} "));
        }
        let col = quote_qualified_col(engine, &f.column);
        let op_upper = f.operator.trim().to_ascii_uppercase();
        match op_upper.as_str() {
            "IS NULL" | "IS NOT NULL" => sql.push_str(&format!("{col} {op_upper}")),
            "IN" | "NOT IN" => sql.push_str(&format!("{col} {op_upper} ({})", f.value.trim())),
            _ => {
                let val_sql = format_filter_value(engine, &f.value);
                sql.push_str(&format!("{col} {op_upper} {val_sql}"));
            }
        }
    }
    sql
}

fn format_filter_value(engine: DatabaseEngine, raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("NULL")
        || trimmed.eq_ignore_ascii_case("TRUE")
        || trimmed.eq_ignore_ascii_case("FALSE")
        || trimmed.parse::<i64>().is_ok()
        || trimmed.parse::<f64>().is_ok_and(f64::is_finite)
    {
        return trimmed.to_owned();
    }
    if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2 {
        return trimmed.to_owned();
    }
    engine.quote_literal(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_postgres_json_and_text_explain() {
        let json_plan = r#"[{
            "Plan": {
                "Node Type": "Hash Join",
                "Total Cost": 42.5,
                "Plan Rows": 120,
                "Actual Total Time": 1.25,
                "Actual Rows": 100,
                "Plans": [
                    {
                        "Node Type": "Seq Scan",
                        "Relation Name": "orders",
                        "Total Cost": 20.0,
                        "Plan Rows": 500
                    }
                ]
            }
        }]"#;
        let nodes = parse_explain_output(
            DatabaseEngine::Postgres,
            &["QUERY PLAN".to_owned()],
            &[vec![json_plan.to_owned()]],
        );
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].operation, "Hash Join");
        assert_eq!(nodes[0].children.len(), 1);
        assert!(nodes[0].children[0].is_seq_scan_warning);
        assert_eq!(nodes[0].children[0].relation.as_deref(), Some("orders"));

        let text_rows = vec![
            vec!["Hash Join  (cost=1.50..42.50 rows=100 width=16) (actual time=0.20..1.25 rows=100 loops=1)".to_owned()],
            vec!["  ->  Seq Scan on users  (cost=0.00..15.50 rows=550 width=8)".to_owned()],
        ];
        let text_nodes = parse_explain_output(
            DatabaseEngine::Postgres,
            &["QUERY PLAN".to_owned()],
            &text_rows,
        );
        assert_eq!(text_nodes.len(), 1);
        assert_eq!(text_nodes[0].cost, Some(42.50));
        assert_eq!(text_nodes[0].actual_rows, Some(100));
        assert_eq!(text_nodes[0].children.len(), 1);
        assert!(text_nodes[0].children[0].is_seq_scan_warning);
    }

    #[test]
    fn parses_sqlite_explain_query_plan_tree() {
        let cols = vec![
            "id".to_owned(),
            "parent".to_owned(),
            "notused".to_owned(),
            "detail".to_owned(),
        ];
        let rows = vec![
            vec![
                "2".to_owned(),
                "0".to_owned(),
                "0".to_owned(),
                "SCAN orders".to_owned(),
            ],
            vec![
                "4".to_owned(),
                "2".to_owned(),
                "0".to_owned(),
                "SEARCH users USING INTEGER PRIMARY KEY (rowid=?)".to_owned(),
            ],
        ];
        let nodes = parse_explain_output(DatabaseEngine::Sqlite, &cols, &rows);
        assert_eq!(nodes.len(), 1);
        assert!(nodes[0].is_seq_scan_warning);
        assert_eq!(nodes[0].relation.as_deref(), Some("orders"));
        assert_eq!(nodes[0].children.len(), 1);
        assert!(!nodes[0].children[0].is_seq_scan_warning);
        assert_eq!(nodes[0].children[0].operation, "SEARCH");
    }

    #[test]
    fn parses_mysql_tabular_explain() {
        let cols = vec![
            "id".to_owned(),
            "select_type".to_owned(),
            "table".to_owned(),
            "type".to_owned(),
            "key".to_owned(),
            "rows".to_owned(),
            "Extra".to_owned(),
        ];
        let rows = vec![vec![
            "1".to_owned(),
            "SIMPLE".to_owned(),
            "users".to_owned(),
            "ALL".to_owned(),
            "NULL".to_owned(),
            "1000".to_owned(),
            "Using where".to_owned(),
        ]];
        let nodes = parse_explain_output(DatabaseEngine::Mysql, &cols, &rows);
        assert_eq!(nodes.len(), 1);
        assert!(nodes[0].is_seq_scan_warning);
        assert_eq!(nodes[0].estimated_rows, Some(1000.0));
    }

    #[test]
    fn build_visual_query_sql_generates_joins_filters_and_limits() {
        let spec = VisualQuerySpec {
            table: "users".to_owned(),
            schema: Some("public".to_owned()),
            selected_columns: vec!["users.id".to_owned(), "users.name".to_owned()],
            joins: vec![JoinClause {
                kind: JoinKind::Left,
                table: "orders".to_owned(),
                alias: Some("o".to_owned()),
                left_col: "users.id".to_owned(),
                right_col: "o.user_id".to_owned(),
            }],
            filters: vec![
                WhereClause {
                    column: "users.active".to_owned(),
                    operator: "=".to_owned(),
                    value: "true".to_owned(),
                    logical_op: "AND".to_owned(),
                },
                WhereClause {
                    column: "users.role".to_owned(),
                    operator: "=".to_owned(),
                    value: "admin".to_owned(),
                    logical_op: "AND".to_owned(),
                },
            ],
            group_by: vec!["users.id".to_owned(), "users.name".to_owned()],
            order_by: vec![("users.id".to_owned(), false)],
            limit: Some(50),
        };
        let pg_sql = build_visual_query_sql(DatabaseEngine::Postgres, &spec);
        assert!(pg_sql.contains("SELECT \"users\".\"id\", \"users\".\"name\""));
        assert!(pg_sql.contains("FROM \"public\".\"users\""));
        assert!(pg_sql.contains(
            "LEFT JOIN \"public\".\"orders\" AS \"o\" ON \"users\".\"id\" = \"o\".\"user_id\""
        ));
        assert!(
            pg_sql
                .contains("WHERE \"users\".\"active\" = true\n  AND \"users\".\"role\" = 'admin'")
        );
        assert!(pg_sql.contains("LIMIT 50;"));

        let mssql_sql = build_visual_query_sql(DatabaseEngine::Mssql, &spec);
        assert!(mssql_sql.contains("SELECT TOP 50 [users].[id], [users].[name]"));
    }
}
