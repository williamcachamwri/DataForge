//! Pure-Rust cross-engine data transfer and schema migration service.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tablepro_core::browse::{PageRequest, read_page};
use tablepro_core::column::{ColumnDefault, ColumnInfo};
use tablepro_core::ddl::{ColumnAttributes, ColumnDefinition, TableDefinition};
use tablepro_core::filter::FilterSet;
use tablepro_core::meta::{PagePosition, RowIdentity, TableRef};
use tablepro_core::value::{Temporal, Value};
use tablepro_core::{
    DriverError, EditorSession, EditorSessionOptions, ResultEvent, ResultSink, RunEnd, RunOptions,
    SinkFlow, UserSql,
};
use tablepro_session::Session;
use tablepro_session::operation::{OperationError, OperationKind};
use tokio_util::sync::CancellationToken;

use crate::ddl_builder::{
    AlterTableOp, ColumnSpec, CreateTableSpec, DatabaseEngine, build_alter_table_sql,
    build_create_table_sql,
};
use crate::error::ServiceError;

const DEFAULT_BATCH_SIZE: usize = 500;
const MSSQL_MAX_VALUES_ROWS: usize = 1000;

/// Behavior when preparing and loading the target table during a transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TransferMode {
    /// Create the target table if it does not exist, then insert all source rows.
    #[default]
    CreateAndLoad,
    /// Insert source rows into an existing target table without modifying existing rows.
    Append,
    /// Truncate (or delete all rows from) the existing target table, then insert source rows.
    TruncateAndLoad,
}

/// Optional per-column mapping override for name, target SQL type, or inclusion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnTypeMapping {
    pub source_column: String,
    pub target_column: String,
    pub target_type: String,
    pub include: bool,
}

/// Configuration for [`transfer_table`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferConfig {
    pub source_schema: Option<String>,
    pub source_table: String,
    pub target_schema: Option<String>,
    pub target_table: String,
    pub target_engine: DatabaseEngine,
    pub mode: TransferMode,
    pub batch_size: usize,
    pub column_mappings: Vec<ColumnTypeMapping>,
}

/// Execution summary returned by [`transfer_table`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TransferReport {
    pub rows_read: u64,
    pub rows_written: u64,
    pub batches: u64,
    pub created_table: bool,
    pub elapsed_ms: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CanonicalSqlType {
    TinyInt,
    SmallInt,
    Integer,
    BigInt,
    Boolean,
    Float,
    Double,
    Decimal,
    Varchar,
    Char,
    Text,
    Blob,
    Date,
    Time,
    Timestamp,
    TimestampTz,
    Uuid,
    Json,
    Xml,
    Interval,
    Inet,
}

#[derive(Debug, Clone, Copy)]
enum TypeParamStyle {
    None,
    Length {
        mysql_default: Option<&'static str>,
        mssql_default: Option<&'static str>,
    },
    Precision {
        clickhouse_default: &'static str,
    },
}

struct TargetTypeRow {
    canonical: CanonicalSqlType,
    sqlite: &'static str,
    duckdb: &'static str,
    postgres: &'static str,
    mysql: &'static str,
    mssql: &'static str,
    clickhouse: &'static str,
    generic: &'static str,
    param_style: TypeParamStyle,
}

impl TargetTypeRow {
    const fn for_engine(&self, engine: DatabaseEngine) -> &'static str {
        match engine {
            DatabaseEngine::Sqlite => self.sqlite,
            DatabaseEngine::Duckdb => self.duckdb,
            DatabaseEngine::Postgres => self.postgres,
            DatabaseEngine::Mysql => self.mysql,
            DatabaseEngine::Mssql => self.mssql,
            DatabaseEngine::Clickhouse => self.clickhouse,
            DatabaseEngine::Generic => self.generic,
        }
    }
}

const SOURCE_TYPE_OVERRIDES: &[(DatabaseEngine, &str, CanonicalSqlType)] = &[
    (DatabaseEngine::Sqlite, "integer", CanonicalSqlType::BigInt),
    (DatabaseEngine::Sqlite, "int", CanonicalSqlType::BigInt),
    (DatabaseEngine::Sqlite, "real", CanonicalSqlType::Double),
    (
        DatabaseEngine::Clickhouse,
        "int8",
        CanonicalSqlType::TinyInt,
    ),
    (
        DatabaseEngine::Clickhouse,
        "uint8",
        CanonicalSqlType::SmallInt,
    ),
    (
        DatabaseEngine::Clickhouse,
        "int16",
        CanonicalSqlType::SmallInt,
    ),
    (
        DatabaseEngine::Clickhouse,
        "uint16",
        CanonicalSqlType::Integer,
    ),
    (
        DatabaseEngine::Clickhouse,
        "int32",
        CanonicalSqlType::Integer,
    ),
    (
        DatabaseEngine::Clickhouse,
        "uint32",
        CanonicalSqlType::BigInt,
    ),
    (
        DatabaseEngine::Clickhouse,
        "int64",
        CanonicalSqlType::BigInt,
    ),
    (
        DatabaseEngine::Clickhouse,
        "uint64",
        CanonicalSqlType::BigInt,
    ),
    (
        DatabaseEngine::Clickhouse,
        "float32",
        CanonicalSqlType::Float,
    ),
    (
        DatabaseEngine::Clickhouse,
        "float64",
        CanonicalSqlType::Double,
    ),
    (DatabaseEngine::Clickhouse, "string", CanonicalSqlType::Text),
    (
        DatabaseEngine::Mysql,
        "tinyint(1)",
        CanonicalSqlType::Boolean,
    ),
    (DatabaseEngine::Mssql, "bit", CanonicalSqlType::Boolean),
    (DatabaseEngine::Mssql, "float", CanonicalSqlType::Double),
];

const BASE_TYPE_ALIASES: &[(&str, CanonicalSqlType)] = &[
    ("tinyint", CanonicalSqlType::TinyInt),
    ("int1", CanonicalSqlType::TinyInt),
    ("smallint", CanonicalSqlType::SmallInt),
    ("int2", CanonicalSqlType::SmallInt),
    ("smallserial", CanonicalSqlType::SmallInt),
    ("int", CanonicalSqlType::Integer),
    ("integer", CanonicalSqlType::Integer),
    ("int4", CanonicalSqlType::Integer),
    ("mediumint", CanonicalSqlType::Integer),
    ("serial", CanonicalSqlType::Integer),
    ("uint16", CanonicalSqlType::Integer),
    ("bigint", CanonicalSqlType::BigInt),
    ("int8", CanonicalSqlType::BigInt),
    ("bigserial", CanonicalSqlType::BigInt),
    ("hugeint", CanonicalSqlType::BigInt),
    ("int128", CanonicalSqlType::BigInt),
    ("uint32", CanonicalSqlType::BigInt),
    ("uint64", CanonicalSqlType::BigInt),
    ("uint128", CanonicalSqlType::BigInt),
    ("bool", CanonicalSqlType::Boolean),
    ("boolean", CanonicalSqlType::Boolean),
    ("bit", CanonicalSqlType::Boolean),
    ("real", CanonicalSqlType::Float),
    ("float4", CanonicalSqlType::Float),
    ("float32", CanonicalSqlType::Float),
    ("binary_float", CanonicalSqlType::Float),
    ("double", CanonicalSqlType::Double),
    ("double precision", CanonicalSqlType::Double),
    ("float", CanonicalSqlType::Double),
    ("float8", CanonicalSqlType::Double),
    ("float64", CanonicalSqlType::Double),
    ("binary_double", CanonicalSqlType::Double),
    ("decimal", CanonicalSqlType::Decimal),
    ("numeric", CanonicalSqlType::Decimal),
    ("dec", CanonicalSqlType::Decimal),
    ("number", CanonicalSqlType::Decimal),
    ("money", CanonicalSqlType::Decimal),
    ("smallmoney", CanonicalSqlType::Decimal),
    ("decimal32", CanonicalSqlType::Decimal),
    ("decimal64", CanonicalSqlType::Decimal),
    ("decimal128", CanonicalSqlType::Decimal),
    ("decimal256", CanonicalSqlType::Decimal),
    ("varchar", CanonicalSqlType::Varchar),
    ("nvarchar", CanonicalSqlType::Varchar),
    ("character varying", CanonicalSqlType::Varchar),
    ("varchar2", CanonicalSqlType::Varchar),
    ("nvarchar2", CanonicalSqlType::Varchar),
    ("char", CanonicalSqlType::Char),
    ("nchar", CanonicalSqlType::Char),
    ("character", CanonicalSqlType::Char),
    ("bpchar", CanonicalSqlType::Char),
    ("fixedstring", CanonicalSqlType::Char),
    ("text", CanonicalSqlType::Text),
    ("ntext", CanonicalSqlType::Text),
    ("clob", CanonicalSqlType::Text),
    ("longtext", CanonicalSqlType::Text),
    ("mediumtext", CanonicalSqlType::Text),
    ("tinytext", CanonicalSqlType::Text),
    ("string", CanonicalSqlType::Text),
    ("blob", CanonicalSqlType::Blob),
    ("bytea", CanonicalSqlType::Blob),
    ("binary", CanonicalSqlType::Blob),
    ("varbinary", CanonicalSqlType::Blob),
    ("image", CanonicalSqlType::Blob),
    ("longblob", CanonicalSqlType::Blob),
    ("mediumblob", CanonicalSqlType::Blob),
    ("tinyblob", CanonicalSqlType::Blob),
    ("date", CanonicalSqlType::Date),
    ("date32", CanonicalSqlType::Date),
    ("time", CanonicalSqlType::Time),
    ("timetz", CanonicalSqlType::Time),
    ("time without time zone", CanonicalSqlType::Time),
    ("time with time zone", CanonicalSqlType::Time),
    ("timestamp", CanonicalSqlType::Timestamp),
    ("timestamp without time zone", CanonicalSqlType::Timestamp),
    ("datetime", CanonicalSqlType::Timestamp),
    ("datetime2", CanonicalSqlType::Timestamp),
    ("smalldatetime", CanonicalSqlType::Timestamp),
    ("datetime64", CanonicalSqlType::Timestamp),
    ("timestamptz", CanonicalSqlType::TimestampTz),
    ("timestamp with time zone", CanonicalSqlType::TimestampTz),
    ("datetimeoffset", CanonicalSqlType::TimestampTz),
    ("uuid", CanonicalSqlType::Uuid),
    ("uniqueidentifier", CanonicalSqlType::Uuid),
    ("json", CanonicalSqlType::Json),
    ("jsonb", CanonicalSqlType::Json),
    ("xml", CanonicalSqlType::Xml),
    ("interval", CanonicalSqlType::Interval),
    ("inet", CanonicalSqlType::Inet),
    ("cidr", CanonicalSqlType::Inet),
    ("ipv4", CanonicalSqlType::Inet),
    ("ipv6", CanonicalSqlType::Inet),
];

const TARGET_TYPE_TABLE: &[TargetTypeRow] = &[
    TargetTypeRow {
        canonical: CanonicalSqlType::TinyInt,
        sqlite: "INTEGER",
        duckdb: "TINYINT",
        postgres: "SMALLINT",
        mysql: "TINYINT",
        mssql: "TINYINT",
        clickhouse: "Int8",
        generic: "SMALLINT",
        param_style: TypeParamStyle::None,
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::SmallInt,
        sqlite: "INTEGER",
        duckdb: "SMALLINT",
        postgres: "SMALLINT",
        mysql: "SMALLINT",
        mssql: "SMALLINT",
        clickhouse: "Int16",
        generic: "SMALLINT",
        param_style: TypeParamStyle::None,
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::Integer,
        sqlite: "INTEGER",
        duckdb: "INTEGER",
        postgres: "INTEGER",
        mysql: "INT",
        mssql: "INT",
        clickhouse: "Int32",
        generic: "INTEGER",
        param_style: TypeParamStyle::None,
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::BigInt,
        sqlite: "INTEGER",
        duckdb: "BIGINT",
        postgres: "BIGINT",
        mysql: "BIGINT",
        mssql: "BIGINT",
        clickhouse: "Int64",
        generic: "BIGINT",
        param_style: TypeParamStyle::None,
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::Boolean,
        sqlite: "BOOLEAN",
        duckdb: "BOOLEAN",
        postgres: "BOOLEAN",
        mysql: "TINYINT(1)",
        mssql: "BIT",
        clickhouse: "Bool",
        generic: "BOOLEAN",
        param_style: TypeParamStyle::None,
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::Float,
        sqlite: "REAL",
        duckdb: "FLOAT",
        postgres: "REAL",
        mysql: "FLOAT",
        mssql: "REAL",
        clickhouse: "Float32",
        generic: "REAL",
        param_style: TypeParamStyle::None,
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::Double,
        sqlite: "REAL",
        duckdb: "DOUBLE",
        postgres: "DOUBLE PRECISION",
        mysql: "DOUBLE",
        mssql: "FLOAT",
        clickhouse: "Float64",
        generic: "DOUBLE PRECISION",
        param_style: TypeParamStyle::None,
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::Decimal,
        sqlite: "NUMERIC",
        duckdb: "DECIMAL",
        postgres: "NUMERIC",
        mysql: "DECIMAL",
        mssql: "DECIMAL",
        clickhouse: "Decimal",
        generic: "DECIMAL",
        param_style: TypeParamStyle::Precision {
            clickhouse_default: "Decimal(18, 4)",
        },
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::Varchar,
        sqlite: "TEXT",
        duckdb: "VARCHAR",
        postgres: "VARCHAR",
        mysql: "VARCHAR",
        mssql: "NVARCHAR",
        clickhouse: "String",
        generic: "VARCHAR",
        param_style: TypeParamStyle::Length {
            mysql_default: Some("VARCHAR(255)"),
            mssql_default: Some("NVARCHAR(MAX)"),
        },
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::Char,
        sqlite: "TEXT",
        duckdb: "VARCHAR",
        postgres: "CHAR",
        mysql: "CHAR",
        mssql: "NCHAR",
        clickhouse: "String",
        generic: "CHAR",
        param_style: TypeParamStyle::Length {
            mysql_default: Some("CHAR(1)"),
            mssql_default: Some("NCHAR(1)"),
        },
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::Text,
        sqlite: "TEXT",
        duckdb: "VARCHAR",
        postgres: "TEXT",
        mysql: "LONGTEXT",
        mssql: "NVARCHAR(MAX)",
        clickhouse: "String",
        generic: "TEXT",
        param_style: TypeParamStyle::None,
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::Blob,
        sqlite: "BLOB",
        duckdb: "BLOB",
        postgres: "BYTEA",
        mysql: "LONGBLOB",
        mssql: "VARBINARY(MAX)",
        clickhouse: "String",
        generic: "BLOB",
        param_style: TypeParamStyle::None,
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::Date,
        sqlite: "DATE",
        duckdb: "DATE",
        postgres: "DATE",
        mysql: "DATE",
        mssql: "DATE",
        clickhouse: "Date32",
        generic: "DATE",
        param_style: TypeParamStyle::None,
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::Time,
        sqlite: "TIME",
        duckdb: "TIME",
        postgres: "TIME",
        mysql: "TIME",
        mssql: "TIME",
        clickhouse: "String",
        generic: "TIME",
        param_style: TypeParamStyle::None,
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::Timestamp,
        sqlite: "TEXT",
        duckdb: "TIMESTAMP",
        postgres: "TIMESTAMP",
        mysql: "DATETIME",
        mssql: "DATETIME2",
        clickhouse: "DateTime64(6)",
        generic: "TIMESTAMP",
        param_style: TypeParamStyle::None,
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::TimestampTz,
        sqlite: "TEXT",
        duckdb: "TIMESTAMPTZ",
        postgres: "TIMESTAMPTZ",
        mysql: "DATETIME",
        mssql: "DATETIMEOFFSET",
        clickhouse: "DateTime64(6, 'UTC')",
        generic: "TIMESTAMPTZ",
        param_style: TypeParamStyle::None,
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::Uuid,
        sqlite: "TEXT",
        duckdb: "UUID",
        postgres: "UUID",
        mysql: "CHAR(36)",
        mssql: "UNIQUEIDENTIFIER",
        clickhouse: "UUID",
        generic: "UUID",
        param_style: TypeParamStyle::None,
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::Json,
        sqlite: "TEXT",
        duckdb: "JSON",
        postgres: "JSONB",
        mysql: "JSON",
        mssql: "NVARCHAR(MAX)",
        clickhouse: "String",
        generic: "JSON",
        param_style: TypeParamStyle::None,
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::Xml,
        sqlite: "TEXT",
        duckdb: "VARCHAR",
        postgres: "XML",
        mysql: "LONGTEXT",
        mssql: "XML",
        clickhouse: "String",
        generic: "TEXT",
        param_style: TypeParamStyle::None,
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::Interval,
        sqlite: "TEXT",
        duckdb: "INTERVAL",
        postgres: "INTERVAL",
        mysql: "VARCHAR(64)",
        mssql: "VARCHAR(64)",
        clickhouse: "String",
        generic: "TEXT",
        param_style: TypeParamStyle::None,
    },
    TargetTypeRow {
        canonical: CanonicalSqlType::Inet,
        sqlite: "TEXT",
        duckdb: "VARCHAR",
        postgres: "INET",
        mysql: "VARCHAR(45)",
        mssql: "VARCHAR(45)",
        clickhouse: "IPv6",
        generic: "TEXT",
        param_style: TypeParamStyle::None,
    },
];

/// Maps a raw SQL column type from `source` to an equivalent SQL type on `target`.
#[must_use]
pub fn map_sql_type(source: DatabaseEngine, target: DatabaseEngine, raw_type: &str) -> String {
    let trimmed = raw_type.trim();
    if trimmed.is_empty() {
        return lookup_target_row(CanonicalSqlType::Text)
            .for_engine(target)
            .to_owned();
    }
    if source == target && source != DatabaseEngine::Generic {
        return trimmed.to_owned();
    }

    let (base, args) = normalize_sql_type(trimmed);
    let canonical = resolve_canonical_type(source, &base, args.as_deref());
    format_target_type(canonical, target, args.as_deref())
}

fn unwrap_wrapper_type(raw: &str) -> &str {
    let mut current = raw.trim();
    loop {
        let lower = current.to_ascii_lowercase();
        let inner_opt = if lower.starts_with("nullable(") && current.ends_with(')') {
            Some(&current[9..current.len() - 1])
        } else if lower.starts_with("lowcardinality(") && current.ends_with(')') {
            Some(&current[15..current.len() - 1])
        } else {
            None
        };
        match inner_opt {
            Some(inner) => current = inner.trim(),
            None => return current,
        }
    }
}

fn normalize_sql_type(raw: &str) -> (String, Option<String>) {
    let unwrapped = unwrap_wrapper_type(raw);
    let without_array = unwrapped.strip_suffix("[]").unwrap_or(unwrapped).trim();
    let (head, args) = match (without_array.find('('), without_array.rfind(')')) {
        (Some(open), Some(close)) if close > open => (
            without_array[..open].trim(),
            Some(without_array[open + 1..close].trim().to_owned()),
        ),
        _ => (without_array, None),
    };
    let lower = head.to_ascii_lowercase();
    let cleaned = lower
        .split_whitespace()
        .filter(|tok| !matches!(*tok, "unsigned" | "zerofill" | "signed"))
        .collect::<Vec<_>>()
        .join(" ");
    (cleaned, args)
}

fn resolve_canonical_type(
    source: DatabaseEngine,
    base: &str,
    args: Option<&str>,
) -> CanonicalSqlType {
    let with_args = args.map(|a| format!("{base}({a})"));
    if let Some(full) = &with_args
        && let Some((_, _, canonical)) = SOURCE_TYPE_OVERRIDES
            .iter()
            .find(|(eng, name, _)| *eng == source && *name == full)
    {
        return *canonical;
    }
    if let Some((_, _, canonical)) = SOURCE_TYPE_OVERRIDES
        .iter()
        .find(|(eng, name, _)| *eng == source && *name == base)
    {
        return *canonical;
    }
    BASE_TYPE_ALIASES
        .iter()
        .find(|(alias, _)| *alias == base)
        .map_or(CanonicalSqlType::Text, |(_, canonical)| *canonical)
}

fn lookup_target_row(canonical: CanonicalSqlType) -> &'static TargetTypeRow {
    TARGET_TYPE_TABLE
        .iter()
        .find(|row| row.canonical == canonical)
        .unwrap_or(&TARGET_TYPE_TABLE[10])
}

fn format_target_type(
    canonical: CanonicalSqlType,
    target: DatabaseEngine,
    args: Option<&str>,
) -> String {
    let row = lookup_target_row(canonical);
    let base = row.for_engine(target);
    let clean_args = args.map(str::trim).filter(|a| !a.is_empty());

    match row.param_style {
        TypeParamStyle::None => base.to_owned(),
        TypeParamStyle::Length {
            mysql_default,
            mssql_default,
        } => format_length_type(target, base, clean_args, mysql_default, mssql_default),
        TypeParamStyle::Precision { clickhouse_default } => {
            format_precision_type(target, base, clean_args, clickhouse_default)
        }
    }
}

fn format_length_type(
    target: DatabaseEngine,
    base: &str,
    args: Option<&str>,
    mysql_default: Option<&'static str>,
    mssql_default: Option<&'static str>,
) -> String {
    let valid_len = args.filter(|a| a.chars().all(|c| c.is_ascii_digit()));
    match (target, valid_len) {
        (DatabaseEngine::Sqlite | DatabaseEngine::Duckdb | DatabaseEngine::Clickhouse, _) => {
            base.to_owned()
        }
        (DatabaseEngine::Mysql, None) => mysql_default.unwrap_or(base).to_owned(),
        (DatabaseEngine::Mssql, None) => mssql_default.unwrap_or(base).to_owned(),
        (_, Some(len)) => format!("{base}({len})"),
        (_, None) => base.to_owned(),
    }
}

fn format_precision_type(
    target: DatabaseEngine,
    base: &str,
    args: Option<&str>,
    clickhouse_default: &'static str,
) -> String {
    let valid_prec = args.filter(|a| {
        a.split(',')
            .all(|part| !part.trim().is_empty() && part.trim().chars().all(|c| c.is_ascii_digit()))
    });
    match (target, valid_prec) {
        (DatabaseEngine::Sqlite, _) => base.to_owned(),
        (DatabaseEngine::Clickhouse, None) => clickhouse_default.to_owned(),
        (_, Some(prec)) => format!("{base}({prec})"),
        (_, None) => base.to_owned(),
    }
}

/// Generates a dialect-aware `CREATE TABLE IF NOT EXISTS` DDL statement for `target_table`
/// from a source [`TableDefinition`].
#[must_use]
pub fn build_target_create_table_ddl(
    def: &TableDefinition,
    target_engine: DatabaseEngine,
    target_table: &str,
) -> String {
    let plan = build_default_column_plan(def, DatabaseEngine::Generic, target_engine, &[]);
    build_target_create_table_ddl_with_plan(def, target_engine, None, target_table, &plan)
}

#[derive(Debug, Clone)]
struct PlannedColumn {
    source_index: usize,
    target_name: String,
    target_type: String,
    is_boolean_target: bool,
}

fn build_default_column_plan(
    def: &TableDefinition,
    source_engine: DatabaseEngine,
    target_engine: DatabaseEngine,
    mappings: &[ColumnTypeMapping],
) -> Vec<PlannedColumn> {
    let mut plan = Vec::with_capacity(def.columns.len());
    for (idx, col) in def.columns.iter().enumerate() {
        let matched = mappings
            .iter()
            .find(|m| m.source_column == col.info.name)
            .or_else(|| {
                mappings
                    .iter()
                    .find(|m| m.source_column.eq_ignore_ascii_case(&col.info.name))
            });
        if let Some(mapping) = matched
            && !mapping.include
        {
            continue;
        }
        let target_name = matched
            .map(|m| m.target_column.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or(&col.info.name)
            .to_owned();
        let raw_source_type = column_type_text(col);
        let target_type = matched
            .map(|m| m.target_type.trim())
            .filter(|s| !s.is_empty())
            .map_or_else(
                || map_sql_type(source_engine, target_engine, &raw_source_type),
                str::to_owned,
            );
        let is_boolean_target = is_boolean_sql_type(&target_type);
        plan.push(PlannedColumn {
            source_index: idx,
            target_name,
            target_type,
            is_boolean_target,
        });
    }
    plan
}

fn is_boolean_sql_type(raw: &str) -> bool {
    let lower = raw.trim().to_ascii_lowercase();
    matches!(lower.as_str(), "bool" | "boolean" | "bit" | "tinyint(1)")
}

fn build_target_create_table_ddl_with_plan(
    def: &TableDefinition,
    target_engine: DatabaseEngine,
    target_schema: Option<&str>,
    target_table: &str,
    plan: &[PlannedColumn],
) -> String {
    let pk_names: HashSet<&str> = def
        .primary_key
        .as_ref()
        .map(|pk| pk.columns.iter().map(String::as_str).collect())
        .unwrap_or_default();

    let columns: Vec<ColumnSpec> = plan
        .iter()
        .filter_map(|p| {
            let col = def.columns.get(p.source_index)?;
            let is_pk = col.info.primary_key || pk_names.contains(col.info.name.as_str());
            let nullable = if target_engine == DatabaseEngine::Clickhouse && is_pk {
                false
            } else {
                col.info.nullable
            };
            Some(ColumnSpec {
                name: p.target_name.clone(),
                data_type: p.target_type.clone(),
                nullable,
                default_expr: convert_default_expr(&col.info.default, target_engine),
                is_primary_key: is_pk,
                is_unique: false,
                auto_increment: col.info.is_auto_increment
                    && target_engine != DatabaseEngine::Duckdb,
            })
        })
        .collect();

    let spec = CreateTableSpec {
        table_name: target_table.to_owned(),
        columns,
        foreign_keys: Vec::new(),
        indexes: Vec::new(),
        if_not_exists: true,
    };
    build_create_table_sql(target_engine, target_schema, &spec)
}

fn column_type_text(col: &ColumnDefinition) -> String {
    col.info.column_type.catalog.as_ref().map_or_else(
        || col.info.column_type.name.clone(),
        |c| c.declaration.as_sql().to_owned(),
    )
}

fn convert_default_expr(default: &ColumnDefault, target_engine: DatabaseEngine) -> Option<String> {
    match default {
        ColumnDefault::None => None,
        ColumnDefault::Literal(val) => Some(format_sql_literal(target_engine, val)),
        ColumnDefault::Expression(expr) => {
            let trimmed = expr.as_sql().trim();
            is_portable_default_expr(trimmed).then(|| trimmed.to_owned())
        }
    }
}

fn is_portable_default_expr(expr: &str) -> bool {
    let upper = expr.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "CURRENT_TIMESTAMP" | "CURRENT_DATE" | "CURRENT_TIME" | "NULL" | "TRUE" | "FALSE"
    ) {
        return true;
    }
    if expr.starts_with('\'') && expr.ends_with('\'') && expr.len() >= 2 {
        return true;
    }
    expr.parse::<f64>().is_ok()
}

/// Formats a [`Value`] as a safe SQL literal for the given [`DatabaseEngine`].
#[must_use]
pub fn format_sql_literal(engine: DatabaseEngine, value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_owned(),
        Value::Bool(flag) => format_bool_literal(engine, *flag),
        Value::Int32(n) => n.to_string(),
        Value::Int(n) => n.to_string(),
        Value::UInt(n) => n.to_string(),
        Value::WideInt(n) => n.to_string(),
        Value::Float32(f) => format_f64_literal(engine, f64::from(*f)),
        Value::Float64(f) => format_f64_literal(engine, *f),
        Value::Decimal(d) => {
            if d.is_finite() {
                d.to_string()
            } else {
                "NULL".to_owned()
            }
        }
        Value::Text(text) => format_text_literal(engine, text),
        Value::Bytes(bytes) => format_bytes_literal(engine, bytes),
        Value::Uuid(u) => format_text_literal(engine, &u.to_string()),
        Value::Json(j) => format_text_literal(engine, j.as_str()),
        Value::Date(t) => format_temporal_date(engine, t),
        Value::Time(t) => format_text_literal(engine, &t.format(None)),
        Value::TimeTz(t) => {
            format_text_literal(engine, &format!("{}{}", t.time.format(None), t.offset))
        }
        Value::Timestamp(t) => format_temporal_timestamp(engine, t),
        Value::TimestampTz(t) => format_temporal_timestamptz(engine, t),
        Value::Interval(i) => format_text_literal(engine, &i.to_string()),
        Value::Bits(b) => format_text_literal(engine, &b.to_string()),
        Value::Array(items) => format_array_literal(engine, items),
        Value::Other(other) => format_text_literal(engine, &other.text),
        Value::Undecodable(_) => "NULL".to_owned(),
    }
}

fn format_bool_literal(engine: DatabaseEngine, flag: bool) -> String {
    match engine {
        DatabaseEngine::Sqlite | DatabaseEngine::Mssql => if flag { "1" } else { "0" }.to_owned(),
        DatabaseEngine::Postgres
        | DatabaseEngine::Mysql
        | DatabaseEngine::Duckdb
        | DatabaseEngine::Clickhouse
        | DatabaseEngine::Generic => if flag { "TRUE" } else { "FALSE" }.to_owned(),
    }
}

fn format_f64_literal(engine: DatabaseEngine, val: f64) -> String {
    if val.is_finite() {
        return format!("{val:?}");
    }
    match (engine, val.is_nan(), val.is_sign_positive()) {
        (DatabaseEngine::Duckdb, true, _) => "CAST('NaN' AS DOUBLE)".to_owned(),
        (DatabaseEngine::Duckdb, false, true) => "CAST('Infinity' AS DOUBLE)".to_owned(),
        (DatabaseEngine::Duckdb, false, false) => "CAST('-Infinity' AS DOUBLE)".to_owned(),
        (DatabaseEngine::Postgres, true, _) => "'NaN'::double precision".to_owned(),
        (DatabaseEngine::Postgres, false, true) => "'Infinity'::double precision".to_owned(),
        (DatabaseEngine::Postgres, false, false) => "'-Infinity'::double precision".to_owned(),
        (DatabaseEngine::Clickhouse, true, _) => "nan".to_owned(),
        (DatabaseEngine::Clickhouse, false, true) => "inf".to_owned(),
        (DatabaseEngine::Clickhouse, false, false) => "-inf".to_owned(),
        (DatabaseEngine::Sqlite, false, true) => "9e999".to_owned(),
        (DatabaseEngine::Sqlite, false, false) => "-9e999".to_owned(),
        _ => "NULL".to_owned(),
    }
}

fn format_text_literal(engine: DatabaseEngine, text: &str) -> String {
    if engine == DatabaseEngine::Duckdb && text.contains('\0') {
        let pieces: Vec<String> = text
            .split('\0')
            .map(|part| engine.quote_literal(part))
            .collect();
        return format!("({})", pieces.join(" || chr(0) || "));
    }
    engine.quote_literal(text)
}

fn format_bytes_literal(engine: DatabaseEngine, bytes: &[u8]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
    match engine {
        DatabaseEngine::Postgres => format!("'\\x{hex}'"),
        DatabaseEngine::Mssql => format!("0x{hex}"),
        DatabaseEngine::Duckdb => {
            let escaped: String = bytes.iter().map(|b| format!("\\x{b:02X}")).collect();
            format!("CAST('{escaped}' AS BLOB)")
        }
        DatabaseEngine::Clickhouse => format!("unhex('{hex}')"),
        DatabaseEngine::Sqlite | DatabaseEngine::Mysql | DatabaseEngine::Generic => {
            format!("X'{hex}'")
        }
    }
}

fn format_temporal_date(engine: DatabaseEngine, val: &Temporal<chrono::NaiveDate>) -> String {
    match val {
        Temporal::Finite(d) => format_text_literal(engine, &d.format("%Y-%m-%d").to_string()),
        Temporal::Infinity => format_text_literal(engine, "infinity"),
        Temporal::NegInfinity => format_text_literal(engine, "-infinity"),
    }
}

fn format_temporal_timestamp(
    engine: DatabaseEngine,
    val: &Temporal<chrono::NaiveDateTime>,
) -> String {
    match val {
        Temporal::Finite(ts) => {
            let formatted = if ts.and_utc().timestamp_subsec_nanos() == 0 {
                ts.format("%Y-%m-%d %H:%M:%S").to_string()
            } else {
                ts.format("%Y-%m-%d %H:%M:%S%.f").to_string()
            };
            format_text_literal(engine, &formatted)
        }
        Temporal::Infinity => format_text_literal(engine, "infinity"),
        Temporal::NegInfinity => format_text_literal(engine, "-infinity"),
    }
}

fn format_temporal_timestamptz(
    engine: DatabaseEngine,
    val: &Temporal<tablepro_core::value::OffsetTimestamp>,
) -> String {
    match val {
        Temporal::Finite(ts) => format_text_literal(engine, &ts.to_string()),
        Temporal::Infinity => format_text_literal(engine, "infinity"),
        Temporal::NegInfinity => format_text_literal(engine, "-infinity"),
    }
}

fn format_array_literal(engine: DatabaseEngine, items: &[Value]) -> String {
    match engine {
        DatabaseEngine::Postgres => {
            let inner: Vec<String> = items
                .iter()
                .map(|v| format_sql_literal(engine, v))
                .collect();
            format!("ARRAY[{}]", inner.join(", "))
        }
        DatabaseEngine::Duckdb | DatabaseEngine::Clickhouse => {
            let inner: Vec<String> = items
                .iter()
                .map(|v| format_sql_literal(engine, v))
                .collect();
            format!("[{}]", inner.join(", "))
        }
        _ => {
            let serialized: Vec<String> = items
                .iter()
                .map(|v| format_sql_literal(engine, v))
                .collect();
            format_text_literal(engine, &format!("[{}]", serialized.join(", ")))
        }
    }
}

fn format_cell_for_column(
    engine: DatabaseEngine,
    value: &Value,
    col: &PlannedColumn,
    warnings: &mut Vec<String>,
) -> String {
    if matches!(value, Value::Undecodable(_)) {
        warnings.push(format!(
            "Column '{}' contained an undecodable value; wrote NULL.",
            col.target_name
        ));
        return "NULL".to_owned();
    }
    if col.is_boolean_target {
        match value {
            Value::Int(n) => return format_bool_literal(engine, *n != 0),
            Value::Int32(n) => return format_bool_literal(engine, *n != 0),
            Value::UInt(n) => return format_bool_literal(engine, *n != 0),
            _ => {}
        }
    }
    format_sql_literal(engine, value)
}

fn build_batch_insert_sqls(
    engine: DatabaseEngine,
    schema: Option<&str>,
    table: &str,
    cols: &[PlannedColumn],
    rows: &[Vec<Value>],
    warnings: &mut Vec<String>,
) -> Vec<String> {
    if rows.is_empty() || cols.is_empty() {
        return Vec::new();
    }
    let qualified = engine.qualify_table(schema, table);
    let col_list = cols
        .iter()
        .map(|c| engine.quote_identifier(&c.target_name))
        .collect::<Vec<_>>()
        .join(", ");
    let chunk_limit = if engine == DatabaseEngine::Mssql {
        MSSQL_MAX_VALUES_ROWS
    } else {
        rows.len()
    };

    let mut statements = Vec::new();
    for chunk in rows.chunks(chunk_limit) {
        let mut value_tuples = Vec::with_capacity(chunk.len());
        for row in chunk {
            let cells: Vec<String> = cols
                .iter()
                .map(|c| {
                    let val = row.get(c.source_index).unwrap_or(&Value::Null);
                    format_cell_for_column(engine, val, c, warnings)
                })
                .collect();
            value_tuples.push(format!("({})", cells.join(", ")));
        }
        statements.push(format!(
            "INSERT INTO {qualified} ({col_list}) VALUES {};",
            value_tuples.join(", ")
        ));
    }
    statements
}

struct NoopSink;

#[async_trait]
impl ResultSink for NoopSink {
    async fn accept(&mut self, _event: ResultEvent) -> SinkFlow {
        SinkFlow::Continue
    }
}

/// Transfers a table's schema (optionally) and rows from `src` to `dst` in streamed batches.
///
/// # Errors
///
/// Returns [`ServiceError`] if validation fails, source metadata/pages cannot be read,
/// target DDL/truncate fails, or any batched `INSERT` statement fails.
pub async fn transfer_table(
    src: &Session,
    dst: &Session,
    src_engine: DatabaseEngine,
    dst_engine: DatabaseEngine,
    config: &TransferConfig,
) -> Result<TransferReport, ServiceError> {
    let cancel = CancellationToken::new();
    transfer_table_cancellable(src, dst, src_engine, dst_engine, config, &cancel).await
}

/// Cancellation-aware variant of [`transfer_table`].
///
/// # Errors
///
/// Returns [`ServiceError`] on validation failure, cancellation, read error, or write error.
pub async fn transfer_table_cancellable(
    src: &Session,
    dst: &Session,
    src_engine: DatabaseEngine,
    dst_engine: DatabaseEngine,
    config: &TransferConfig,
    cancel: &CancellationToken,
) -> Result<TransferReport, ServiceError> {
    let started = Instant::now();
    validate_transfer_config(config)?;
    let batch_size = if config.batch_size == 0 {
        DEFAULT_BATCH_SIZE
    } else {
        config.batch_size
    };

    let src_table = TableRef::new(config.source_schema.clone(), config.source_table.clone());
    let def = fetch_source_table_def(src, &src_table, cancel).await?;
    let plan = build_default_column_plan(
        &def,
        src_engine,
        config.target_engine,
        &config.column_mappings,
    );
    if plan.is_empty() {
        return Err(ServiceError::Import(
            "no columns selected for data transfer".to_owned(),
        ));
    }

    let mut warnings = Vec::new();
    if config.target_engine != dst_engine {
        warnings.push(format!(
            "TransferConfig target_engine ({}) differs from dst_engine ({}); using config.target_engine.",
            config.target_engine.as_str(),
            dst_engine.as_str()
        ));
    }

    let dst_editor = dst
        .open_editor_session(EditorSessionOptions::default(), cancel)
        .await
        .map_err(|err| ServiceError::Session(err.to_string()))?;

    let transfer_res = execute_transfer_loop(
        src,
        dst_editor.as_ref(),
        &src_table,
        &def,
        &plan,
        config,
        batch_size,
        &mut warnings,
        cancel,
    )
    .await;

    let _ = dst_editor.close().await;
    let target_ref = TableRef::new(config.target_schema.clone(), config.target_table.clone());
    dst.invalidate_table(&target_ref);

    let (rows_read, rows_written, batches, created_table) = transfer_res?;
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(TransferReport {
        rows_read,
        rows_written,
        batches,
        created_table,
        elapsed_ms,
        warnings,
    })
}

fn validate_transfer_config(config: &TransferConfig) -> Result<(), ServiceError> {
    if config.source_table.trim().is_empty() {
        return Err(ServiceError::Import(
            "source_table must not be empty".to_owned(),
        ));
    }
    if config.target_table.trim().is_empty() {
        return Err(ServiceError::Import(
            "target_table must not be empty".to_owned(),
        ));
    }
    Ok(())
}

async fn fetch_source_table_def(
    src: &Session,
    table: &TableRef,
    cancel: &CancellationToken,
) -> Result<TableDefinition, ServiceError> {
    if let Ok(def) = src
        .run_cancellable(OperationKind::Metadata, cancel, |lease, call| {
            let t = table.clone();
            async move {
                lease
                    .fetch_table_definition(&t, call)
                    .await
                    .map_err(OperationError::from)
            }
        })
        .await
    {
        return Ok(def);
    }

    let cols = src
        .columns(table.clone())
        .await
        .map_err(|err| ServiceError::Session(err.to_string()))?;
    let columns = cols
        .iter()
        .cloned()
        .map(|info| ColumnDefinition::new(info, ColumnAttributes::default()))
        .collect();
    Ok(TableDefinition {
        table: table.clone(),
        columns,
        primary_key: None,
        indexes: Vec::new(),
        foreign_keys: Vec::new(),
        inbound_foreign_keys: Vec::new(),
        source: None,
        options: None,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "Internal transfer pipeline coordinator"
)]
async fn execute_transfer_loop(
    src: &Session,
    dst_editor: &dyn EditorSession,
    src_table: &TableRef,
    def: &TableDefinition,
    plan: &[PlannedColumn],
    config: &TransferConfig,
    batch_size: usize,
    warnings: &mut Vec<String>,
    cancel: &CancellationToken,
) -> Result<(u64, u64, u64, bool), ServiceError> {
    let created_table = prepare_target_table(dst_editor, def, plan, config, cancel).await?;
    let source_columns: Vec<ColumnInfo> = def.columns.iter().map(|c| c.info.clone()).collect();
    let identity = fetch_row_identity(src, src_table, cancel).await?;
    let filter = FilterSet::default();

    let mut position = PagePosition::First;
    let mut rows_read = 0_u64;
    let mut rows_written = 0_u64;
    let mut batches = 0_u64;

    loop {
        if cancel.is_cancelled() {
            return Err(ServiceError::Session("data transfer cancelled".to_owned()));
        }
        let (page_rows, next_pos) = read_source_page(
            src,
            src_table,
            &source_columns,
            &filter,
            &identity,
            &position,
            batch_size as u64,
            cancel,
        )
        .await?;

        if page_rows.is_empty() {
            break;
        }
        let fetched_len = page_rows.len();
        rows_read = rows_read.saturating_add(fetched_len as u64);

        let stmts = build_batch_insert_sqls(
            config.target_engine,
            config.target_schema.as_deref(),
            &config.target_table,
            plan,
            &page_rows,
            warnings,
        );
        for sql in stmts {
            run_sql_on_editor(dst_editor, &sql, cancel).await?;
        }
        rows_written = rows_written.saturating_add(fetched_len as u64);
        batches = batches.saturating_add(1);

        if fetched_len < batch_size {
            break;
        }
        position = next_pos.unwrap_or(PagePosition::Offset(rows_read));
    }

    Ok((rows_read, rows_written, batches, created_table))
}

async fn prepare_target_table(
    dst_editor: &dyn EditorSession,
    def: &TableDefinition,
    plan: &[PlannedColumn],
    config: &TransferConfig,
    cancel: &CancellationToken,
) -> Result<bool, ServiceError> {
    match config.mode {
        TransferMode::CreateAndLoad => {
            let ddl = build_target_create_table_ddl_with_plan(
                def,
                config.target_engine,
                config.target_schema.as_deref(),
                &config.target_table,
                plan,
            );
            run_sql_on_editor(dst_editor, &ddl, cancel).await?;
            Ok(true)
        }
        TransferMode::TruncateAndLoad => {
            let stmts = build_alter_table_sql(
                config.target_engine,
                config.target_schema.as_deref(),
                &config.target_table,
                &[AlterTableOp::TruncateTable],
            );
            for sql in stmts {
                run_sql_on_editor(dst_editor, &sql, cancel).await?;
            }
            Ok(false)
        }
        TransferMode::Append => Ok(false),
    }
}

async fn fetch_row_identity(
    src: &Session,
    table: &TableRef,
    cancel: &CancellationToken,
) -> Result<RowIdentity, ServiceError> {
    src.run_cancellable(OperationKind::Metadata, cancel, |lease, call| {
        let t = table.clone();
        async move {
            lease
                .row_identity(&t, call)
                .await
                .map_err(OperationError::from)
        }
    })
    .await
    .map_err(|err| ServiceError::Session(err.to_string()))
}

#[expect(
    clippy::too_many_arguments,
    reason = "Bridges core browse PageRequest parameters"
)]
async fn read_source_page(
    src: &Session,
    table: &TableRef,
    columns: &[ColumnInfo],
    filter: &FilterSet,
    identity: &RowIdentity,
    position: &PagePosition,
    limit: u64,
    cancel: &CancellationToken,
) -> Result<(Vec<Vec<Value>>, Option<PagePosition>), ServiceError> {
    let table_owned = table.clone();
    let cols_arc: Arc<[ColumnInfo]> = columns.into();
    let filter_owned = filter.clone();
    let identity_owned = identity.clone();
    let pos_owned = position.clone();

    let page_read = src
        .run_cancellable(OperationKind::Page, cancel, |lease, call| {
            let table = table_owned.clone();
            let cols = cols_arc.clone();
            let filter = filter_owned.clone();
            let identity = identity_owned.clone();
            let position = pos_owned.clone();
            async move {
                let conn = lease.connection();
                let req = PageRequest {
                    table: &table,
                    columns: &cols,
                    filter: &filter,
                    sort: &[],
                    identity: &identity,
                    position: &position,
                    limit,
                };
                read_page::<PageOpError>(conn.as_ref(), &req, &call)
                    .await
                    .map_err(PageOpError::into_operation_error)
            }
        })
        .await
        .map_err(|err| ServiceError::Session(err.to_string()))?;

    let visible = page_read.layout.visible.clone();
    let rows = page_read
        .result
        .rows
        .into_iter()
        .map(|row| {
            row.get(visible.clone())
                .map(<[_]>::to_vec)
                .unwrap_or_default()
        })
        .collect();
    Ok((rows, page_read.next))
}

async fn run_sql_on_editor(
    editor: &dyn EditorSession,
    sql: &str,
    cancel: &CancellationToken,
) -> Result<(), ServiceError> {
    let summary = editor
        .run(
            UserSql::new(sql.to_owned()),
            RunOptions::new(cancel.clone()),
            Box::new(NoopSink),
        )
        .await
        .map_err(|err| ServiceError::Session(err.to_string()))?;
    match summary.end {
        RunEnd::Completed => Ok(()),
        RunEnd::Failed(err) => Err(ServiceError::Session(err.to_string())),
        RunEnd::Cancelled => Err(ServiceError::Session("data transfer cancelled".to_owned())),
        RunEnd::TimedOut { .. } => Err(ServiceError::Session("data transfer timed out".to_owned())),
    }
}

enum PageOpError {
    Browse(tablepro_core::browse::BrowseSqlError),
    Driver(DriverError),
}

impl From<tablepro_core::browse::BrowseSqlError> for PageOpError {
    fn from(err: tablepro_core::browse::BrowseSqlError) -> Self {
        Self::Browse(err)
    }
}

impl From<DriverError> for PageOpError {
    fn from(err: DriverError) -> Self {
        Self::Driver(err)
    }
}

impl PageOpError {
    fn into_operation_error(self) -> OperationError {
        match self {
            Self::Browse(err) => OperationError::Driver(DriverError::Protocol(err.to_string())),
            Self::Driver(err) => OperationError::Driver(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::Mutex;
    use tablepro_core::column::{CatalogType, ColumnKind, ColumnType, ReadForm, SqlTypeExpr};
    use tablepro_core::credentials::CredentialInteraction;
    use tablepro_core::ddl::PrimaryKeyConstraint;
    use tablepro_core::{
        ConnectEnvironment, DatabaseDriver, FileEndpoint, LivenessPolicy, SafeModeLevel,
        WorkspaceSettings,
    };
    use tablepro_driver_duckdb::DuckDbDriver;
    use tablepro_driver_sqlite::SqliteDriver;
    use tablepro_session::SessionDeps;
    use tablepro_session::connect::{
        ConnectRequest, ConnectServices, ConnectionMetadata, Connector, DriverConnector,
        NoStoredCredentials, RequestTarget,
    };
    use tablepro_session::operation::DefaultDeadlines;
    use tablepro_session::runtime::Tasks;
    use tablepro_ssh::InProcessSsh;
    use uuid::Uuid;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[derive(Clone, Default)]
    struct RowsSink {
        rows: Arc<Mutex<Vec<Vec<Value>>>>,
    }

    #[async_trait]
    impl ResultSink for RowsSink {
        async fn accept(&mut self, event: ResultEvent) -> SinkFlow {
            if let ResultEvent::Rows(batch) = event
                && let Ok(mut guard) = self.rows.lock()
            {
                guard.extend(batch);
            }
            SinkFlow::Continue
        }
    }

    async fn open_test_session(
        db_file: &Path,
        driver: Arc<dyn DatabaseDriver>,
        driver_id: &str,
    ) -> Result<Arc<Session>, Box<dyn std::error::Error>> {
        let connector = Arc::new(DriverConnector::new(
            ConnectServices {
                environment: ConnectEnvironment::new("data-transfer-test"),
                ssh: Arc::new(InProcessSsh::system()),
                liveness: LivenessPolicy::DESKTOP,
            },
            Arc::new(DefaultDeadlines::new(LivenessPolicy::DESKTOP)),
        ));
        let endpoint = if driver_id == "duckdb" {
            let conn = duckdb::Connection::open(db_file)?;
            conn.execute_batch("CHECKPOINT")?;
            drop(conn);
            FileEndpoint::open_existing(db_file.to_owned())?
        } else {
            FileEndpoint::create_new(db_file.to_owned())?
        };
        let request = ConnectRequest {
            metadata: ConnectionMetadata {
                id: Uuid::new_v4(),
                name: format!("test-{driver_id}"),
                driver_id: driver_id.to_owned(),
            },
            driver,
            target: RequestTarget::File(endpoint),
            workspace_settings: WorkspaceSettings::workspace(false),
            safe_mode: SafeModeLevel::Off,
        };
        let pending = connector
            .establish(
                request,
                CredentialInteraction::Unattended,
                CancellationToken::new(),
            )
            .await?;
        let session = Session::start(
            pending,
            SessionDeps {
                connector,
                credentials: Arc::new(NoStoredCredentials),
                deadlines: Arc::new(DefaultDeadlines::new(LivenessPolicy::DESKTOP)),
                tasks: Tasks::new(tokio::runtime::Handle::current()),
                prompter: None,
            },
        );
        Ok(session)
    }

    async fn exec_sql_rows(
        session: &Session,
        sql: &str,
    ) -> Result<Vec<Vec<Value>>, Box<dyn std::error::Error>> {
        let cancel = CancellationToken::new();
        let editor = session
            .open_editor_session(EditorSessionOptions::default(), &cancel)
            .await?;
        let sink = RowsSink::default();
        let summary = editor
            .run(
                UserSql::new(sql.to_owned()),
                RunOptions::new(cancel),
                Box::new(sink.clone()),
            )
            .await?;
        let _ = editor.close().await;
        if !matches!(summary.end, RunEnd::Completed) {
            return Err(format!("SQL failed: {:?}", summary.end).into());
        }
        let rows = sink
            .rows
            .lock()
            .map_err(|err| format!("sink lock poisoned: {err}"))?
            .clone();
        Ok(rows)
    }

    fn make_col_def(
        name: &str,
        sql_type: &str,
        nullable: bool,
        pk: bool,
        auto_inc: bool,
    ) -> ColumnDefinition {
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
                is_auto_increment: auto_inc,
                is_generated: false,
                partition_scoped: false,
                default: ColumnDefault::None,
                comment: None,
            },
            ColumnAttributes::default(),
        )
    }

    #[test]
    fn type_mapping_matrix_covers_all_six_engines() {
        assert_eq!(
            map_sql_type(DatabaseEngine::Sqlite, DatabaseEngine::Duckdb, "INTEGER"),
            "BIGINT"
        );
        assert_eq!(
            map_sql_type(DatabaseEngine::Sqlite, DatabaseEngine::Duckdb, "REAL"),
            "DOUBLE"
        );
        assert_eq!(
            map_sql_type(DatabaseEngine::Sqlite, DatabaseEngine::Postgres, "TEXT"),
            "TEXT"
        );
        assert_eq!(
            map_sql_type(
                DatabaseEngine::Postgres,
                DatabaseEngine::Mysql,
                "VARCHAR(120)"
            ),
            "VARCHAR(120)"
        );
        assert_eq!(
            map_sql_type(DatabaseEngine::Postgres, DatabaseEngine::Mssql, "BOOLEAN"),
            "BIT"
        );
        assert_eq!(
            map_sql_type(
                DatabaseEngine::Mysql,
                DatabaseEngine::Postgres,
                "TINYINT(1)"
            ),
            "BOOLEAN"
        );
        assert_eq!(
            map_sql_type(
                DatabaseEngine::Clickhouse,
                DatabaseEngine::Postgres,
                "Nullable(Int8)"
            ),
            "SMALLINT"
        );
        assert_eq!(
            map_sql_type(
                DatabaseEngine::Postgres,
                DatabaseEngine::Clickhouse,
                "DECIMAL(12, 4)"
            ),
            "Decimal(12, 4)"
        );
        assert_eq!(
            map_sql_type(
                DatabaseEngine::Postgres,
                DatabaseEngine::Clickhouse,
                "BYTEA"
            ),
            "String"
        );
        assert_eq!(
            map_sql_type(DatabaseEngine::Duckdb, DatabaseEngine::Mssql, "TIMESTAMPTZ"),
            "DATETIMEOFFSET"
        );
        assert_eq!(
            map_sql_type(DatabaseEngine::Sqlite, DatabaseEngine::Sqlite, "INTEGER"),
            "INTEGER"
        );
    }

    #[test]
    fn build_target_create_table_ddl_handles_dialects() {
        let def = TableDefinition {
            table: TableRef::unqualified("events"),
            columns: vec![
                make_col_def("id", "INTEGER", false, true, true),
                make_col_def("title", "VARCHAR(100)", false, false, false),
                make_col_def("active", "BOOLEAN", true, false, false),
            ],
            primary_key: Some(PrimaryKeyConstraint {
                name: None,
                columns: vec!["id".to_owned()],
            }),
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            inbound_foreign_keys: Vec::new(),
            source: None,
            options: None,
        };

        let duck_ddl = build_target_create_table_ddl(&def, DatabaseEngine::Duckdb, "events_copy");
        assert!(duck_ddl.contains("CREATE TABLE IF NOT EXISTS \"events_copy\""));
        assert!(!duck_ddl.contains("AS IDENTITY"));
        assert!(duck_ddl.contains("PRIMARY KEY (\"id\")"));

        let ch_ddl = build_target_create_table_ddl(&def, DatabaseEngine::Clickhouse, "events_ch");
        assert!(ch_ddl.contains("ENGINE = MergeTree() ORDER BY (\"id\")"));
        assert!(ch_ddl.contains("Nullable(Bool)"));

        let mssql_ddl = build_target_create_table_ddl(&def, DatabaseEngine::Mssql, "events_ms");
        assert!(mssql_ddl.contains("[events_ms]"));
        assert!(mssql_ddl.contains("[title] NVARCHAR(100) NOT NULL"));
    }

    #[test]
    fn value_literal_formatting_is_dialect_aware() {
        assert_eq!(
            format_sql_literal(DatabaseEngine::Sqlite, &Value::Null),
            "NULL"
        );
        assert_eq!(
            format_sql_literal(DatabaseEngine::Sqlite, &Value::Bool(true)),
            "1"
        );
        assert_eq!(
            format_sql_literal(DatabaseEngine::Duckdb, &Value::Bool(false)),
            "FALSE"
        );
        assert_eq!(
            format_sql_literal(DatabaseEngine::Duckdb, &Value::Float64(2.0)),
            "2.0"
        );
        assert_eq!(
            format_sql_literal(DatabaseEngine::Duckdb, &Value::Text("a'b\0c".to_owned())),
            "('a''b' || chr(0) || 'c')"
        );
        assert_eq!(
            format_sql_literal(DatabaseEngine::Duckdb, &Value::Bytes(vec![0xDE, 0xAD])),
            "CAST('\\xDE\\xAD' AS BLOB)"
        );
        assert_eq!(
            format_sql_literal(DatabaseEngine::Sqlite, &Value::Bytes(vec![0xDE, 0xAD])),
            "X'DEAD'"
        );
        assert_eq!(
            format_sql_literal(DatabaseEngine::Postgres, &Value::Bytes(vec![0xDE, 0xAD])),
            "'\\xDEAD'"
        );
        assert_eq!(
            format_sql_literal(DatabaseEngine::Mssql, &Value::Bytes(vec![0xDE, 0xAD])),
            "0xDEAD"
        );
    }

    #[tokio::test]
    async fn column_mappings_exclude_rename_and_override_types() -> TestResult {
        let dir = tempfile::tempdir()?;
        let src = open_test_session(
            &dir.path().join("map_src.sqlite"),
            Arc::new(SqliteDriver),
            "sqlite",
        )
        .await?;
        let dst = open_test_session(
            &dir.path().join("map_dst.duckdb"),
            Arc::new(DuckDbDriver),
            "duckdb",
        )
        .await?;

        exec_sql_rows(
            &src,
            "CREATE TABLE users (id INTEGER PRIMARY KEY, secret TEXT NOT NULL, full_name TEXT NOT NULL);
             INSERT INTO users VALUES (1, 'hidden', 'Ada Lovelace');",
        )
        .await?;

        let cfg = TransferConfig {
            source_schema: None,
            source_table: "users".to_owned(),
            target_schema: None,
            target_table: "users_public".to_owned(),
            target_engine: DatabaseEngine::Duckdb,
            mode: TransferMode::CreateAndLoad,
            batch_size: 10,
            column_mappings: vec![
                ColumnTypeMapping {
                    source_column: "secret".to_owned(),
                    target_column: String::new(),
                    target_type: String::new(),
                    include: false,
                },
                ColumnTypeMapping {
                    source_column: "full_name".to_owned(),
                    target_column: "display_name".to_owned(),
                    target_type: "VARCHAR".to_owned(),
                    include: true,
                },
            ],
        };
        let report = transfer_table(
            &src,
            &dst,
            DatabaseEngine::Sqlite,
            DatabaseEngine::Duckdb,
            &cfg,
        )
        .await?;
        assert_eq!(report.rows_written, 1);
        let cols = dst
            .columns(TableRef::unqualified("users_public"))
            .await
            .map_err(|e| format!("columns failed: {e}"))?;
        let names: Vec<&str> = cols.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["id", "display_name"]);
        let rows = exec_sql_rows(&dst, "SELECT id, display_name FROM users_public").await?;
        assert_eq!(
            rows[0],
            vec![Value::Int(1), Value::Text("Ada Lovelace".to_owned())]
        );

        let _ = src.begin_shutdown().await;
        let _ = dst.begin_shutdown().await;
        Ok(())
    }

    #[tokio::test]
    async fn live_sqlite_to_sqlite_append_and_truncate_and_load() -> TestResult {
        let dir = tempfile::tempdir()?;
        let src = open_test_session(
            &dir.path().join("src.sqlite"),
            Arc::new(SqliteDriver),
            "sqlite",
        )
        .await?;
        let dst = open_test_session(
            &dir.path().join("dst.sqlite"),
            Arc::new(SqliteDriver),
            "sqlite",
        )
        .await?;

        exec_sql_rows(
            &src,
            "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL, qty INTEGER NOT NULL);
             INSERT INTO items VALUES (1, 'one', 10), (2, 'two', 20), (3, 'three', 30), (4, 'four', 40), (5, 'five', 50);
             CREATE TABLE extra_items (id INTEGER PRIMARY KEY, name TEXT NOT NULL, qty INTEGER NOT NULL);
             INSERT INTO extra_items VALUES (6, 'six', 60), (7, 'seven', 70);",
        )
        .await?;

        let base_cfg = TransferConfig {
            source_schema: None,
            source_table: "items".to_owned(),
            target_schema: None,
            target_table: "items_copy".to_owned(),
            target_engine: DatabaseEngine::Sqlite,
            mode: TransferMode::CreateAndLoad,
            batch_size: 2,
            column_mappings: Vec::new(),
        };
        let r1 = transfer_table(
            &src,
            &dst,
            DatabaseEngine::Sqlite,
            DatabaseEngine::Sqlite,
            &base_cfg,
        )
        .await?;
        assert!(r1.created_table);
        assert_eq!((r1.rows_read, r1.rows_written, r1.batches), (5, 5, 3));

        let r2 = transfer_table(
            &src,
            &dst,
            DatabaseEngine::Sqlite,
            DatabaseEngine::Sqlite,
            &TransferConfig {
                source_table: "extra_items".to_owned(),
                mode: TransferMode::Append,
                ..base_cfg.clone()
            },
        )
        .await?;
        assert!(!r2.created_table);
        assert_eq!(r2.rows_written, 2);
        assert_eq!(
            exec_sql_rows(&dst, "SELECT COUNT(*) FROM items_copy").await?[0][0],
            Value::Int(7)
        );

        let r3 = transfer_table(
            &src,
            &dst,
            DatabaseEngine::Sqlite,
            DatabaseEngine::Sqlite,
            &TransferConfig {
                source_table: "extra_items".to_owned(),
                mode: TransferMode::TruncateAndLoad,
                ..base_cfg
            },
        )
        .await?;
        assert!(!r3.created_table);
        assert_eq!(r3.rows_written, 2);
        let after_trunc =
            exec_sql_rows(&dst, "SELECT id, name FROM items_copy ORDER BY id").await?;
        assert_eq!(after_trunc.len(), 2);
        assert_eq!(after_trunc[0][0], Value::Int(6));
        assert_eq!(after_trunc[1][1], Value::Text("seven".to_owned()));

        let _ = src.begin_shutdown().await;
        let _ = dst.begin_shutdown().await;
        Ok(())
    }

    async fn verify_duckdb_transferred_records(duck_session: &Session) -> TestResult {
        let duck_cols = duck_session
            .columns(TableRef::unqualified("records_duck"))
            .await
            .map_err(|e| format!("duck columns error: {e}"))?;
        assert_eq!(duck_cols.len(), 6);
        assert_eq!(duck_cols[0].column_type.name.to_ascii_uppercase(), "BIGINT");
        assert_eq!(duck_cols[2].column_type.name.to_ascii_uppercase(), "DOUBLE");
        assert_eq!(
            duck_cols[3].column_type.name.to_ascii_uppercase(),
            "BOOLEAN"
        );

        let rows = exec_sql_rows(
            duck_session,
            "SELECT id, title, score, active, payload, notes FROM records_duck ORDER BY id",
        )
        .await?;
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0][0], Value::Int(1));
        assert_eq!(rows[0][1], Value::Text("O'Reilly \"SQL\"".to_owned()));
        assert_eq!(rows[0][2], Value::Float64(98.5));
        assert_eq!(rows[0][3], Value::Bool(true));
        assert_eq!(rows[0][4], Value::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF]));
        assert_eq!(rows[0][5], Value::Text("first row".to_owned()));

        assert_eq!(rows[1][0], Value::Int(2));
        assert_eq!(rows[1][2], Value::Float64(2.0));
        assert_eq!(rows[1][3], Value::Bool(false));
        assert_eq!(rows[1][4], Value::Null);
        assert_eq!(rows[1][5], Value::Null);

        assert_eq!(rows[2][1], Value::Text(String::new()));
        assert_eq!(rows[2][2], Value::Null);
        assert_eq!(rows[2][3], Value::Bool(true));
        assert_eq!(rows[2][4], Value::Bytes(vec![0x00, 0xFF]));
        Ok(())
    }

    #[tokio::test]
    async fn live_sqlite_to_duckdb_cross_engine_transfer() -> TestResult {
        let dir = tempfile::tempdir()?;
        let sqlite_session = open_test_session(
            &dir.path().join("source.sqlite"),
            Arc::new(SqliteDriver),
            "sqlite",
        )
        .await?;
        let duck_session = open_test_session(
            &dir.path().join("target.duckdb"),
            Arc::new(DuckDbDriver),
            "duckdb",
        )
        .await?;

        exec_sql_rows(
            &sqlite_session,
            "CREATE TABLE records (
                id INTEGER PRIMARY KEY,
                title TEXT NOT NULL,
                score REAL,
                active BOOLEAN NOT NULL,
                payload BLOB,
                notes TEXT
            );
            INSERT INTO records VALUES
                (1, 'O''Reilly \"SQL\"', 98.5, 1, X'DEADBEEF', 'first row'),
                (2, 'plain', 2.0, 0, NULL, NULL),
                (3, '', NULL, 1, X'00FF', 'last');",
        )
        .await?;

        let cfg = TransferConfig {
            source_schema: None,
            source_table: "records".to_owned(),
            target_schema: None,
            target_table: "records_duck".to_owned(),
            target_engine: DatabaseEngine::Duckdb,
            mode: TransferMode::CreateAndLoad,
            batch_size: 2,
            column_mappings: Vec::new(),
        };
        let report = transfer_table(
            &sqlite_session,
            &duck_session,
            DatabaseEngine::Sqlite,
            DatabaseEngine::Duckdb,
            &cfg,
        )
        .await?;
        assert!(report.created_table);
        assert_eq!(
            (report.rows_read, report.rows_written, report.batches),
            (3, 3, 2)
        );
        verify_duckdb_transferred_records(&duck_session).await?;

        // Verify TruncateAndLoad also works against DuckDB target.
        let trunc_report = transfer_table(
            &sqlite_session,
            &duck_session,
            DatabaseEngine::Sqlite,
            DatabaseEngine::Duckdb,
            &TransferConfig {
                mode: TransferMode::TruncateAndLoad,
                ..cfg
            },
        )
        .await?;
        assert!(!trunc_report.created_table);
        assert_eq!(trunc_report.rows_written, 3);
        let count_rows = exec_sql_rows(&duck_session, "SELECT COUNT(*) FROM records_duck").await?;
        assert_eq!(count_rows[0][0], Value::Int(3));

        let _ = sqlite_session.begin_shutdown().await;
        let _ = duck_session.begin_shutdown().await;
        Ok(())
    }
}
