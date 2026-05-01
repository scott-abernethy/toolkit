use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use common::sql::{self, QueryResponse};
use common::{load_named_section, Result, ToolkitError};
use native_tls::TlsConnector;
use postgres::types::Type;
use postgres_native_tls::MakeTlsConnector;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::time::Duration;
use uuid::Uuid;

/// Maximum time to wait for the TCP/TLS handshake before giving up.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Server-side `statement_timeout` (milliseconds). Long-running queries are
/// aborted by Postgres itself, releasing the daemon thread that called us.
const STATEMENT_TIMEOUT_MS: u64 = 60_000;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ConnConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: Option<String>,
    /// Use TLS for this connection (default: false).
    pub tls: Option<bool>,
    /// Tables the agent is permitted to write to (INSERT/UPDATE/DELETE/TRUNCATE).
    /// If absent or empty, the connection is treated as strictly read-only.
    pub writable_tables: Option<Vec<String>>,
}

impl ConnConfig {
    fn use_tls(&self) -> bool {
        self.tls.unwrap_or(false)
    }
}

pub fn load_config(conn: Option<&str>) -> Result<ConnConfig> {
    load_named_section("psql", conn)
}

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

fn connect(config: &ConnConfig) -> Result<postgres::Client> {
    let mut cfg = postgres::Config::new();
    cfg.host(&config.host)
        .port(config.port)
        .dbname(&config.database)
        .user(&config.user)
        .connect_timeout(CONNECT_TIMEOUT);

    if let Some(pw) = &config.password {
        cfg.password(pw);
    }

    // Always start the session read-only at the server. Writes to allowlisted
    // tables are wrapped in a transaction with `SET LOCAL transaction_read_write`
    // so the read-only default is the floor for everything else, including
    // anything outside the explicit write statement.
    //
    // statement_timeout aborts long-running queries server-side so a stuck
    // query can't pin a daemon thread indefinitely.
    cfg.options(&format!(
        "-c default_transaction_read_only=on -c statement_timeout={STATEMENT_TIMEOUT_MS}"
    ));

    if config.use_tls() {
        let tls = TlsConnector::builder()
            .build()
            .map_err(|e| ToolkitError::connection(format!("tls error: {}", e)))?;
        let connector = MakeTlsConnector::new(tls);
        cfg.connect(connector).map_err(|e| sanitize_pg_error(&e))
    } else {
        cfg.connect(postgres::NoTls)
            .map_err(|e| sanitize_pg_error(&e))
    }
}

// ---------------------------------------------------------------------------
// Error sanitisation
// ---------------------------------------------------------------------------

fn sanitize_pg_error(e: &postgres::Error) -> ToolkitError {
    let msg = e.to_string().to_lowercase();
    if msg.contains("password authentication failed") || msg.contains("authentication failed") {
        ToolkitError::auth("authentication failed")
    } else if msg.contains("timeout") || msg.contains("timed out") {
        ToolkitError::connection("connection timed out")
    } else if msg.contains("connection refused") || msg.contains("connection to server") {
        ToolkitError::connection("connection refused")
    } else if msg.contains("database") && msg.contains("does not exist") {
        ToolkitError::not_found("database does not exist")
    } else if msg.contains("role") && msg.contains("does not exist") {
        ToolkitError::not_found("role does not exist")
    } else if msg.contains("permission denied") || msg.contains("insufficient privilege") {
        ToolkitError::permission("permission denied")
    } else if msg.contains("ssl") || msg.contains("tls") {
        ToolkitError::connection("ssl error")
    } else if let Some(db_err) = e.as_db_error() {
        // Whitelist SQLSTATE only — db_error.message() can echo client
        // identifiers or backend internals. Agents can look up SQLSTATE codes
        // (e.g. 42P01 = undefined_table, 42601 = syntax_error).
        ToolkitError::other(format!("database error: SQLSTATE {}", db_err.code().code()))
    } else {
        ToolkitError::other("query error")
    }
}

// ---------------------------------------------------------------------------
// Type → JSON mapping
// ---------------------------------------------------------------------------

fn cell_to_json(row: &postgres::Row, i: usize) -> Value {
    let ty = row.columns()[i].type_();
    match *ty {
        Type::BOOL => row
            .get::<_, Option<bool>>(i)
            .map(Value::Bool)
            .unwrap_or(Value::Null),

        Type::INT2 => row
            .get::<_, Option<i16>>(i)
            .map(|v| Value::Number(v.into()))
            .unwrap_or(Value::Null),

        Type::INT4 => row
            .get::<_, Option<i32>>(i)
            .map(|v| Value::Number(v.into()))
            .unwrap_or(Value::Null),

        Type::INT8 => row
            .get::<_, Option<i64>>(i)
            .map(|v| Value::Number(v.into()))
            .unwrap_or(Value::Null),

        Type::FLOAT4 => row
            .get::<_, Option<f32>>(i)
            .map(|v| {
                serde_json::Number::from_f64(v as f64)
                    .map(Value::Number)
                    .unwrap_or(Value::Null)
            })
            .unwrap_or(Value::Null),

        Type::FLOAT8 => row
            .get::<_, Option<f64>>(i)
            .map(|v| {
                serde_json::Number::from_f64(v)
                    .map(Value::Number)
                    .unwrap_or(Value::Null)
            })
            .unwrap_or(Value::Null),

        Type::JSONB | Type::JSON => row
            .get::<_, Option<serde_json::Value>>(i)
            .unwrap_or(Value::Null),

        Type::UUID => row
            .get::<_, Option<Uuid>>(i)
            .map(|u| Value::String(u.to_string()))
            .unwrap_or(Value::Null),

        Type::TIMESTAMP => row
            .get::<_, Option<NaiveDateTime>>(i)
            .map(|dt| Value::String(dt.to_string()))
            .unwrap_or(Value::Null),

        Type::TIMESTAMPTZ => row
            .get::<_, Option<DateTime<Utc>>>(i)
            .map(|dt| Value::String(dt.to_rfc3339()))
            .unwrap_or(Value::Null),

        Type::DATE => row
            .get::<_, Option<NaiveDate>>(i)
            .map(|d| Value::String(d.to_string()))
            .unwrap_or(Value::Null),

        Type::TIME => row
            .get::<_, Option<NaiveTime>>(i)
            .map(|t| Value::String(t.to_string()))
            .unwrap_or(Value::Null),

        // TEXT, VARCHAR, NAME, BPCHAR, UNKNOWN, NUMERIC, and anything else
        // with a text-compatible representation.
        _ => row
            .try_get::<_, Option<String>>(i)
            .ok()
            .flatten()
            .map(Value::String)
            .unwrap_or(Value::Null),
    }
}

fn row_to_json(row: &postgres::Row) -> Map<String, Value> {
    row.columns()
        .iter()
        .enumerate()
        .map(|(i, col)| (col.name().to_string(), cell_to_json(row, i)))
        .collect()
}

// ---------------------------------------------------------------------------
// Query execution
// ---------------------------------------------------------------------------

fn exec_query(
    config: &ConnConfig,
    sql: &str,
    params: &[&(dyn postgres::types::ToSql + Sync)],
) -> Result<Vec<postgres::Row>> {
    let mut client = connect(config)?;
    client.query(sql, params).map_err(|e| sanitize_pg_error(&e))
}

// ---------------------------------------------------------------------------
// Subcommand implementations
// ---------------------------------------------------------------------------

pub fn run_query(config: &ConnConfig, sql: &str) -> Result<QueryResponse> {
    // Authorise every write target individually. Multi-statement input like
    // `INSERT INTO allowed; DELETE FROM forbidden` only passes if every
    // target is on the allowlist.
    let targets = sql::detect_write_targets(sql);
    for table in &targets {
        sql::assert_write_allowed(config.writable_tables.as_ref(), table)?;
    }

    let raw = if targets.is_empty() {
        exec_query(config, sql, &[])?
    } else {
        // Session is read-only; flip to read-write only for the duration of
        // this transaction. ROLLBACK on error preserves the read-only floor.
        let mut client = connect(config)?;
        client
            .batch_execute("BEGIN; SET LOCAL transaction_read_write;")
            .map_err(|e| sanitize_pg_error(&e))?;
        match client.query(sql, &[]) {
            Ok(rows) => {
                client
                    .batch_execute("COMMIT;")
                    .map_err(|e| sanitize_pg_error(&e))?;
                rows
            }
            Err(e) => {
                let _ = client.batch_execute("ROLLBACK;");
                return Err(sanitize_pg_error(&e));
            }
        }
    };

    let rows: Vec<Map<String, Value>> = raw.iter().map(row_to_json).collect();
    Ok(QueryResponse::from_rows(rows))
}

pub fn list_tables(config: &ConnConfig, schema: &str) -> Result<QueryResponse> {
    let raw = exec_query(
        config,
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = $1 ORDER BY table_name",
        &[&schema],
    )?;
    let rows: Vec<Map<String, Value>> = raw.iter().map(row_to_json).collect();
    Ok(QueryResponse::from_rows(rows))
}

pub fn describe_table(config: &ConnConfig, table: &str) -> Result<QueryResponse> {
    let (schema, tbl) = if table.contains('.') {
        let parts: Vec<&str> = table.splitn(2, '.').collect();
        (parts[0], parts[1])
    } else {
        ("public", table)
    };

    let raw = exec_query(
        config,
        "SELECT column_name, data_type, is_nullable, column_default \
         FROM information_schema.columns \
         WHERE table_schema = $1 AND table_name = $2 \
         ORDER BY ordinal_position",
        &[&schema, &tbl],
    )?;
    let rows: Vec<Map<String, Value>> = raw.iter().map(row_to_json).collect();
    Ok(QueryResponse::from_rows(rows))
}
