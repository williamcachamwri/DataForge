//! Dialect-aware DDL builder for visual schema design (Feature 1).

use serde::{Deserialize, Serialize};
use tablepro_core::sql_syntax::SqlGrammar;

/// Database engine dialect for DDL, SQL generation, and plan parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum DatabaseEngine {
    #[default]
    Sqlite,
    Postgres,
    Mysql,
    Mssql,
    Duckdb,
    Clickhouse,
    Generic,
}

impl DatabaseEngine {
    /// Maps a driver/connection ID string (e.g. `"postgres"`, `"sqlite"`) to a [`DatabaseEngine`].
    #[must_use]
    pub fn from_driver_id(driver_id: &str) -> Self {
        match driver_id.trim().to_ascii_lowercase().as_str() {
            "postgres" | "postgresql" | "pg" => Self::Postgres,
            "mysql" | "mariadb" | "mysqlx" => Self::Mysql,
            "sqlite" | "sqlite3" => Self::Sqlite,
            "mssql" | "sqlserver" | "sql_server" => Self::Mssql,
            "duckdb" => Self::Duckdb,
            "clickhouse" | "ch" => Self::Clickhouse,
            _ => Self::Generic,
        }
    }

    /// Maps `tablepro_core`'s [`SqlGrammar`] to a [`DatabaseEngine`].
    #[must_use]
    pub const fn from_grammar(grammar: SqlGrammar) -> Self {
        match grammar {
            SqlGrammar::PostgreSql => Self::Postgres,
            SqlGrammar::MySql => Self::Mysql,
            SqlGrammar::Sqlite => Self::Sqlite,
            SqlGrammar::MsSql => Self::Mssql,
            SqlGrammar::DuckDb => Self::Duckdb,
            SqlGrammar::ClickHouse => Self::Clickhouse,
            SqlGrammar::Cql | SqlGrammar::Redis | SqlGrammar::Mongo => Self::Generic,
        }
    }

    /// Canonical lowercase engine identifier string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
            Self::Mysql => "mysql",
            Self::Mssql => "mssql",
            Self::Duckdb => "duckdb",
            Self::Clickhouse => "clickhouse",
            Self::Generic => "generic",
        }
    }

    /// Quotes a SQL identifier according to the engine's quoting rules.
    #[must_use]
    pub fn quote_identifier(self, ident: &str) -> String {
        match self {
            Self::Mysql => format!("`{}`", ident.replace('`', "``")),
            Self::Mssql => format!("[{}]", ident.replace(']', "]]")),
            Self::Sqlite | Self::Postgres | Self::Duckdb | Self::Clickhouse | Self::Generic => {
                format!("\"{}\"", ident.replace('"', "\"\""))
            }
        }
    }

    /// Qualifies a table name with an optional schema using engine identifier quoting.
    #[must_use]
    pub fn qualify_table(self, schema: Option<&str>, table: &str) -> String {
        match schema.map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => format!(
                "{}.{}",
                self.quote_identifier(s),
                self.quote_identifier(table)
            ),
            None => self.quote_identifier(table),
        }
    }

    /// Quotes a string literal safely for SQL statements.
    #[must_use]
    pub fn quote_literal(self, value: &str) -> String {
        let escaped = match self {
            Self::Mysql | Self::Clickhouse => value.replace('\\', "\\\\").replace('\'', "''"),
            Self::Sqlite | Self::Postgres | Self::Mssql | Self::Duckdb | Self::Generic => {
                value.replace('\'', "''")
            }
        };
        format!("'{escaped}'")
    }
}

impl From<SqlGrammar> for DatabaseEngine {
    fn from(grammar: SqlGrammar) -> Self {
        Self::from_grammar(grammar)
    }
}

impl From<&str> for DatabaseEngine {
    fn from(driver_id: &str) -> Self {
        Self::from_driver_id(driver_id)
    }
}

/// Specification for a single table column in DDL generation.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ColumnSpec {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default_expr: Option<String>,
    pub is_primary_key: bool,
    pub is_unique: bool,
    pub auto_increment: bool,
}

/// Specification for a foreign key constraint in DDL generation.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ForeignKeySpec {
    pub name: String,
    pub column: String,
    pub ref_table: String,
    pub ref_column: String,
    pub on_delete: Option<String>,
    pub on_update: Option<String>,
}

/// Specification for a secondary index in DDL generation.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct IndexSpec {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

/// Full specification for `CREATE TABLE` DDL generation.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CreateTableSpec {
    pub table_name: String,
    pub columns: Vec<ColumnSpec>,
    pub foreign_keys: Vec<ForeignKeySpec>,
    pub indexes: Vec<IndexSpec>,
    pub if_not_exists: bool,
}

/// Operations supported by [`build_alter_table_sql`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlterTableOp {
    AddColumn(ColumnSpec),
    DropColumn(String),
    RenameColumn { from: String, to: String },
    ModifyColumn { column: ColumnSpec },
    AddIndex(IndexSpec),
    DropIndex(String),
    AddForeignKey(ForeignKeySpec),
    DropForeignKey(String),
    TruncateTable,
    DropTable { if_exists: bool },
}

/// Generates a dialect-aware `CREATE TABLE` script (including secondary indexes).
#[must_use]
pub fn build_create_table_sql(
    engine: DatabaseEngine,
    schema: Option<&str>,
    spec: &CreateTableSpec,
) -> String {
    let qualified = engine.qualify_table(schema, &spec.table_name);
    let pk_cols: Vec<&ColumnSpec> = spec.columns.iter().filter(|c| c.is_primary_key).collect();
    let inline_sqlite_pk =
        engine == DatabaseEngine::Sqlite && pk_cols.len() == 1 && pk_cols[0].auto_increment;

    let mut defs: Vec<String> = spec
        .columns
        .iter()
        .map(|col| format!("  {}", format_column_def(engine, col, inline_sqlite_pk)))
        .collect();

    if !pk_cols.is_empty() && !inline_sqlite_pk && engine != DatabaseEngine::Clickhouse {
        let pk_list = pk_cols
            .iter()
            .map(|c| engine.quote_identifier(&c.name))
            .collect::<Vec<_>>()
            .join(", ");
        defs.push(format!("  PRIMARY KEY ({pk_list})"));
    }

    if engine != DatabaseEngine::Clickhouse {
        for fk in &spec.foreign_keys {
            defs.push(format!("  {}", format_fk_clause(engine, schema, fk)));
        }
    }

    let header = create_table_header(
        engine,
        schema,
        &spec.table_name,
        &qualified,
        spec.if_not_exists,
    );
    let body = defs.join(",\n");
    let suffix = clickhouse_engine_suffix(engine, &pk_cols);
    let mut statements = vec![format!("{header} (\n{body}\n){suffix};")];

    for idx in &spec.indexes {
        statements.push(build_create_index_sql(engine, &qualified, idx));
    }
    statements.join("\n")
}

/// Generates dialect-aware `ALTER TABLE` / DDL statements for each requested operation.
#[must_use]
pub fn build_alter_table_sql(
    engine: DatabaseEngine,
    schema: Option<&str>,
    table: &str,
    ops: &[AlterTableOp],
) -> Vec<String> {
    let qualified = engine.qualify_table(schema, table);
    let mut out = Vec::new();
    for op in ops {
        match op {
            AlterTableOp::AddColumn(col) => {
                let col_sql = format_column_def(engine, col, false);
                let kw = if engine == DatabaseEngine::Mssql {
                    "ADD"
                } else {
                    "ADD COLUMN"
                };
                out.push(format!("ALTER TABLE {qualified} {kw} {col_sql};"));
            }
            AlterTableOp::DropColumn(name) => {
                out.push(format!(
                    "ALTER TABLE {qualified} DROP COLUMN {};",
                    engine.quote_identifier(name)
                ));
            }
            AlterTableOp::RenameColumn { from, to } => {
                out.push(format_rename_column(
                    engine, schema, table, &qualified, from, to,
                ));
            }
            AlterTableOp::ModifyColumn { column } => {
                out.extend(format_modify_column(engine, &qualified, column));
            }
            AlterTableOp::AddIndex(idx) => {
                out.push(build_create_index_sql(engine, &qualified, idx));
            }
            AlterTableOp::DropIndex(name) => {
                out.push(format_drop_index(engine, schema, &qualified, name));
            }
            AlterTableOp::AddForeignKey(fk) => {
                out.push(format_add_foreign_key(engine, schema, &qualified, fk));
            }
            AlterTableOp::DropForeignKey(name) => {
                out.push(format_drop_foreign_key(engine, &qualified, name));
            }
            AlterTableOp::TruncateTable => {
                let sql = if engine == DatabaseEngine::Sqlite {
                    format!("DELETE FROM {qualified};")
                } else {
                    format!("TRUNCATE TABLE {qualified};")
                };
                out.push(sql);
            }
            AlterTableOp::DropTable { if_exists } => {
                let clause = if *if_exists { "IF EXISTS " } else { "" };
                out.push(format!("DROP TABLE {clause}{qualified};"));
            }
        }
    }
    out
}

fn create_table_header(
    engine: DatabaseEngine,
    schema: Option<&str>,
    raw_table: &str,
    qualified: &str,
    if_not_exists: bool,
) -> String {
    if !if_not_exists {
        return format!("CREATE TABLE {qualified}");
    }
    if engine == DatabaseEngine::Mssql {
        let raw_qualified = match schema.map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => format!("{s}.{raw_table}"),
            None => raw_table.to_owned(),
        };
        return format!(
            "IF OBJECT_ID(N{}, N'U') IS NULL\nCREATE TABLE {qualified}",
            engine.quote_literal(&raw_qualified)
        );
    }
    format!("CREATE TABLE IF NOT EXISTS {qualified}")
}

fn clickhouse_engine_suffix(engine: DatabaseEngine, pk_cols: &[&ColumnSpec]) -> String {
    if engine != DatabaseEngine::Clickhouse {
        return String::new();
    }
    if pk_cols.is_empty() {
        return " ENGINE = MergeTree() ORDER BY tuple()".to_owned();
    }
    let keys = pk_cols
        .iter()
        .map(|c| engine.quote_identifier(&c.name))
        .collect::<Vec<_>>()
        .join(", ");
    format!(" ENGINE = MergeTree() ORDER BY ({keys})")
}

fn format_column_def(engine: DatabaseEngine, col: &ColumnSpec, inline_sqlite_pk: bool) -> String {
    let qname = engine.quote_identifier(&col.name);
    let data_type = resolve_column_type(engine, col);
    let serial = is_serial_type(&data_type);
    let mut parts = vec![qname, data_type];

    if inline_sqlite_pk && col.is_primary_key && col.auto_increment {
        parts.push("PRIMARY KEY AUTOINCREMENT".to_owned());
    } else if col.auto_increment {
        match engine {
            DatabaseEngine::Mysql => parts.push("AUTO_INCREMENT".to_owned()),
            DatabaseEngine::Mssql => parts.push("IDENTITY(1,1)".to_owned()),
            DatabaseEngine::Postgres | DatabaseEngine::Duckdb => {
                if !serial {
                    parts.push("GENERATED BY DEFAULT AS IDENTITY".to_owned());
                }
            }
            DatabaseEngine::Sqlite | DatabaseEngine::Clickhouse | DatabaseEngine::Generic => {}
        }
    }

    if !col.nullable && engine != DatabaseEngine::Clickhouse {
        parts.push("NOT NULL".to_owned());
    }
    if col.is_unique && !col.is_primary_key && engine != DatabaseEngine::Clickhouse {
        parts.push("UNIQUE".to_owned());
    }
    if let Some(default) = col
        .default_expr
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parts.push(format!("DEFAULT {default}"));
    }
    parts.join(" ")
}

fn resolve_column_type(engine: DatabaseEngine, col: &ColumnSpec) -> String {
    let raw = if col.data_type.trim().is_empty() {
        "TEXT"
    } else {
        col.data_type.trim()
    };
    if engine == DatabaseEngine::Sqlite && col.auto_increment && col.is_primary_key {
        return "INTEGER".to_owned();
    }
    if engine == DatabaseEngine::Postgres && col.auto_increment {
        let upper = raw.to_ascii_uppercase();
        if upper == "INT" || upper == "INTEGER" || upper == "INT4" {
            return "SERIAL".to_owned();
        }
        if upper == "BIGINT" || upper == "INT8" {
            return "BIGSERIAL".to_owned();
        }
    }
    if engine == DatabaseEngine::Clickhouse && col.nullable && !raw.starts_with("Nullable(") {
        return format!("Nullable({raw})");
    }
    raw.to_owned()
}

fn is_serial_type(raw: &str) -> bool {
    let upper = raw.trim().to_ascii_uppercase();
    matches!(upper.as_str(), "SERIAL" | "BIGSERIAL" | "SMALLSERIAL")
}

fn format_fk_clause(engine: DatabaseEngine, schema: Option<&str>, fk: &ForeignKeySpec) -> String {
    let ref_qualified = engine.qualify_table(schema, &fk.ref_table);
    let mut sql = format!(
        "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {ref_qualified} ({})",
        engine.quote_identifier(&fk.name),
        engine.quote_identifier(&fk.column),
        engine.quote_identifier(&fk.ref_column),
    );
    if let Some(on_delete) = fk
        .on_delete
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        sql.push_str(&format!(" ON DELETE {on_delete}"));
    }
    if let Some(on_update) = fk
        .on_update
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        sql.push_str(&format!(" ON UPDATE {on_update}"));
    }
    sql
}

fn build_create_index_sql(
    engine: DatabaseEngine,
    qualified_table: &str,
    idx: &IndexSpec,
) -> String {
    let unique = if idx.unique { "UNIQUE " } else { "" };
    let cols = idx
        .columns
        .iter()
        .map(|c| engine.quote_identifier(c))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "CREATE {unique}INDEX {} ON {qualified_table} ({cols});",
        engine.quote_identifier(&idx.name)
    )
}

fn format_rename_column(
    engine: DatabaseEngine,
    schema: Option<&str>,
    table: &str,
    qualified_table: &str,
    from: &str,
    to: &str,
) -> String {
    if engine == DatabaseEngine::Mssql {
        let prefix = schema
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map_or_else(String::new, |s| format!("{s}."));
        return format!(
            "EXEC sp_rename {}, {}, 'COLUMN';",
            engine.quote_literal(&format!("{prefix}{table}.{from}")),
            engine.quote_literal(to),
        );
    }
    format!(
        "ALTER TABLE {qualified_table} RENAME COLUMN {} TO {};",
        engine.quote_identifier(from),
        engine.quote_identifier(to),
    )
}

fn format_modify_column(
    engine: DatabaseEngine,
    qualified_table: &str,
    col: &ColumnSpec,
) -> Vec<String> {
    let qname = engine.quote_identifier(&col.name);
    let data_type = resolve_column_type(engine, col);
    match engine {
        DatabaseEngine::Sqlite => vec![format!(
            "-- SQLite does not support ALTER COLUMN in-place; recreate table {qualified_table} to modify {qname} ({data_type})."
        )],
        DatabaseEngine::Postgres | DatabaseEngine::Duckdb => {
            let mut stmts = vec![format!(
                "ALTER TABLE {qualified_table} ALTER COLUMN {qname} TYPE {data_type};"
            )];
            let null_action = if col.nullable {
                "DROP NOT NULL"
            } else {
                "SET NOT NULL"
            };
            stmts.push(format!(
                "ALTER TABLE {qualified_table} ALTER COLUMN {qname} {null_action};"
            ));
            if let Some(default) = col
                .default_expr
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                stmts.push(format!(
                    "ALTER TABLE {qualified_table} ALTER COLUMN {qname} SET DEFAULT {default};"
                ));
            }
            stmts
        }
        DatabaseEngine::Mysql => {
            let def = format_column_def(engine, col, false);
            vec![format!(
                "ALTER TABLE {qualified_table} MODIFY COLUMN {def};"
            )]
        }
        DatabaseEngine::Mssql => {
            let null_kw = if col.nullable { "NULL" } else { "NOT NULL" };
            vec![format!(
                "ALTER TABLE {qualified_table} ALTER COLUMN {qname} {data_type} {null_kw};"
            )]
        }
        DatabaseEngine::Clickhouse => vec![format!(
            "ALTER TABLE {qualified_table} MODIFY COLUMN {qname} {data_type};"
        )],
        DatabaseEngine::Generic => vec![format!(
            "ALTER TABLE {qualified_table} ALTER COLUMN {qname} TYPE {data_type};"
        )],
    }
}

fn format_drop_index(
    engine: DatabaseEngine,
    schema: Option<&str>,
    qualified_table: &str,
    name: &str,
) -> String {
    match engine {
        DatabaseEngine::Mysql | DatabaseEngine::Mssql => {
            format!(
                "DROP INDEX {} ON {qualified_table};",
                engine.quote_identifier(name)
            )
        }
        DatabaseEngine::Clickhouse => {
            format!(
                "ALTER TABLE {qualified_table} DROP INDEX {};",
                engine.quote_identifier(name)
            )
        }
        DatabaseEngine::Sqlite
        | DatabaseEngine::Postgres
        | DatabaseEngine::Duckdb
        | DatabaseEngine::Generic => {
            let idx_ref = engine.qualify_table(schema, name);
            format!("DROP INDEX IF EXISTS {idx_ref};")
        }
    }
}

fn format_add_foreign_key(
    engine: DatabaseEngine,
    schema: Option<&str>,
    qualified_table: &str,
    fk: &ForeignKeySpec,
) -> String {
    if engine == DatabaseEngine::Sqlite {
        return format!(
            "-- SQLite does not support ALTER TABLE ADD CONSTRAINT FOREIGN KEY; recreate table {qualified_table} to add {}.",
            engine.quote_identifier(&fk.name)
        );
    }
    let clause = format_fk_clause(engine, schema, fk);
    format!("ALTER TABLE {qualified_table} ADD {clause};")
}

fn format_drop_foreign_key(engine: DatabaseEngine, qualified_table: &str, name: &str) -> String {
    let qname = engine.quote_identifier(name);
    match engine {
        DatabaseEngine::Mysql => format!("ALTER TABLE {qualified_table} DROP FOREIGN KEY {qname};"),
        DatabaseEngine::Sqlite => format!(
            "-- SQLite does not support ALTER TABLE DROP CONSTRAINT FOREIGN KEY; recreate table {qualified_table} to drop {qname}."
        ),
        _ => format!("ALTER TABLE {qualified_table} DROP CONSTRAINT {qname};"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tablepro_core::{
        CallOptions, ConnectEnvironment, ConnectOptions, ConnectTarget, DatabaseDriver,
        EditorSessionOptions, FileEndpoint, LivenessPolicy, RunOptions, UserSql, WorkspaceSettings,
    };
    use tablepro_driver_sqlite::SqliteDriver;
    use tokio_util::sync::CancellationToken;

    fn sample_spec() -> CreateTableSpec {
        CreateTableSpec {
            table_name: "orders".to_owned(),
            if_not_exists: true,
            columns: vec![
                ColumnSpec {
                    name: "id".to_owned(),
                    data_type: "INTEGER".to_owned(),
                    nullable: false,
                    default_expr: None,
                    is_primary_key: true,
                    is_unique: false,
                    auto_increment: true,
                },
                ColumnSpec {
                    name: "user_id".to_owned(),
                    data_type: "INTEGER".to_owned(),
                    nullable: false,
                    default_expr: None,
                    is_primary_key: false,
                    is_unique: false,
                    auto_increment: false,
                },
                ColumnSpec {
                    name: "code".to_owned(),
                    data_type: "TEXT".to_owned(),
                    nullable: false,
                    default_expr: Some("'pending'".to_owned()),
                    is_primary_key: false,
                    is_unique: true,
                    auto_increment: false,
                },
            ],
            foreign_keys: vec![ForeignKeySpec {
                name: "fk_orders_user".to_owned(),
                column: "user_id".to_owned(),
                ref_table: "users".to_owned(),
                ref_column: "id".to_owned(),
                on_delete: Some("CASCADE".to_owned()),
                on_update: None,
            }],
            indexes: vec![IndexSpec {
                name: "idx_orders_user".to_owned(),
                columns: vec!["user_id".to_owned()],
                unique: false,
            }],
        }
    }

    #[test]
    fn postgres_create_and_alter_table_dialect() {
        let sql = build_create_table_sql(DatabaseEngine::Postgres, Some("public"), &sample_spec());
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS \"public\".\"orders\""));
        assert!(sql.contains("\"id\" SERIAL NOT NULL"));
        assert!(sql.contains("PRIMARY KEY (\"id\")"));
        assert!(sql.contains("REFERENCES \"public\".\"users\" (\"id\") ON DELETE CASCADE"));
        assert!(
            sql.contains(
                "CREATE INDEX \"idx_orders_user\" ON \"public\".\"orders\" (\"user_id\");"
            )
        );

        let alters = build_alter_table_sql(
            DatabaseEngine::Postgres,
            Some("public"),
            "orders",
            &[
                AlterTableOp::RenameColumn {
                    from: "code".to_owned(),
                    to: "order_code".to_owned(),
                },
                AlterTableOp::ModifyColumn {
                    column: ColumnSpec {
                        name: "order_code".to_owned(),
                        data_type: "VARCHAR(64)".to_owned(),
                        nullable: true,
                        default_expr: None,
                        is_primary_key: false,
                        is_unique: false,
                        auto_increment: false,
                    },
                },
                AlterTableOp::TruncateTable,
            ],
        );
        assert!(
            alters
                .iter()
                .any(|s| s.contains("RENAME COLUMN \"code\" TO \"order_code\""))
        );
        assert!(
            alters
                .iter()
                .any(|s| s.contains("ALTER COLUMN \"order_code\" TYPE VARCHAR(64)"))
        );
        assert!(alters.iter().any(|s| s.contains("DROP NOT NULL")));
        assert!(
            alters
                .iter()
                .any(|s| s == "TRUNCATE TABLE \"public\".\"orders\";")
        );
    }

    #[test]
    fn mysql_mssql_clickhouse_identifier_quoting_and_ops() {
        let mysql_sql = build_create_table_sql(DatabaseEngine::Mysql, Some("app"), &sample_spec());
        assert!(mysql_sql.contains("CREATE TABLE IF NOT EXISTS `app`.`orders`"));
        assert!(mysql_sql.contains("`id` INTEGER AUTO_INCREMENT NOT NULL"));

        let mssql_alters = build_alter_table_sql(
            DatabaseEngine::Mssql,
            Some("dbo"),
            "orders",
            &[
                AlterTableOp::RenameColumn {
                    from: "code".to_owned(),
                    to: "order_code".to_owned(),
                },
                AlterTableOp::DropIndex("idx_orders_user".to_owned()),
            ],
        );
        assert_eq!(
            mssql_alters[0],
            "EXEC sp_rename 'dbo.orders.code', 'order_code', 'COLUMN';"
        );
        assert_eq!(
            mssql_alters[1],
            "DROP INDEX [idx_orders_user] ON [dbo].[orders];"
        );

        let ch_sql = build_create_table_sql(DatabaseEngine::Clickhouse, None, &sample_spec());
        assert!(ch_sql.contains("ENGINE = MergeTree() ORDER BY (\"id\")"));
    }

    #[tokio::test]
    async fn sqlite_generated_ddl_executes_against_real_sqlite()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let db_path = dir.path().join("ddl_test.sqlite");
        let endpoint = FileEndpoint::create_new(db_path)?;
        let options = ConnectOptions {
            target: ConnectTarget::File(endpoint),
            workspace_settings: WorkspaceSettings::workspace(false),
            liveness: LivenessPolicy::DESKTOP,
            environment: ConnectEnvironment::new("ddl-test"),
        };
        let cancel = CancellationToken::new();
        let conn = SqliteDriver.connect(options, cancel.clone()).await?;

        let users_spec = CreateTableSpec {
            table_name: "users".to_owned(),
            if_not_exists: true,
            columns: vec![ColumnSpec {
                name: "id".to_owned(),
                data_type: "INTEGER".to_owned(),
                nullable: false,
                default_expr: None,
                is_primary_key: true,
                is_unique: false,
                auto_increment: true,
            }],
            foreign_keys: Vec::new(),
            indexes: Vec::new(),
        };
        let create_users = build_create_table_sql(DatabaseEngine::Sqlite, None, &users_spec);
        let create_orders = build_create_table_sql(DatabaseEngine::Sqlite, None, &sample_spec());
        let alter_orders = build_alter_table_sql(
            DatabaseEngine::Sqlite,
            None,
            "orders",
            &[
                AlterTableOp::AddColumn(ColumnSpec {
                    name: "note".to_owned(),
                    data_type: "TEXT".to_owned(),
                    nullable: true,
                    default_expr: None,
                    is_primary_key: false,
                    is_unique: false,
                    auto_increment: false,
                }),
                AlterTableOp::RenameColumn {
                    from: "note".to_owned(),
                    to: "memo".to_owned(),
                },
                AlterTableOp::DropColumn("memo".to_owned()),
            ],
        );

        let script = format!(
            "{create_users}\n{create_orders}\n{}",
            alter_orders.join("\n")
        );
        let editor = conn
            .open_editor_session(EditorSessionOptions::default(), cancel.clone())
            .await?;
        struct NoopSink;
        #[async_trait::async_trait]
        impl tablepro_core::ResultSink for NoopSink {
            async fn accept(
                &mut self,
                _event: tablepro_core::ResultEvent,
            ) -> tablepro_core::SinkFlow {
                tablepro_core::SinkFlow::Continue
            }
        }
        let summary = editor
            .run(
                UserSql::new(script),
                RunOptions::new(cancel.clone()),
                Box::new(NoopSink),
            )
            .await?;
        assert!(matches!(summary.end, tablepro_core::RunEnd::Completed));
        let cols = conn
            .fetch_columns(
                &tablepro_core::meta::TableRef::unqualified("orders"),
                CallOptions::new(None, cancel),
            )
            .await?;
        assert_eq!(cols.len(), 3);
        let _ = Arc::strong_count(&conn);
        Ok(())
    }
}
