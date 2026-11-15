//! Schema and table data comparison with reviewable sync SQL (Feature 10).

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use tablepro_core::QueryResult;
use tablepro_core::column::ColumnDefault;
use tablepro_core::ddl::{ColumnDefinition, TableDefinition};
use tablepro_core::value::{Temporal, Value, ValueIdentity, same_value};

use crate::ddl_builder::{
    AlterTableOp, ColumnSpec, DatabaseEngine, ForeignKeySpec, IndexSpec, build_alter_table_sql,
};

/// Individual column attribute change detected between source and target definitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnChangeKind {
    TypeChanged {
        source_type: String,
        target_type: String,
    },
    NullabilityChanged {
        source_nullable: bool,
        target_nullable: bool,
    },
    DefaultChanged {
        source_default: Option<String>,
        target_default: Option<String>,
    },
}

/// Column-level difference between two [`TableDefinition`]s.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnDiff {
    pub column_name: String,
    pub changes: Vec<ColumnChangeKind>,
}

/// Result of comparing two [`TableDefinition`]s (`source` = reference, `target` = destination to sync).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SchemaDiffReport {
    pub table_name: String,
    pub added_columns: Vec<String>,
    pub removed_columns: Vec<String>,
    pub modified_columns: Vec<ColumnDiff>,
    pub added_indexes: Vec<String>,
    pub removed_indexes: Vec<String>,
    pub added_foreign_keys: Vec<String>,
    pub removed_foreign_keys: Vec<String>,
    pub is_identical: bool,
    pub sync_ddl: Vec<String>,
}

/// Difference category for a single row in [`DataDiffReport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RowDiffKind {
    OnlyInSource,
    OnlyInTarget,
    Modified,
}

/// Per-row diff summary for data compare.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowDiffEntry {
    pub kind: RowDiffKind,
    pub primary_key: Vec<String>,
    pub source_values: Vec<String>,
    pub target_values: Vec<String>,
    pub changed_columns: Vec<String>,
}

/// Result of comparing two [`QueryResult`] datasets for the same table.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DataDiffReport {
    pub only_in_source: Vec<RowDiffEntry>,
    pub only_in_target: Vec<RowDiffEntry>,
    pub modified: Vec<RowDiffEntry>,
    pub matching_count: usize,
    pub sync_sql: Vec<String>,
    pub warnings: Vec<String>,
}

/// Compares `source` (reference) against `target` (destination) and produces reviewable sync DDL.
#[must_use]
pub fn diff_table_schemas(
    engine: DatabaseEngine,
    source: &TableDefinition,
    target: &TableDefinition,
) -> SchemaDiffReport {
    let mut ops = Vec::new();
    let mut added_columns = Vec::new();
    let mut removed_columns = Vec::new();
    let mut modified_columns = Vec::new();

    for src_col in &source.columns {
        match target.column(src_col.name()) {
            None => {
                added_columns.push(src_col.name().to_owned());
                ops.push(AlterTableOp::AddColumn(column_def_to_spec(src_col)));
            }
            Some(tgt_col) => {
                if let Some(diff) = compare_column_defs(src_col, tgt_col) {
                    modified_columns.push(diff);
                    ops.push(AlterTableOp::ModifyColumn {
                        column: column_def_to_spec(src_col),
                    });
                }
            }
        }
    }

    for tgt_col in &target.columns {
        if source.column(tgt_col.name()).is_none() {
            removed_columns.push(tgt_col.name().to_owned());
            ops.push(AlterTableOp::DropColumn(tgt_col.name().to_owned()));
        }
    }

    let (added_indexes, removed_indexes) = diff_indexes(source, target, &mut ops);
    let (added_foreign_keys, removed_foreign_keys) = diff_foreign_keys(source, target, &mut ops);

    let is_identical = added_columns.is_empty()
        && removed_columns.is_empty()
        && modified_columns.is_empty()
        && added_indexes.is_empty()
        && removed_indexes.is_empty()
        && added_foreign_keys.is_empty()
        && removed_foreign_keys.is_empty();

    let sync_ddl = if ops.is_empty() {
        Vec::new()
    } else {
        build_alter_table_sql(
            engine,
            target.table.schema.as_deref(),
            &target.table.name,
            &ops,
        )
    };

    SchemaDiffReport {
        table_name: target.table.name.clone(),
        added_columns,
        removed_columns,
        modified_columns,
        added_indexes,
        removed_indexes,
        added_foreign_keys,
        removed_foreign_keys,
        is_identical,
        sync_ddl,
    }
}

fn diff_indexes(
    source: &TableDefinition,
    target: &TableDefinition,
    ops: &mut Vec<AlterTableOp>,
) -> (Vec<String>, Vec<String>) {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let src_secondary: Vec<_> = source.indexes.iter().filter(|i| !i.primary).collect();
    let tgt_secondary: Vec<_> = target.indexes.iter().filter(|i| !i.primary).collect();

    for idx in &src_secondary {
        let same_in_tgt = tgt_secondary.iter().any(|t| {
            t.name == idx.name && t.unique == idx.unique && t.key_columns() == idx.key_columns()
        });
        if !same_in_tgt {
            if tgt_secondary.iter().any(|t| t.name == idx.name) {
                removed.push(idx.name.clone());
                ops.push(AlterTableOp::DropIndex(idx.name.clone()));
            }
            added.push(idx.name.clone());
            ops.push(AlterTableOp::AddIndex(IndexSpec {
                name: idx.name.clone(),
                columns: idx.key_columns(),
                unique: idx.unique,
            }));
        }
    }

    for idx in &tgt_secondary {
        if !src_secondary.iter().any(|s| s.name == idx.name) {
            removed.push(idx.name.clone());
            ops.push(AlterTableOp::DropIndex(idx.name.clone()));
        }
    }
    (added, removed)
}

fn diff_foreign_keys(
    source: &TableDefinition,
    target: &TableDefinition,
    ops: &mut Vec<AlterTableOp>,
) -> (Vec<String>, Vec<String>) {
    let mut added = Vec::new();
    let mut removed = Vec::new();

    for fk in &source.foreign_keys {
        if !target.foreign_keys.contains(fk) {
            if target.foreign_keys.iter().any(|t| t.name == fk.name) {
                removed.push(fk.name.clone());
                ops.push(AlterTableOp::DropForeignKey(fk.name.clone()));
            }
            added.push(fk.name.clone());
            if let (Some(col), Some(ref_col)) = (fk.columns.first(), fk.ref_columns.first()) {
                ops.push(AlterTableOp::AddForeignKey(ForeignKeySpec {
                    name: fk.name.clone(),
                    column: col.clone(),
                    ref_table: fk.ref_table.clone(),
                    ref_column: ref_col.clone(),
                    on_delete: fk.on_delete.map(|a| a.as_sql().to_owned()),
                    on_update: fk.on_update.map(|a| a.as_sql().to_owned()),
                }));
            }
        }
    }

    for fk in &target.foreign_keys {
        if !source.foreign_keys.iter().any(|s| s.name == fk.name) {
            removed.push(fk.name.clone());
            ops.push(AlterTableOp::DropForeignKey(fk.name.clone()));
        }
    }
    (added, removed)
}

fn compare_column_defs(src: &ColumnDefinition, tgt: &ColumnDefinition) -> Option<ColumnDiff> {
    let mut changes = Vec::new();
    let src_ty = column_type_text(src);
    let tgt_ty = column_type_text(tgt);
    if !src_ty.eq_ignore_ascii_case(&tgt_ty) {
        changes.push(ColumnChangeKind::TypeChanged {
            source_type: src_ty,
            target_type: tgt_ty,
        });
    }
    if src.info.nullable != tgt.info.nullable {
        changes.push(ColumnChangeKind::NullabilityChanged {
            source_nullable: src.info.nullable,
            target_nullable: tgt.info.nullable,
        });
    }
    let src_def = column_default_text(&src.info.default);
    let tgt_def = column_default_text(&tgt.info.default);
    if src_def != tgt_def {
        changes.push(ColumnChangeKind::DefaultChanged {
            source_default: src_def,
            target_default: tgt_def,
        });
    }
    (!changes.is_empty()).then(|| ColumnDiff {
        column_name: src.name().to_owned(),
        changes,
    })
}

fn column_type_text(col: &ColumnDefinition) -> String {
    col.info.column_type.catalog.as_ref().map_or_else(
        || col.info.column_type.name.clone(),
        |c| c.declaration.as_sql().to_owned(),
    )
}

fn column_default_text(default: &ColumnDefault) -> Option<String> {
    match default {
        ColumnDefault::None => None,
        ColumnDefault::Literal(v) => Some(value_to_display(v)),
        ColumnDefault::Expression(expr) => Some(expr.as_sql().to_owned()),
    }
}

fn column_def_to_spec(col: &ColumnDefinition) -> ColumnSpec {
    ColumnSpec {
        name: col.info.name.clone(),
        data_type: column_type_text(col),
        nullable: col.info.nullable,
        default_expr: column_default_text(&col.info.default),
        is_primary_key: col.info.primary_key,
        is_unique: false,
        auto_increment: col.info.is_auto_increment,
    }
}

/// Compares `source_rows` (reference) against `target_rows` (destination) keyed by `pk_cols`,
/// producing row differences and reviewable `INSERT` / `UPDATE` / `DELETE` sync SQL.
#[must_use]
pub fn diff_table_rows(
    engine: DatabaseEngine,
    schema: Option<&str>,
    table: &str,
    pk_cols: &[String],
    source_rows: &QueryResult,
    target_rows: &QueryResult,
) -> DataDiffReport {
    let mut warnings = Vec::new();
    let Some((src_pk_idx, tgt_pk_idx)) =
        resolve_pk_indices(pk_cols, source_rows, target_rows, &mut warnings)
    else {
        return DataDiffReport {
            warnings,
            ..DataDiffReport::default()
        };
    };

    let mut target_map: HashMap<Vec<ValueIdentity>, usize> =
        HashMap::with_capacity(target_rows.rows.len());
    for (idx, row) in target_rows.rows.iter().enumerate() {
        let key = build_row_key(row, &tgt_pk_idx);
        target_map.insert(key, idx);
    }

    let qualified = engine.qualify_table(schema, table);
    let mut only_in_source = Vec::new();
    let mut modified = Vec::new();
    let mut sync_sql = Vec::new();
    let mut matched_target_indices = HashSet::new();
    let mut matching_count = 0_usize;

    let tgt_col_map: HashMap<&str, usize> = target_rows
        .columns
        .iter()
        .enumerate()
        .map(|(i, c)| (c.name.as_str(), i))
        .collect();

    for src_row in &source_rows.rows {
        let key = build_row_key(src_row, &src_pk_idx);
        let pk_display: Vec<String> = key.iter().map(|v| value_to_display(v.value())).collect();
        if let Some(&tgt_idx) = target_map.get(&key) {
            matched_target_indices.insert(tgt_idx);
            let tgt_row = &target_rows.rows[tgt_idx];
            let changed = collect_changed_columns(source_rows, src_row, tgt_row, &tgt_col_map);
            if changed.is_empty() {
                matching_count += 1;
            } else {
                let changed_names: Vec<String> =
                    changed.iter().map(|(n, _)| (*n).to_owned()).collect();
                let sql = build_sync_update(engine, &qualified, pk_cols, &key, &changed);
                sync_sql.push(sql);
                modified.push(RowDiffEntry {
                    kind: RowDiffKind::Modified,
                    primary_key: pk_display,
                    source_values: src_row.iter().map(value_to_display).collect(),
                    target_values: tgt_row.iter().map(value_to_display).collect(),
                    changed_columns: changed_names,
                });
            }
        } else {
            sync_sql.push(build_sync_insert(engine, &qualified, source_rows, src_row));
            only_in_source.push(RowDiffEntry {
                kind: RowDiffKind::OnlyInSource,
                primary_key: pk_display,
                source_values: src_row.iter().map(value_to_display).collect(),
                target_values: Vec::new(),
                changed_columns: Vec::new(),
            });
        }
    }

    let mut only_in_target = Vec::new();
    for (idx, tgt_row) in target_rows.rows.iter().enumerate() {
        if matched_target_indices.contains(&idx) {
            continue;
        }
        let key = build_row_key(tgt_row, &tgt_pk_idx);
        let pk_display: Vec<String> = key.iter().map(|v| value_to_display(v.value())).collect();
        sync_sql.push(build_sync_delete(engine, &qualified, pk_cols, &key));
        only_in_target.push(RowDiffEntry {
            kind: RowDiffKind::OnlyInTarget,
            primary_key: pk_display,
            source_values: Vec::new(),
            target_values: tgt_row.iter().map(value_to_display).collect(),
            changed_columns: Vec::new(),
        });
    }

    DataDiffReport {
        only_in_source,
        only_in_target,
        modified,
        matching_count,
        sync_sql,
        warnings,
    }
}

fn resolve_pk_indices(
    pk_cols: &[String],
    source: &QueryResult,
    target: &QueryResult,
    warnings: &mut Vec<String>,
) -> Option<(Vec<usize>, Vec<usize>)> {
    if pk_cols.is_empty() {
        warnings.push("Data compare requires at least one primary key column.".to_owned());
        return None;
    }
    let mut src_idx = Vec::with_capacity(pk_cols.len());
    let mut tgt_idx = Vec::with_capacity(pk_cols.len());
    for pk in pk_cols {
        let Some(s) = source.columns.iter().position(|c| c.name == *pk) else {
            warnings.push(format!(
                "Primary key column '{pk}' missing in source result set."
            ));
            return None;
        };
        let Some(t) = target.columns.iter().position(|c| c.name == *pk) else {
            warnings.push(format!(
                "Primary key column '{pk}' missing in target result set."
            ));
            return None;
        };
        src_idx.push(s);
        tgt_idx.push(t);
    }
    Some((src_idx, tgt_idx))
}

fn build_row_key(row: &[Value], indices: &[usize]) -> Vec<ValueIdentity> {
    indices
        .iter()
        .map(|&i| ValueIdentity::new(row.get(i).cloned().unwrap_or(Value::Null)))
        .collect()
}

fn collect_changed_columns<'a>(
    source_rows: &'a QueryResult,
    src_row: &'a [Value],
    tgt_row: &[Value],
    tgt_col_map: &HashMap<&str, usize>,
) -> Vec<(&'a str, &'a Value)> {
    let mut changed = Vec::new();
    for (i, col) in source_rows.columns.iter().enumerate() {
        let src_val = src_row.get(i).unwrap_or(&Value::Null);
        if let Some(&t_idx) = tgt_col_map.get(col.name.as_str()) {
            let tgt_val = tgt_row.get(t_idx).unwrap_or(&Value::Null);
            if !same_value(src_val, tgt_val) {
                changed.push((col.name.as_str(), src_val));
            }
        }
    }
    changed
}

fn build_sync_insert(
    engine: DatabaseEngine,
    qualified_table: &str,
    source_rows: &QueryResult,
    row: &[Value],
) -> String {
    let cols = source_rows
        .columns
        .iter()
        .map(|c| engine.quote_identifier(&c.name))
        .collect::<Vec<_>>()
        .join(", ");
    let vals = (0..source_rows.columns.len())
        .map(|i| value_to_sql_literal(engine, row.get(i).unwrap_or(&Value::Null)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("INSERT INTO {qualified_table} ({cols}) VALUES ({vals});")
}

fn build_sync_update(
    engine: DatabaseEngine,
    qualified_table: &str,
    pk_cols: &[String],
    pk_vals: &[ValueIdentity],
    changed: &[(&str, &Value)],
) -> String {
    let set_sql = changed
        .iter()
        .map(|(col, val)| {
            format!(
                "{} = {}",
                engine.quote_identifier(col),
                value_to_sql_literal(engine, val)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let where_sql = build_pk_where(engine, pk_cols, pk_vals);
    format!("UPDATE {qualified_table} SET {set_sql} WHERE {where_sql};")
}

fn build_sync_delete(
    engine: DatabaseEngine,
    qualified_table: &str,
    pk_cols: &[String],
    pk_vals: &[ValueIdentity],
) -> String {
    let where_sql = build_pk_where(engine, pk_cols, pk_vals);
    format!("DELETE FROM {qualified_table} WHERE {where_sql};")
}

fn build_pk_where(engine: DatabaseEngine, pk_cols: &[String], pk_vals: &[ValueIdentity]) -> String {
    pk_cols
        .iter()
        .zip(pk_vals.iter())
        .map(|(col, val)| {
            let qcol = engine.quote_identifier(col);
            if val.value().is_null() {
                format!("{qcol} IS NULL")
            } else {
                format!("{qcol} = {}", value_to_sql_literal(engine, val.value()))
            }
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn value_to_sql_literal(engine: DatabaseEngine, val: &Value) -> String {
    match val {
        Value::Null => "NULL".to_owned(),
        Value::Bool(b) => match engine {
            DatabaseEngine::Sqlite | DatabaseEngine::Mssql => if *b { "1" } else { "0" }.to_owned(),
            _ => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        },
        Value::Int32(n) => n.to_string(),
        Value::Int(n) => n.to_string(),
        Value::UInt(n) => n.to_string(),
        Value::WideInt(n) => n.to_string(),
        Value::Float32(f) if f.is_finite() => f.to_string(),
        Value::Float64(f) if f.is_finite() => f.to_string(),
        Value::Decimal(d) => d.to_string(),
        Value::Bytes(bytes) => {
            let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
            match engine {
                DatabaseEngine::Postgres => format!("'\\x{hex}'"),
                DatabaseEngine::Mssql => format!("0x{hex}"),
                _ => format!("X'{hex}'"),
            }
        }
        _ => engine.quote_literal(&value_to_display(val)),
    }
}

fn value_to_display(val: &Value) -> String {
    match val {
        Value::Null => "NULL".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Int32(n) => n.to_string(),
        Value::Int(n) => n.to_string(),
        Value::UInt(n) => n.to_string(),
        Value::WideInt(n) => n.to_string(),
        Value::Float32(f) => f.to_string(),
        Value::Float64(f) => f.to_string(),
        Value::Decimal(d) => d.to_string(),
        Value::Text(s) => s.clone(),
        Value::Bytes(b) => format!("{} bytes", b.len()),
        Value::Uuid(u) => u.to_string(),
        Value::Json(j) => j.as_str().to_owned(),
        Value::Date(Temporal::Finite(d)) => d.format("%Y-%m-%d").to_string(),
        Value::Time(t) => t.format(None),
        Value::TimeTz(t) => format!("{}{}", t.time.format(None), t.offset),
        Value::Timestamp(Temporal::Finite(ts)) => ts.format("%Y-%m-%d %H:%M:%S").to_string(),
        Value::TimestampTz(Temporal::Finite(ts)) => ts.to_string(),
        Value::Date(Temporal::Infinity)
        | Value::Timestamp(Temporal::Infinity)
        | Value::TimestampTz(Temporal::Infinity) => "infinity".to_owned(),
        Value::Date(Temporal::NegInfinity)
        | Value::Timestamp(Temporal::NegInfinity)
        | Value::TimestampTz(Temporal::NegInfinity) => "-infinity".to_owned(),
        Value::Interval(i) => i.to_string(),
        Value::Bits(b) => b.to_string(),
        Value::Array(arr) => format!(
            "[{}]",
            arr.iter()
                .map(value_to_display)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Other(o) => o.text.clone(),
        Value::Undecodable(u) => format!("<undecodable:{}>", u.type_name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tablepro_core::column::{
        CatalogType, ColumnInfo, ColumnKind, ColumnType, ReadForm, ResultColumn, SqlTypeExpr,
    };
    use tablepro_core::ddl::ColumnAttributes;
    use tablepro_core::meta::TableRef;

    fn make_col(name: &str, sql_type: &str, nullable: bool, pk: bool) -> ColumnDefinition {
        ColumnDefinition::new(
            ColumnInfo {
                name: name.to_owned(),
                column_type: ColumnType {
                    name: sql_type.to_owned(),
                    kind: ColumnKind::Other,
                    catalog: Some(CatalogType {
                        declaration: SqlTypeExpr::from_catalog_text(sql_type),
                        cast_target: SqlTypeExpr::from_catalog_text(sql_type),
                    }),
                    dynamic_storage: false,
                    read_form: ReadForm::Native,
                },
                nullable,
                primary_key: pk,
                is_auto_increment: false,
                is_generated: false,
                partition_scoped: false,
                default: ColumnDefault::None,
                comment: None,
            },
            ColumnAttributes::default(),
        )
    }

    fn make_table(name: &str, columns: Vec<ColumnDefinition>) -> TableDefinition {
        TableDefinition {
            table: TableRef::new(Some("public".to_owned()), name),
            columns,
            primary_key: None,
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            inbound_foreign_keys: Vec::new(),
            source: None,
            options: None,
        }
    }

    #[test]
    fn diff_table_schemas_detects_added_removed_and_modified_columns() {
        let src = make_table(
            "users",
            vec![
                make_col("id", "INTEGER", false, true),
                make_col("email", "VARCHAR(255)", false, false),
                make_col("bio", "TEXT", true, false),
            ],
        );
        let tgt = make_table(
            "users",
            vec![
                make_col("id", "INTEGER", false, true),
                make_col("email", "VARCHAR(120)", true, false),
                make_col("legacy_code", "TEXT", true, false),
            ],
        );

        let report = diff_table_schemas(DatabaseEngine::Postgres, &src, &tgt);
        assert!(!report.is_identical);
        assert_eq!(report.added_columns, vec!["bio"]);
        assert_eq!(report.removed_columns, vec!["legacy_code"]);
        assert_eq!(report.modified_columns.len(), 1);
        assert_eq!(report.modified_columns[0].column_name, "email");
        assert!(
            report
                .sync_ddl
                .iter()
                .any(|s| s.contains("ADD COLUMN \"bio\""))
        );
        assert!(
            report
                .sync_ddl
                .iter()
                .any(|s| s.contains("DROP COLUMN \"legacy_code\""))
        );
        assert!(
            report
                .sync_ddl
                .iter()
                .any(|s| s.contains("ALTER COLUMN \"email\" TYPE VARCHAR(255)"))
        );
    }

    #[test]
    fn diff_table_rows_detects_inserts_updates_deletes_and_matches() {
        let cols = vec![
            ResultColumn::new("id", ColumnType::untyped_text("INTEGER")),
            ResultColumn::new("name", ColumnType::untyped_text("TEXT")),
        ];
        let src = QueryResult::new(
            cols.clone(),
            vec![
                vec![Value::Int(1), Value::Text("Alice".to_owned())],
                vec![Value::Int(2), Value::Text("Bob Updated".to_owned())],
                vec![Value::Int(3), Value::Text("Carol".to_owned())],
            ],
        );
        let tgt = QueryResult::new(
            cols,
            vec![
                vec![Value::Int(1), Value::Text("Alice".to_owned())],
                vec![Value::Int(2), Value::Text("Bob".to_owned())],
                vec![Value::Int(4), Value::Text("Dave".to_owned())],
            ],
        );

        let report = diff_table_rows(
            DatabaseEngine::Sqlite,
            None,
            "users",
            &["id".to_owned()],
            &src,
            &tgt,
        );
        assert_eq!(report.matching_count, 1);
        assert_eq!(report.only_in_source.len(), 1);
        assert_eq!(report.only_in_target.len(), 1);
        assert_eq!(report.modified.len(), 1);
        assert!(
            report
                .sync_sql
                .iter()
                .any(|s| s.starts_with("INSERT INTO \"users\""))
        );
        assert!(report.sync_sql.iter().any(|s| {
            s.starts_with("UPDATE \"users\" SET \"name\" = 'Bob Updated' WHERE \"id\" = 2;")
        }));
        assert!(
            report
                .sync_sql
                .iter()
                .any(|s| s == "DELETE FROM \"users\" WHERE \"id\" = 4;")
        );
    }
}
