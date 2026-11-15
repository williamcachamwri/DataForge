//! Pure-Rust deterministic mock / synthetic data generator with FK pool sampling
//! and dialect-aware SQL batch insertion.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tablepro_core::column::{ColumnKind, IntegerKind, IntegerWidth};
use tablepro_core::ddl::{ColumnDefinition, IdentityStyle, TableDefinition};
use tablepro_core::foreign_key_info::ForeignKeyInfo;
use tablepro_core::value::{Temporal, Value};
use tablepro_core::{
    Connection, EditorSession, EditorSessionOptions, ResultEvent, ResultSink, RunEnd, RunOptions,
    SinkFlow, UserSql,
};
use tablepro_session::Session;
use tokio_util::sync::CancellationToken;

use crate::ddl_builder::DatabaseEngine;
use crate::error::ServiceError;

const DEFAULT_NULLABLE_RATIO: f32 = 0.1;
const MAX_FK_POOL_SIZE: usize = 10_000;
const MAX_SAMPLE_SQL: usize = 3;

/// Synthetic value generation strategy for a single table column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MockValueKind {
    AutoIncrement,
    Uuid,
    Email,
    FullName,
    Company,
    Phone,
    IntRange {
        min: i64,
        max: i64,
    },
    FloatRange {
        min: f64,
        max: f64,
    },
    Bool,
    TimestampIso,
    JsonObject,
    LoremText,
    EnumValues(Vec<String>),
    ForeignKeySample {
        ref_table: String,
        ref_column: String,
    },
}

/// Column-level rule controlling generated mock values and null frequency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MockColumnRule {
    pub column_name: String,
    pub sql_type: String,
    pub nullable: bool,
    pub null_ratio: f32,
    pub kind: MockValueKind,
}

/// Full configuration for generating synthetic rows into a target table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MockGenerationPlan {
    pub schema: Option<String>,
    pub table: String,
    pub row_count: usize,
    pub batch_size: usize,
    pub seed: u64,
    pub rules: Vec<MockColumnRule>,
}

/// Summary report returned after executing a [`MockGenerationPlan`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MockGenerationReport {
    pub inserted_rows: usize,
    pub batches_executed: usize,
    pub elapsed_ms: u64,
    pub sample_sql: Vec<String>,
}

/// Deterministic seeded PRNG (`SplitMix64` seeding + `XorShift64*` generation).
#[derive(Debug, Clone)]
pub struct SeededRng {
    state: u64,
}

impl SeededRng {
    /// Creates a new deterministic PRNG initialized from `seed`.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        let state = splitmix64(seed);
        Self {
            state: if state == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                state
            },
        }
    }

    /// Generates the next pseudo-random `u64`.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Generates a uniform `f64` in `[0.0, 1.0)`.
    pub fn next_f64(&mut self) -> f64 {
        const SCALE: f64 = 1.0 / ((1_u64 << 53) as f64);
        let bits = self.next_u64() >> 11;
        (bits as f64) * SCALE
    }

    /// Generates a uniform `i64` in `[min, max]` inclusive.
    pub fn gen_range_i64(&mut self, min: i64, max: i64) -> i64 {
        if min >= max {
            return min;
        }
        let span = (i128::from(max) - i128::from(min)) as u128 + 1;
        let sample = u128::from(self.next_u64());
        let offset = if span <= u128::from(u64::MAX) {
            (sample * span) >> 64
        } else {
            sample % span
        };
        i64::try_from(i128::from(min) + (offset as i128)).unwrap_or(min)
    }

    /// Generates a `f64` in `[min, max)`.
    pub fn gen_range_f64(&mut self, min: f64, max: f64) -> f64 {
        if !min.is_finite() || !max.is_finite() || min >= max {
            return if min.is_finite() { min } else { 0.0 };
        }
        min + (max - min) * self.next_f64()
    }

    /// Generates a deterministic boolean.
    pub fn gen_bool(&mut self) -> bool {
        (self.next_u64() >> 63) == 1
    }

    /// Selects a random element from `items`, or `None` if `items` is empty.
    pub fn choose<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            return None;
        }
        let idx = usize::try_from(self.next_u64() % (items.len() as u64)).unwrap_or(0);
        items.get(idx)
    }
}

const fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Infers a [`MockColumnRule`] from a column's DDL definition and outbound foreign keys.
#[must_use]
pub fn infer_rule_from_column(
    col: &ColumnDefinition,
    fk_refs: &[ForeignKeyInfo],
) -> MockColumnRule {
    let kind = infer_value_kind(col, fk_refs);
    let nullable = col.info.nullable;
    let null_ratio = if nullable {
        DEFAULT_NULLABLE_RATIO
    } else {
        0.0
    };
    MockColumnRule {
        column_name: col.name().to_owned(),
        sql_type: col.info.column_type.name.clone(),
        nullable,
        null_ratio,
        kind,
    }
}

/// Builds a default [`MockGenerationPlan`] by inspecting all columns and foreign keys of `def`.
#[must_use]
pub fn infer_plan_from_table(
    def: &TableDefinition,
    row_count: usize,
    batch_size: usize,
    seed: u64,
) -> MockGenerationPlan {
    let rules = def
        .columns
        .iter()
        .map(|col| infer_rule_from_column(col, &def.foreign_keys))
        .collect();
    MockGenerationPlan {
        schema: def.table.schema.clone(),
        table: def.table.name.clone(),
        row_count,
        batch_size: batch_size.max(1),
        seed,
        rules,
    }
}

fn infer_value_kind(col: &ColumnDefinition, fk_refs: &[ForeignKeyInfo]) -> MockValueKind {
    if let Some(fk_kind) = find_foreign_key_kind(col.name(), fk_refs) {
        return fk_kind;
    }
    if col.info.is_auto_increment || col.attributes.identity != IdentityStyle::None {
        return MockValueKind::AutoIncrement;
    }

    let lower_name = col.name().to_ascii_lowercase();
    let lower_type = col.info.column_type.name.to_ascii_lowercase();
    match &col.info.column_type.kind {
        ColumnKind::Bool(_) => MockValueKind::Bool,
        ColumnKind::Integer(int_kind) => infer_int_range(&lower_name, *int_kind),
        ColumnKind::Float(_) | ColumnKind::Decimal { .. } | ColumnKind::Money { .. } => {
            infer_float_range(&lower_name)
        }
        ColumnKind::Uuid => MockValueKind::Uuid,
        ColumnKind::Json { .. } => MockValueKind::JsonObject,
        ColumnKind::Enum { labels, .. } if !labels.is_empty() => {
            MockValueKind::EnumValues(labels.clone())
        }
        ColumnKind::Date | ColumnKind::Time { .. } | ColumnKind::Timestamp { .. } => {
            MockValueKind::TimestampIso
        }
        _ => infer_from_name_and_raw_type(&lower_name, &lower_type),
    }
}

fn find_foreign_key_kind(col_name: &str, fk_refs: &[ForeignKeyInfo]) -> Option<MockValueKind> {
    for fk in fk_refs {
        if let Some(idx) = fk
            .columns
            .iter()
            .position(|c| c.eq_ignore_ascii_case(col_name))
        {
            let ref_col = fk
                .ref_columns
                .get(idx)
                .or_else(|| fk.ref_columns.first())
                .cloned()?;
            return Some(MockValueKind::ForeignKeySample {
                ref_table: fk.ref_table.clone(),
                ref_column: ref_col,
            });
        }
    }
    None
}

fn infer_int_range(lower_name: &str, int_kind: IntegerKind) -> MockValueKind {
    if lower_name.contains("age") {
        return MockValueKind::IntRange { min: 18, max: 85 };
    }
    if lower_name.contains("year") {
        return MockValueKind::IntRange {
            min: 1995,
            max: 2026,
        };
    }
    if lower_name.contains("port") {
        return MockValueKind::IntRange {
            min: 1024,
            max: 65_535,
        };
    }
    if lower_name.contains("qty") || lower_name.contains("quantity") || lower_name.contains("count")
    {
        return MockValueKind::IntRange { min: 1, max: 100 };
    }
    let max = match int_kind.width {
        IntegerWidth::W8 => 120,
        IntegerWidth::W16 => 10_000,
        _ => 100_000,
    };
    let min = i64::from(!int_kind.signed);
    MockValueKind::IntRange { min, max }
}

fn infer_float_range(lower_name: &str) -> MockValueKind {
    if lower_name.contains("lat") {
        return MockValueKind::FloatRange {
            min: -90.0,
            max: 90.0,
        };
    }
    if lower_name.contains("lon") || lower_name.contains("lng") {
        return MockValueKind::FloatRange {
            min: -180.0,
            max: 180.0,
        };
    }
    if lower_name.contains("rate") || lower_name.contains("ratio") || lower_name.contains("pct") {
        return MockValueKind::FloatRange { min: 0.0, max: 1.0 };
    }
    MockValueKind::FloatRange {
        min: 1.0,
        max: 5_000.0,
    }
}

fn infer_from_name_and_raw_type(lower_name: &str, lower_type: &str) -> MockValueKind {
    if lower_name.contains("email") || lower_name.contains("e_mail") {
        return MockValueKind::Email;
    }
    if lower_name.contains("uuid") || lower_name.contains("guid") || lower_type.contains("uuid") {
        return MockValueKind::Uuid;
    }
    if lower_name.contains("phone") || lower_name.contains("mobile") || lower_name.contains("tel") {
        return MockValueKind::Phone;
    }
    if lower_name.contains("company")
        || lower_name.contains("organization")
        || lower_name.contains("org_name")
        || lower_name.contains("vendor")
    {
        return MockValueKind::Company;
    }
    if is_person_name_column(lower_name) {
        return MockValueKind::FullName;
    }
    if lower_name.contains("json")
        || lower_name.contains("payload")
        || lower_name.contains("metadata")
        || lower_type.contains("json")
    {
        return MockValueKind::JsonObject;
    }
    if lower_name.ends_with("_at")
        || lower_name.contains("timestamp")
        || lower_name.contains("created")
        || lower_name.contains("updated")
        || lower_type.contains("timestamp")
        || lower_type.contains("datetime")
        || lower_type == "date"
    {
        return MockValueKind::TimestampIso;
    }
    if lower_type.contains("bool") {
        return MockValueKind::Bool;
    }
    if lower_type.contains("int") {
        return MockValueKind::IntRange {
            min: 1,
            max: 100_000,
        };
    }
    if lower_type.contains("float")
        || lower_type.contains("double")
        || lower_type.contains("real")
        || lower_type.contains("numeric")
        || lower_type.contains("decimal")
    {
        return MockValueKind::FloatRange {
            min: 1.0,
            max: 5_000.0,
        };
    }
    MockValueKind::LoremText
}

fn is_person_name_column(lower_name: &str) -> bool {
    matches!(
        lower_name,
        "name" | "full_name" | "fullname" | "first_name" | "last_name" | "author" | "owner"
    ) || lower_name.contains("full_name")
        || lower_name.contains("customer_name")
        || lower_name.contains("user_name")
        || lower_name.contains("contact_name")
}

/// Escapes a string value as a SQL literal using `engine`'s dialect rules.
#[must_use]
pub fn escape_sql_literal(engine: DatabaseEngine, raw: &str) -> String {
    let cleaned = if raw.contains('\0') {
        raw.replace('\0', "")
    } else {
        raw.to_owned()
    };
    engine.quote_literal(&cleaned)
}

/// Formats a [`tablepro_core::value::Value`] as a dialect-aware SQL literal.
#[must_use]
pub fn format_value_as_sql_literal(engine: DatabaseEngine, val: &Value) -> String {
    match val {
        Value::Null => "NULL".to_owned(),
        Value::Bool(b) => format_bool_literal(engine, *b),
        Value::Int32(n) => n.to_string(),
        Value::Int(n) => n.to_string(),
        Value::UInt(n) => n.to_string(),
        Value::WideInt(w) => w.to_string(),
        Value::Float32(f) if f.is_finite() => f.to_string(),
        Value::Float64(f) if f.is_finite() => f.to_string(),
        Value::Float32(_) | Value::Float64(_) => "NULL".to_owned(),
        Value::Decimal(d) => d.to_string(),
        Value::Bytes(bytes) => format_bytes_literal(engine, bytes),
        Value::Text(s) => escape_sql_literal(engine, s),
        Value::Uuid(u) => escape_sql_literal(engine, &u.to_string()),
        Value::Json(j) => escape_sql_literal(engine, j.as_str()),
        Value::Date(Temporal::Finite(d)) => {
            escape_sql_literal(engine, &d.format("%Y-%m-%d").to_string())
        }
        Value::Time(t) => escape_sql_literal(engine, &t.format(None)),
        Value::TimeTz(t) => {
            escape_sql_literal(engine, &format!("{}{}", t.time.format(None), t.offset))
        }
        Value::Timestamp(Temporal::Finite(ts)) => {
            escape_sql_literal(engine, &ts.format("%Y-%m-%d %H:%M:%S").to_string())
        }
        Value::TimestampTz(Temporal::Finite(ts)) => escape_sql_literal(engine, &ts.to_string()),
        Value::Date(Temporal::Infinity)
        | Value::Timestamp(Temporal::Infinity)
        | Value::TimestampTz(Temporal::Infinity) => escape_sql_literal(engine, "infinity"),
        Value::Date(Temporal::NegInfinity)
        | Value::Timestamp(Temporal::NegInfinity)
        | Value::TimestampTz(Temporal::NegInfinity) => escape_sql_literal(engine, "-infinity"),
        Value::Interval(i) => escape_sql_literal(engine, &i.to_string()),
        Value::Bits(b) => escape_sql_literal(engine, &b.to_string()),
        Value::Array(arr) => format_array_literal(engine, arr),
        Value::Other(o) => escape_sql_literal(engine, &o.text),
        Value::Undecodable(u) => escape_sql_literal(engine, &u.type_name),
    }
}

fn format_bool_literal(engine: DatabaseEngine, value: bool) -> String {
    match engine {
        DatabaseEngine::Sqlite | DatabaseEngine::Mssql => if value { "1" } else { "0" }.to_owned(),
        DatabaseEngine::Postgres
        | DatabaseEngine::Mysql
        | DatabaseEngine::Duckdb
        | DatabaseEngine::Clickhouse
        | DatabaseEngine::Generic => if value { "TRUE" } else { "FALSE" }.to_owned(),
    }
}

fn format_bytes_literal(engine: DatabaseEngine, bytes: &[u8]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
    match engine {
        DatabaseEngine::Postgres => format!("'\\x{hex}'"),
        DatabaseEngine::Mssql => format!("0x{hex}"),
        _ => format!("X'{hex}'"),
    }
}

fn format_array_literal(engine: DatabaseEngine, arr: &[Value]) -> String {
    let joined = arr
        .iter()
        .map(|v| format_value_as_sql_literal(engine, v))
        .collect::<Vec<_>>()
        .join(", ");
    escape_sql_literal(engine, &format!("[{joined}]"))
}

/// Generates batched `INSERT` SQL statements for `plan` without executing them.
///
/// # Errors
///
/// Returns [`ServiceError::Runtime`] if the plan is invalid or a required FK sample pool is empty.
pub fn build_mock_insert_batches(
    engine: DatabaseEngine,
    plan: &MockGenerationPlan,
    fk_pools: &HashMap<(String, String), Vec<Value>>,
) -> Result<Vec<String>, ServiceError> {
    validate_plan(plan, fk_pools)?;
    if plan.row_count == 0 {
        return Ok(Vec::new());
    }

    let active_indices: Vec<usize> = plan
        .rules
        .iter()
        .enumerate()
        .filter(|(_, r)| !matches!(r.kind, MockValueKind::AutoIncrement))
        .map(|(idx, _)| idx)
        .collect();

    if active_indices.is_empty() {
        return Ok(build_default_value_batches(engine, plan));
    }

    let qualified = engine.qualify_table(plan.schema.as_deref(), &plan.table);
    let col_list = active_indices
        .iter()
        .map(|&idx| engine.quote_identifier(&plan.rules[idx].column_name))
        .collect::<Vec<_>>()
        .join(", ");

    let effective_batch = effective_batch_size(engine, plan.batch_size);
    let mut rng = SeededRng::new(plan.seed);
    let mut batches = Vec::with_capacity(plan.row_count.div_ceil(effective_batch));

    let mut remaining = plan.row_count;
    let mut row_seq = 0_usize;
    while remaining > 0 {
        let count = remaining.min(effective_batch);
        let mut tuples = Vec::with_capacity(count);
        for _ in 0..count {
            tuples.push(generate_row_tuple(
                engine,
                &plan.rules,
                &active_indices,
                fk_pools,
                &mut rng,
                row_seq,
            )?);
            row_seq += 1;
        }
        batches.push(format!(
            "INSERT INTO {qualified} ({col_list}) VALUES\n  {};",
            tuples.join(",\n  ")
        ));
        remaining -= count;
    }
    Ok(batches)
}

fn validate_plan(
    plan: &MockGenerationPlan,
    fk_pools: &HashMap<(String, String), Vec<Value>>,
) -> Result<(), ServiceError> {
    if plan.table.trim().is_empty() {
        return Err(ServiceError::Runtime(
            "mock generation table name cannot be empty".to_owned(),
        ));
    }
    if plan.rules.is_empty() {
        return Err(ServiceError::Runtime(
            "mock generation plan requires at least one column rule".to_owned(),
        ));
    }
    if plan.row_count == 0 {
        return Ok(());
    }
    for rule in &plan.rules {
        if let MockValueKind::ForeignKeySample {
            ref_table,
            ref_column,
        } = &rule.kind
        {
            let always_null = rule.nullable && rule.null_ratio >= 1.0;
            if always_null {
                continue;
            }
            let key = (ref_table.clone(), ref_column.clone());
            let pool_len = fk_pools.get(&key).map_or(0, Vec::len);
            if pool_len == 0 {
                return Err(ServiceError::Runtime(format!(
                    "foreign key pool is empty for {ref_table}.{ref_column}"
                )));
            }
        }
    }
    Ok(())
}

fn effective_batch_size(engine: DatabaseEngine, requested: usize) -> usize {
    let base = requested.max(1);
    match engine {
        DatabaseEngine::Mssql => base.min(1000),
        _ => base,
    }
}

fn build_default_value_batches(engine: DatabaseEngine, plan: &MockGenerationPlan) -> Vec<String> {
    let qualified = engine.qualify_table(plan.schema.as_deref(), &plan.table);
    let stmt = match engine {
        DatabaseEngine::Mysql => format!("INSERT INTO {qualified} () VALUES ();"),
        _ => format!("INSERT INTO {qualified} DEFAULT VALUES;"),
    };
    vec![stmt; plan.row_count]
}

fn generate_row_tuple(
    engine: DatabaseEngine,
    rules: &[MockColumnRule],
    active_indices: &[usize],
    fk_pools: &HashMap<(String, String), Vec<Value>>,
    rng: &mut SeededRng,
    row_seq: usize,
) -> Result<String, ServiceError> {
    let mut literals = Vec::with_capacity(active_indices.len());
    for &idx in active_indices {
        let rule = &rules[idx];
        let literal = generate_cell_literal(engine, rule, fk_pools, rng, row_seq)?;
        literals.push(literal);
    }
    Ok(format!("({})", literals.join(", ")))
}

fn generate_cell_literal(
    engine: DatabaseEngine,
    rule: &MockColumnRule,
    fk_pools: &HashMap<(String, String), Vec<Value>>,
    rng: &mut SeededRng,
    row_seq: usize,
) -> Result<String, ServiceError> {
    if should_emit_null(rule, rng) {
        return Ok("NULL".to_owned());
    }
    match &rule.kind {
        MockValueKind::AutoIncrement => Ok((row_seq + 1).to_string()),
        MockValueKind::Uuid => Ok(escape_sql_literal(engine, &gen_uuid_v4(rng))),
        MockValueKind::Email => Ok(escape_sql_literal(engine, &gen_email(rng, row_seq))),
        MockValueKind::FullName => Ok(escape_sql_literal(engine, &gen_full_name(rng))),
        MockValueKind::Company => Ok(escape_sql_literal(engine, &gen_company(rng))),
        MockValueKind::Phone => Ok(escape_sql_literal(engine, &gen_phone(rng))),
        MockValueKind::IntRange { min, max } => Ok(rng.gen_range_i64(*min, *max).to_string()),
        MockValueKind::FloatRange { min, max } => Ok(format_float_sample(rng, *min, *max)),
        MockValueKind::Bool => Ok(format_bool_literal(engine, rng.gen_bool())),
        MockValueKind::TimestampIso => Ok(escape_sql_literal(engine, &gen_timestamp_iso(rng))),
        MockValueKind::JsonObject => Ok(escape_sql_literal(engine, &gen_json_object(rng, row_seq))),
        MockValueKind::LoremText => Ok(escape_sql_literal(engine, &gen_lorem_text(rng))),
        MockValueKind::EnumValues(values) => {
            if let Some(chosen) = rng.choose(values) {
                Ok(escape_sql_literal(engine, chosen))
            } else if rule.nullable {
                Ok("NULL".to_owned())
            } else {
                Ok(escape_sql_literal(engine, ""))
            }
        }
        MockValueKind::ForeignKeySample {
            ref_table,
            ref_column,
        } => {
            let key = (ref_table.clone(), ref_column.clone());
            let pool = fk_pools.get(&key).ok_or_else(|| {
                ServiceError::Runtime(format!(
                    "missing foreign key pool for {ref_table}.{ref_column}"
                ))
            })?;
            let chosen = rng.choose(pool).ok_or_else(|| {
                ServiceError::Runtime(format!(
                    "foreign key pool is empty for {ref_table}.{ref_column}"
                ))
            })?;
            Ok(format_value_as_sql_literal(engine, chosen))
        }
    }
}

fn should_emit_null(rule: &MockColumnRule, rng: &mut SeededRng) -> bool {
    if !rule.nullable {
        return false;
    }
    let ratio = rule.null_ratio.clamp(0.0, 1.0);
    if ratio <= 0.0 {
        return false;
    }
    if ratio >= 1.0 {
        return true;
    }
    rng.next_f64() < f64::from(ratio)
}

fn format_float_sample(rng: &mut SeededRng, min: f64, max: f64) -> String {
    let val = rng.gen_range_f64(min, max);
    if (max - min).abs() <= 2.0 {
        format!("{val:.4}")
    } else {
        format!("{val:.2}")
    }
}

fn gen_uuid_v4(rng: &mut SeededRng) -> String {
    let hi = (rng.next_u64() & 0xFFFF_FFFF_FFFF_0FFF) | 0x0000_0000_0000_4000;
    let lo = (rng.next_u64() & 0x3FFF_FFFF_FFFF_FFFF) | 0x8000_0000_0000_0000;
    let p1 = (hi >> 32) as u32;
    let p2 = ((hi >> 16) & 0xFFFF) as u16;
    let p3 = (hi & 0xFFFF) as u16;
    let p4 = (lo >> 48) as u16;
    let p5 = lo & 0xFFFF_FFFF_FFFF;
    format!("{p1:08x}-{p2:04x}-{p3:04x}-{p4:04x}-{p5:012x}")
}

fn gen_full_name(rng: &mut SeededRng) -> String {
    let first = rng.choose(FIRST_NAMES).copied().unwrap_or("Alex");
    let last = rng.choose(LAST_NAMES).copied().unwrap_or("Morgan");
    format!("{first} {last}")
}

fn gen_email(rng: &mut SeededRng, row_seq: usize) -> String {
    let first = rng.choose(FIRST_NAMES).copied().unwrap_or("alex");
    let last = rng.choose(LAST_NAMES).copied().unwrap_or("morgan");
    let domain = rng.choose(EMAIL_DOMAINS).copied().unwrap_or("example.com");
    let tag = rng.gen_range_i64(10, 999);
    let clean_first: String = first
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let clean_last: String = last
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    format!("{clean_first}.{clean_last}{tag}_{row_seq}@{domain}")
}

fn gen_company(rng: &mut SeededRng) -> String {
    let prefix = rng.choose(COMPANY_PREFIXES).copied().unwrap_or("Northwind");
    let suffix = rng.choose(COMPANY_SUFFIXES).copied().unwrap_or("Systems");
    format!("{prefix} {suffix}")
}

fn gen_phone(rng: &mut SeededRng) -> String {
    let area = rng.gen_range_i64(201, 989);
    let mid = rng.gen_range_i64(100, 999);
    let tail = rng.gen_range_i64(1000, 9999);
    format!("+1-{area:03}-{mid:03}-{tail:04}")
}

fn gen_timestamp_iso(rng: &mut SeededRng) -> String {
    let year = rng.gen_range_i64(2020, 2026);
    let month = rng.gen_range_i64(1, 12);
    let max_day = days_in_month(year, month);
    let day = rng.gen_range_i64(1, max_day);
    let hour = rng.gen_range_i64(0, 23);
    let minute = rng.gen_range_i64(0, 59);
    let second = rng.gen_range_i64(0, 59);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

const fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) => 29,
        _ => 28,
    }
}

fn gen_json_object(rng: &mut SeededRng, row_seq: usize) -> String {
    let env = rng.choose(JSON_ENVS).copied().unwrap_or("prod");
    let active = rng.gen_bool();
    let score = rng.gen_range_i64(10, 100);
    format!(
        "{{\"seq\":{},\"env\":\"{env}\",\"active\":{active},\"score\":{score}}}",
        row_seq + 1
    )
}

fn gen_lorem_text(rng: &mut SeededRng) -> String {
    let word_count = usize::try_from(rng.gen_range_i64(3, 7)).unwrap_or(4);
    let mut words = Vec::with_capacity(word_count);
    for idx in 0..word_count {
        let raw = rng.choose(LOREM_WORDS).copied().unwrap_or("data");
        if idx == 0 {
            words.push(capitalize_word(raw));
        } else {
            words.push(raw.to_owned());
        }
    }
    words.join(" ")
}

fn capitalize_word(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => {
            let mut out = first.to_uppercase().to_string();
            out.push_str(chars.as_str());
            out
        }
        None => String::new(),
    }
}

/// Executes `plan` against a live [`tablepro_session::Session`], querying any referenced
/// parent foreign-key value pools and inserting generated rows in batches.
///
/// # Errors
///
/// Returns [`ServiceError::Session`] or [`ServiceError::Runtime`] if querying FK pools,
/// generating batches, or executing SQL inserts fails.
pub async fn execute_mock_generation(
    session: &Session,
    plan: &MockGenerationPlan,
) -> Result<MockGenerationReport, ServiceError> {
    let engine = DatabaseEngine::from_grammar(session.grammar());
    let cancel = CancellationToken::new();
    let editor = session
        .open_editor_session(EditorSessionOptions::default(), &cancel)
        .await
        .map_err(|err| ServiceError::Session(err.to_string()))?;
    execute_with_editor(engine, editor.as_ref(), plan, &cancel).await
}

/// Executes `plan` directly against a [`tablepro_core::Connection`].
///
/// # Errors
///
/// Returns [`ServiceError::Session`] or [`ServiceError::Runtime`] if querying FK pools,
/// generating batches, or executing SQL inserts fails.
pub async fn execute_mock_generation_on_connection(
    conn: &dyn Connection,
    plan: &MockGenerationPlan,
) -> Result<MockGenerationReport, ServiceError> {
    let engine = DatabaseEngine::from_grammar(conn.dialect().grammar());
    let cancel = CancellationToken::new();
    let editor = conn
        .open_editor_session(EditorSessionOptions::default(), cancel.clone())
        .await
        .map_err(|err| ServiceError::Session(err.to_string()))?;
    execute_with_editor(engine, editor.as_ref(), plan, &cancel).await
}

async fn execute_with_editor(
    engine: DatabaseEngine,
    editor: &dyn EditorSession,
    plan: &MockGenerationPlan,
    cancel: &CancellationToken,
) -> Result<MockGenerationReport, ServiceError> {
    let started = Instant::now();
    let result = execute_with_editor_inner(engine, editor, plan, cancel, started).await;
    editor.close().await;
    result
}

async fn execute_with_editor_inner(
    engine: DatabaseEngine,
    editor: &dyn EditorSession,
    plan: &MockGenerationPlan,
    cancel: &CancellationToken,
    started: Instant,
) -> Result<MockGenerationReport, ServiceError> {
    let fk_pools = collect_fk_pools(engine, editor, plan, cancel).await?;
    let batches = build_mock_insert_batches(engine, plan, &fk_pools)?;
    let sample_sql: Vec<String> = batches.iter().take(MAX_SAMPLE_SQL).cloned().collect();

    for batch_sql in &batches {
        run_sql_collect_rows(editor, batch_sql, cancel).await?;
    }

    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(MockGenerationReport {
        inserted_rows: plan.row_count,
        batches_executed: batches.len(),
        elapsed_ms,
        sample_sql,
    })
}

async fn collect_fk_pools(
    engine: DatabaseEngine,
    editor: &dyn EditorSession,
    plan: &MockGenerationPlan,
    cancel: &CancellationToken,
) -> Result<HashMap<(String, String), Vec<Value>>, ServiceError> {
    let mut needed = BTreeSet::new();
    for rule in &plan.rules {
        if let MockValueKind::ForeignKeySample {
            ref_table,
            ref_column,
        } = &rule.kind
        {
            needed.insert((ref_table.clone(), ref_column.clone()));
        }
    }
    let mut pools = HashMap::with_capacity(needed.len());
    if plan.row_count == 0 {
        return Ok(pools);
    }
    for (ref_table, ref_column) in needed {
        let sql = build_fk_pool_query(
            engine,
            plan.schema.as_deref(),
            &ref_table,
            &ref_column,
            MAX_FK_POOL_SIZE,
        );
        let rows = run_sql_collect_rows(editor, &sql, cancel).await?;
        let values: Vec<Value> = rows
            .into_iter()
            .filter_map(|mut r| (!r.is_empty()).then(|| r.remove(0)))
            .filter(|v| !v.is_null())
            .collect();
        pools.insert((ref_table, ref_column), values);
    }
    Ok(pools)
}

fn build_fk_pool_query(
    engine: DatabaseEngine,
    default_schema: Option<&str>,
    ref_table: &str,
    ref_column: &str,
    limit: usize,
) -> String {
    let qualified = match ref_table.split_once('.') {
        Some((s, t)) => engine.qualify_table(Some(s), t),
        None => engine.qualify_table(default_schema, ref_table),
    };
    let qcol = engine.quote_identifier(ref_column);
    match engine {
        DatabaseEngine::Mssql => {
            format!("SELECT DISTINCT TOP {limit} {qcol} FROM {qualified} WHERE {qcol} IS NOT NULL;")
        }
        _ => {
            format!(
                "SELECT DISTINCT {qcol} FROM {qualified} WHERE {qcol} IS NOT NULL LIMIT {limit};"
            )
        }
    }
}

#[derive(Default)]
struct RowCollector {
    rows: Mutex<Vec<Vec<Value>>>,
}

struct RowCollectorSink {
    collector: Arc<RowCollector>,
}

#[async_trait]
impl ResultSink for RowCollectorSink {
    async fn accept(&mut self, event: ResultEvent) -> SinkFlow {
        if let ResultEvent::Rows(batch) = event
            && let Ok(mut guard) = self.collector.rows.lock()
        {
            guard.extend(batch);
        }
        SinkFlow::Continue
    }
}

async fn run_sql_collect_rows(
    editor: &dyn EditorSession,
    sql: &str,
    cancel: &CancellationToken,
) -> Result<Vec<Vec<Value>>, ServiceError> {
    let collector = Arc::new(RowCollector::default());
    let sink = Box::new(RowCollectorSink {
        collector: collector.clone(),
    });
    let summary = editor
        .run(
            UserSql::new(sql.to_owned()),
            RunOptions::new(cancel.clone()),
            sink,
        )
        .await
        .map_err(|err| ServiceError::Session(err.to_string()))?;

    match summary.end {
        RunEnd::Completed => {
            let guard = collector
                .rows
                .lock()
                .map_err(|err| ServiceError::Session(format!("collector lock poisoned: {err}")))?;
            Ok(guard.clone())
        }
        RunEnd::Failed(err) => Err(ServiceError::Session(err.to_string())),
        RunEnd::Cancelled => Err(ServiceError::Session(
            "mock generation execution cancelled".to_owned(),
        )),
        RunEnd::TimedOut { .. } => Err(ServiceError::Session(
            "mock generation execution timed out".to_owned(),
        )),
    }
}

const FIRST_NAMES: &[&str] = &[
    "Alice", "Liam", "Maya", "Noah", "Priya", "Mateo", "Elena", "Lucas", "Hannah", "Omar", "Chloe",
    "Ethan", "Sofia", "Julian", "Nadia", "Caleb", "Zara", "Adrian", "Mei", "Daniel", "Aria",
    "Samuel", "Layla", "Owen", "Kira", "Victor", "Tessa", "Marcus", "Sana", "Leo", "Clara",
    "Isaac",
];

const LAST_NAMES: &[&str] = &[
    "Johnson",
    "Patel",
    "Kim",
    "O'Connor",
    "Garcia",
    "Nguyen",
    "Chen",
    "Smith",
    "Kowalski",
    "Hernandez",
    "Tanaka",
    "Muller",
    "Rossi",
    "D'Angelo",
    "Silva",
    "Williams",
    "Brown",
    "Park",
    "Alvarez",
    "Nakamura",
    "Berg",
    "Petrov",
    "Okafor",
    "Jensen",
    "Moreau",
    "Singh",
    "Campbell",
    "Foster",
    "Bennett",
    "Brooks",
    "Reed",
    "Turner",
];

const EMAIL_DOMAINS: &[&str] = &[
    "example.com",
    "acme.io",
    "northwind.dev",
    "globex.org",
    "initech.net",
    "contoso.co",
];

const COMPANY_PREFIXES: &[&str] = &[
    "Northwind",
    "Acme",
    "Globex",
    "Initech",
    "Apex",
    "Vanguard",
    "Summit",
    "Vertex",
    "Horizon",
    "Cascade",
    "Meridian",
    "Cobalt",
    "Atlas",
    "Helios",
    "Pinnacle",
    "Solstice",
];

const COMPANY_SUFFIXES: &[&str] = &[
    "Systems",
    "Labs",
    "Analytics",
    "Logistics",
    "Cloud",
    "Technologies",
    "Corp",
    "Industries",
    "Software",
    "Dynamics",
];

const JSON_ENVS: &[&str] = &["prod", "staging", "dev", "qa"];

const LOREM_WORDS: &[&str] = &[
    "adaptive",
    "analytics",
    "cluster",
    "compact",
    "consistent",
    "data",
    "distributed",
    "durable",
    "edge",
    "elastic",
    "engine",
    "event",
    "fast",
    "indexed",
    "ledger",
    "metrics",
    "modular",
    "native",
    "pipeline",
    "query",
    "reliable",
    "replica",
    "resilient",
    "schema",
    "shard",
    "snapshot",
    "storage",
    "stream",
    "synthetic",
    "telemetry",
    "vector",
    "workload",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tablepro_core::column::{
        CatalogType, ColumnDefault, ColumnInfo, ColumnType, FloatWidth, ReadForm, SqlTypeExpr,
        TemporalZone, TextLength,
    };
    use tablepro_core::credentials::CredentialInteraction;
    use tablepro_core::ddl::{ColumnAttributes, ReferentialAction};
    use tablepro_core::meta::TableRef;
    use tablepro_core::{
        ConnectEnvironment, FileEndpoint, LivenessPolicy, SafeModeLevel, WorkspaceSettings,
    };
    use tablepro_driver_sqlite::SqliteDriver;
    use tablepro_session::runtime::Tasks;
    use tablepro_session::{
        ConnectRequest, ConnectServices, ConnectionMetadata, Connector, DefaultDeadlines,
        DriverConnector, NoStoredCredentials, RequestTarget, SessionDeps,
    };
    use tablepro_ssh::InProcessSsh;
    use uuid::Uuid;

    fn make_column(
        name: &str,
        sql_type: &str,
        kind: ColumnKind,
        nullable: bool,
        auto_inc: bool,
    ) -> ColumnDefinition {
        ColumnDefinition::new(
            ColumnInfo {
                name: name.to_owned(),
                column_type: ColumnType {
                    name: sql_type.to_owned(),
                    kind,
                    catalog: Some(CatalogType {
                        declaration: SqlTypeExpr::from_catalog_text(sql_type),
                        cast_target: SqlTypeExpr::from_catalog_text(sql_type),
                    }),
                    dynamic_storage: false,
                    read_form: ReadForm::Native,
                },
                nullable,
                primary_key: auto_inc,
                is_auto_increment: auto_inc,
                is_generated: false,
                partition_scoped: false,
                default: ColumnDefault::None,
                comment: None,
            },
            ColumnAttributes::default(),
        )
    }

    async fn open_sqlite_session(db_path: &Path) -> Result<Arc<Session>, ServiceError> {
        let connector = Arc::new(DriverConnector::new(
            ConnectServices {
                environment: ConnectEnvironment::new("mock-gen-test"),
                ssh: Arc::new(InProcessSsh::system()),
                liveness: LivenessPolicy::DESKTOP,
            },
            Arc::new(DefaultDeadlines::new(LivenessPolicy::DESKTOP)),
        ));
        let endpoint = FileEndpoint::create_new(db_path.to_owned())
            .map_err(|e| ServiceError::Session(e.to_string()))?;
        let request = ConnectRequest {
            metadata: ConnectionMetadata {
                id: Uuid::new_v4(),
                name: "sqlite-mock-test".to_owned(),
                driver_id: "sqlite".to_owned(),
            },
            driver: Arc::new(SqliteDriver),
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
            .await
            .map_err(|e| ServiceError::Session(e.to_string()))?;
        Ok(Session::start(
            pending,
            SessionDeps {
                connector,
                credentials: Arc::new(NoStoredCredentials),
                deadlines: Arc::new(DefaultDeadlines::new(LivenessPolicy::DESKTOP)),
                tasks: Tasks::new(tokio::runtime::Handle::current()),
                prompter: None,
            },
        ))
    }

    async fn query_sqlite(session: &Session, sql: &str) -> Result<Vec<Vec<Value>>, ServiceError> {
        let cancel = CancellationToken::new();
        let editor = session
            .open_editor_session(EditorSessionOptions::default(), &cancel)
            .await
            .map_err(|e| ServiceError::Session(e.to_string()))?;
        let res = run_sql_collect_rows(editor.as_ref(), sql, &cancel).await;
        editor.close().await;
        res
    }

    #[test]
    fn seeded_prng_is_deterministic_and_handles_zero_seed() {
        let mut rng_a = SeededRng::new(0);
        let mut rng_b = SeededRng::new(0);
        let mut rng_c = SeededRng::new(42);
        let seq_a: Vec<u64> = (0..10).map(|_| rng_a.next_u64()).collect();
        let seq_b: Vec<u64> = (0..10).map(|_| rng_b.next_u64()).collect();
        let seq_c: Vec<u64> = (0..10).map(|_| rng_c.next_u64()).collect();
        assert_eq!(seq_a, seq_b);
        assert_ne!(seq_a, seq_c);
        assert!(seq_a.iter().all(|&v| v != 0));
    }

    #[test]
    fn infer_rule_detects_fk_autoincrement_and_semantic_types() {
        let fks = vec![ForeignKeyInfo {
            name: "fk_user_org".to_owned(),
            columns: vec!["org_id".to_owned()],
            ref_schema: None,
            ref_table: "organizations".to_owned(),
            ref_columns: vec!["id".to_owned()],
            on_delete: Some(ReferentialAction::Cascade),
            on_update: None,
        }];
        let id_col = make_column(
            "id",
            "INTEGER",
            ColumnKind::Integer(IntegerKind {
                width: IntegerWidth::W64,
                signed: true,
            }),
            false,
            true,
        );
        let fk_col = make_column(
            "org_id",
            "INTEGER",
            ColumnKind::Integer(IntegerKind {
                width: IntegerWidth::W64,
                signed: true,
            }),
            false,
            false,
        );
        let email_col = make_column(
            "email",
            "TEXT",
            ColumnKind::Text {
                length: TextLength::Unbounded,
            },
            false,
            false,
        );
        let role_col = make_column(
            "role",
            "ENUM",
            ColumnKind::Enum {
                labels: vec!["admin".to_owned(), "viewer".to_owned()],
                multiple: false,
            },
            true,
            false,
        );
        let price_col = make_column(
            "unit_price",
            "DOUBLE PRECISION",
            ColumnKind::Float(FloatWidth::F64),
            false,
            false,
        );
        let created_col = make_column(
            "created_at",
            "TIMESTAMP",
            ColumnKind::Timestamp {
                fractional_digits: None,
                zone: TemporalZone::Naive,
            },
            false,
            false,
        );

        assert_eq!(
            infer_rule_from_column(&id_col, &fks).kind,
            MockValueKind::AutoIncrement
        );
        assert_eq!(
            infer_rule_from_column(&fk_col, &fks).kind,
            MockValueKind::ForeignKeySample {
                ref_table: "organizations".to_owned(),
                ref_column: "id".to_owned(),
            }
        );
        assert_eq!(
            infer_rule_from_column(&email_col, &fks).kind,
            MockValueKind::Email
        );
        assert_eq!(
            infer_rule_from_column(&role_col, &fks).kind,
            MockValueKind::EnumValues(vec!["admin".to_owned(), "viewer".to_owned()])
        );
        assert!(matches!(
            infer_rule_from_column(&price_col, &fks).kind,
            MockValueKind::FloatRange { .. }
        ));
        assert_eq!(
            infer_rule_from_column(&created_col, &fks).kind,
            MockValueKind::TimestampIso
        );
    }

    #[test]
    fn infer_plan_from_table_populates_all_columns() {
        let def = TableDefinition {
            table: TableRef::new(Some("public".to_owned()), "contacts"),
            columns: vec![
                make_column("id", "UUID", ColumnKind::Uuid, false, false),
                make_column(
                    "full_name",
                    "TEXT",
                    ColumnKind::Text {
                        length: TextLength::Unbounded,
                    },
                    false,
                    false,
                ),
                make_column(
                    "company_name",
                    "TEXT",
                    ColumnKind::Text {
                        length: TextLength::Unbounded,
                    },
                    true,
                    false,
                ),
            ],
            primary_key: None,
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            inbound_foreign_keys: Vec::new(),
            source: None,
            options: None,
        };
        let plan = infer_plan_from_table(&def, 50, 20, 99);
        assert_eq!(plan.schema.as_deref(), Some("public"));
        assert_eq!(plan.table, "contacts");
        assert_eq!(plan.rules[0].kind, MockValueKind::Uuid);
        assert_eq!(plan.rules[1].kind, MockValueKind::FullName);
        assert_eq!(plan.rules[2].kind, MockValueKind::Company);
        assert_eq!(plan.rules[1].null_ratio, 0.0);
        assert!(plan.rules[2].nullable);
    }

    #[test]
    fn null_ratio_is_only_applied_when_nullable() -> Result<(), Box<dyn std::error::Error>> {
        let plan = MockGenerationPlan {
            schema: None,
            table: "items".to_owned(),
            row_count: 20,
            batch_size: 20,
            seed: 77,
            rules: vec![
                MockColumnRule {
                    column_name: "required_name".to_owned(),
                    sql_type: "TEXT".to_owned(),
                    nullable: false,
                    null_ratio: 0.95,
                    kind: MockValueKind::Company,
                },
                MockColumnRule {
                    column_name: "always_null".to_owned(),
                    sql_type: "TEXT".to_owned(),
                    nullable: true,
                    null_ratio: 1.0,
                    kind: MockValueKind::LoremText,
                },
                MockColumnRule {
                    column_name: "half_null".to_owned(),
                    sql_type: "INTEGER".to_owned(),
                    nullable: true,
                    null_ratio: 0.5,
                    kind: MockValueKind::IntRange { min: 1, max: 10 },
                },
            ],
        };
        let batches = build_mock_insert_batches(DatabaseEngine::Sqlite, &plan, &HashMap::new())?;
        assert_eq!(batches.len(), 1);
        let sql = &batches[0];
        let lines: Vec<&str> = sql.lines().skip(1).collect();
        assert_eq!(lines.len(), 20);
        let mut half_null_count = 0_usize;
        for line in &lines {
            assert!(
                !line.starts_with("  (NULL,"),
                "non-nullable column emitted NULL: {line}"
            );
            assert!(
                line.contains(", NULL, "),
                "100% null_ratio nullable column should emit NULL: {line}"
            );
            if line.ends_with(", NULL)") || line.ends_with(", NULL),") || line.ends_with(", NULL);")
            {
                half_null_count += 1;
            }
        }
        assert!(
            (3..=17).contains(&half_null_count),
            "expected ~10 nulls, got {half_null_count}"
        );
        Ok(())
    }

    #[test]
    fn sql_escaping_and_dialect_booleans_and_batching() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            escape_sql_literal(DatabaseEngine::Postgres, "O'Brien"),
            "'O''Brien'"
        );
        assert_eq!(
            escape_sql_literal(DatabaseEngine::Mysql, "a\\b'c"),
            "'a\\\\b''c'"
        );
        assert_eq!(
            format_value_as_sql_literal(DatabaseEngine::Sqlite, &Value::Bool(true)),
            "1"
        );
        assert_eq!(
            format_value_as_sql_literal(DatabaseEngine::Postgres, &Value::Bool(false)),
            "FALSE"
        );

        let plan = MockGenerationPlan {
            schema: Some("public".to_owned()),
            table: "events".to_owned(),
            row_count: 25,
            batch_size: 10,
            seed: 12345,
            rules: vec![
                MockColumnRule {
                    column_name: "id".to_owned(),
                    sql_type: "SERIAL".to_owned(),
                    nullable: false,
                    null_ratio: 0.0,
                    kind: MockValueKind::AutoIncrement,
                },
                MockColumnRule {
                    column_name: "active".to_owned(),
                    sql_type: "BOOLEAN".to_owned(),
                    nullable: false,
                    null_ratio: 0.0,
                    kind: MockValueKind::Bool,
                },
                MockColumnRule {
                    column_name: "payload".to_owned(),
                    sql_type: "JSONB".to_owned(),
                    nullable: false,
                    null_ratio: 0.0,
                    kind: MockValueKind::JsonObject,
                },
            ],
        };
        let pg_batches_1 =
            build_mock_insert_batches(DatabaseEngine::Postgres, &plan, &HashMap::new())?;
        let pg_batches_2 =
            build_mock_insert_batches(DatabaseEngine::Postgres, &plan, &HashMap::new())?;
        assert_eq!(pg_batches_1, pg_batches_2);
        assert_eq!(pg_batches_1.len(), 3);
        assert!(
            pg_batches_1[0]
                .starts_with("INSERT INTO \"public\".\"events\" (\"active\", \"payload\") VALUES")
        );
        assert!(!pg_batches_1[0].contains("\"id\""));
        assert_eq!(pg_batches_1[0].lines().count(), 11);
        assert_eq!(pg_batches_1[2].lines().count(), 6);
        Ok(())
    }

    #[test]
    fn realistic_value_generators_produce_valid_shapes() {
        let mut rng = SeededRng::new(2026);
        let uuid_str = gen_uuid_v4(&mut rng);
        assert_eq!(uuid_str.len(), 36);
        assert_eq!(uuid_str.chars().nth(14), Some('4'));

        let email = gen_email(&mut rng, 3);
        assert!(email.contains('@'));
        assert!(!email.contains('\''));

        let ts = gen_timestamp_iso(&mut rng);
        assert_eq!(ts.len(), 20);
        assert!(ts.ends_with('Z'));

        let json = gen_json_object(&mut rng, 0);
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&json);
        assert!(parsed.is_ok());
    }

    fn test_rule(
        column_name: &str,
        sql_type: &str,
        nullable: bool,
        null_ratio: f32,
        kind: MockValueKind,
    ) -> MockColumnRule {
        MockColumnRule {
            column_name: column_name.to_owned(),
            sql_type: sql_type.to_owned(),
            nullable,
            null_ratio,
            kind,
        }
    }

    fn sample_org_plan() -> MockGenerationPlan {
        MockGenerationPlan {
            schema: None,
            table: "organizations".to_owned(),
            row_count: 5,
            batch_size: 2,
            seed: 101,
            rules: vec![
                test_rule("id", "INTEGER", false, 0.0, MockValueKind::AutoIncrement),
                test_rule("name", "TEXT", false, 0.0, MockValueKind::Company),
                test_rule(
                    "plan_tier",
                    "TEXT",
                    false,
                    0.0,
                    MockValueKind::EnumValues(vec![
                        "free".to_owned(),
                        "pro".to_owned(),
                        "enterprise".to_owned(),
                    ]),
                ),
            ],
        }
    }

    fn sample_member_plan() -> MockGenerationPlan {
        MockGenerationPlan {
            schema: None,
            table: "members".to_owned(),
            row_count: 15,
            batch_size: 6,
            seed: 555,
            rules: vec![
                test_rule("id", "INTEGER", false, 0.0, MockValueKind::AutoIncrement),
                test_rule(
                    "org_id",
                    "INTEGER",
                    false,
                    0.0,
                    MockValueKind::ForeignKeySample {
                        ref_table: "organizations".to_owned(),
                        ref_column: "id".to_owned(),
                    },
                ),
                test_rule("uid", "TEXT", false, 0.0, MockValueKind::Uuid),
                test_rule("full_name", "TEXT", false, 0.0, MockValueKind::FullName),
                test_rule("email", "TEXT", false, 0.0, MockValueKind::Email),
                test_rule("phone", "TEXT", false, 0.0, MockValueKind::Phone),
                test_rule(
                    "age",
                    "INTEGER",
                    false,
                    0.0,
                    MockValueKind::IntRange { min: 18, max: 65 },
                ),
                test_rule(
                    "score",
                    "REAL",
                    false,
                    0.0,
                    MockValueKind::FloatRange {
                        min: 10.0,
                        max: 100.0,
                    },
                ),
                test_rule("active", "INTEGER", false, 0.0, MockValueKind::Bool),
                test_rule("joined_at", "TEXT", false, 0.0, MockValueKind::TimestampIso),
                test_rule("meta", "TEXT", false, 0.0, MockValueKind::JsonObject),
                test_rule("bio", "TEXT", true, 0.4, MockValueKind::LoremText),
            ],
        }
    }

    #[tokio::test]
    async fn live_sqlite_parent_child_fk_generation_and_determinism()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let s1 = open_sqlite_session(&dir.path().join("mock_1.sqlite")).await?;
        let s2 = open_sqlite_session(&dir.path().join("mock_2.sqlite")).await?;

        let schema_sql = "
            PRAGMA foreign_keys = ON;
            CREATE TABLE organizations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                plan_tier TEXT NOT NULL
            );
            CREATE TABLE members (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                org_id INTEGER NOT NULL REFERENCES organizations(id),
                uid TEXT NOT NULL,
                full_name TEXT NOT NULL,
                email TEXT NOT NULL,
                phone TEXT NOT NULL,
                age INTEGER NOT NULL,
                score REAL NOT NULL,
                active INTEGER NOT NULL,
                joined_at TEXT NOT NULL,
                meta TEXT NOT NULL,
                bio TEXT
            );
        ";
        query_sqlite(&s1, schema_sql).await?;
        query_sqlite(&s2, schema_sql).await?;

        let org_plan = sample_org_plan();
        let org_report = execute_mock_generation(&s1, &org_plan).await?;
        assert_eq!(org_report.inserted_rows, 5);
        assert_eq!(org_report.batches_executed, 3);
        assert!(!org_report.sample_sql.is_empty());
        execute_mock_generation(&s2, &org_plan).await?;

        let member_plan = sample_member_plan();
        let member_report = execute_mock_generation(&s1, &member_plan).await?;
        assert_eq!(member_report.inserted_rows, 15);
        assert_eq!(member_report.batches_executed, 3);
        execute_mock_generation(&s2, &member_plan).await?;

        let verify_sql = "SELECT m.org_id, m.uid, m.full_name, m.email, m.age, m.bio \
             FROM members m INNER JOIN organizations o ON o.id = m.org_id ORDER BY m.id;";
        let rows1 = query_sqlite(&s1, verify_sql).await?;
        let rows2 = query_sqlite(&s2, verify_sql).await?;
        assert_eq!(rows1.len(), 15);
        assert_eq!(rows1, rows2);
        assert!(rows1.iter().any(|r| r.get(5).is_some_and(Value::is_null)));
        assert!(rows1.iter().any(|r| r.get(5).is_some_and(|v| !v.is_null())));

        let _ = s1.begin_shutdown().await;
        let _ = s2.begin_shutdown().await;
        Ok(())
    }

    #[tokio::test]
    async fn live_sqlite_empty_fk_pool_returns_error() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let db_path = dir.path().join("empty_fk.sqlite");
        let session = open_sqlite_session(&db_path).await?;
        query_sqlite(
            &session,
            "CREATE TABLE parents (id INTEGER PRIMARY KEY); \
             CREATE TABLE children (id INTEGER PRIMARY KEY, parent_id INTEGER NOT NULL REFERENCES parents(id));",
        )
        .await?;

        let plan = MockGenerationPlan {
            schema: None,
            table: "children".to_owned(),
            row_count: 5,
            batch_size: 5,
            seed: 1,
            rules: vec![MockColumnRule {
                column_name: "parent_id".to_owned(),
                sql_type: "INTEGER".to_owned(),
                nullable: false,
                null_ratio: 0.0,
                kind: MockValueKind::ForeignKeySample {
                    ref_table: "parents".to_owned(),
                    ref_column: "id".to_owned(),
                },
            }],
        };

        let err = execute_mock_generation(&session, &plan).await;
        assert!(err.is_err());
        let _ = session.begin_shutdown().await;
        Ok(())
    }
}
