use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use common::exit_with_error;
use native_tls::TlsConnector;
use postgres::types::Type;
use postgres_native_tls::MakeTlsConnector;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use uuid::Uuid;

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
    fn is_readonly(&self) -> bool {
        self.writable_tables
            .as_ref()
            .map_or(true, |t| t.is_empty())
    }

    fn use_tls(&self) -> bool {
        self.tls.unwrap_or(false)
    }
}

/// Load a named connection from the [psql] section of the shared config.
/// If `conn` is None and exactly one connection is configured, that one is used.
/// If `conn` is None and multiple connections are configured, exits with an error
/// listing the available names.
pub fn load_config(conn: Option<&str>) -> ConnConfig {
    let mut configs = common::load_section::<HashMap<String, ConnConfig>>("psql");

    match conn {
        Some(name) => configs.remove(name).unwrap_or_else(|| {
            let available = sorted_keys(&configs);
            exit_with_error(format!(
                "Unknown connection '{}'. Available: {}",
                name,
                available.join(", ")
            ))
        }),
        None => {
            if configs.len() == 1 {
                configs.into_values().next().unwrap()
            } else {
                let available = sorted_keys(&configs);
                exit_with_error(format!(
                    "Multiple connections configured, specify --conn. Available: {}",
                    available.join(", ")
                ))
            }
        }
    }
}

fn sorted_keys(map: &HashMap<String, ConnConfig>) -> Vec<String> {
    let mut keys: Vec<String> = map.keys().cloned().collect();
    keys.sort();
    keys
}

// ---------------------------------------------------------------------------
// Write-permission guard
// ---------------------------------------------------------------------------

/// Strips any schema prefix from a table name: `public.foo` → `foo`.
fn strip_schema(name: &str) -> &str {
    name.rfind('.').map_or(name, |i| &name[i + 1..])
}

/// If `sql` is a write statement, returns the bare target table name.
/// Returns `None` for read-only statements.
fn detect_write_target(sql: &str) -> Option<String> {
    let upper = sql.to_uppercase();

    let keywords: &[&str] = &[
        "INSERT INTO ",
        "UPDATE ",
        "DELETE FROM ",
        "TRUNCATE TABLE ",
        "TRUNCATE ",
    ];

    for keyword in keywords {
        if let Some(pos) = upper.find(keyword) {
            let after = sql[pos + keyword.len()..].trim_start();
            let table: String = after
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != ';' && *c != '(')
                .collect();
            if !table.is_empty() {
                return Some(strip_schema(&table).to_lowercase());
            }
        }
    }

    None
}

/// Checks whether a write to `table` is permitted by the config.
/// Exits with an error if not.
fn assert_write_allowed(config: &ConnConfig, table: &str) {
    let allowed = match &config.writable_tables {
        Some(list) if !list.is_empty() => list,
        _ => exit_with_error(format!("write to '{}' denied", table)),
    };

    let normalised = strip_schema(table).to_lowercase();
    if !allowed
        .iter()
        .any(|t| strip_schema(t).to_lowercase() == normalised)
    {
        exit_with_error(format!("write to '{}' denied", table));
    }
}

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

fn connect(config: &ConnConfig) -> postgres::Client {
    let mut cfg = postgres::Config::new();
    cfg.host(&config.host)
        .port(config.port)
        .dbname(&config.database)
        .user(&config.user);

    if let Some(pw) = &config.password {
        cfg.password(pw);
    }

    // Enforce read-only at the session level for connections with no write tables.
    if config.is_readonly() {
        cfg.options("-c default_transaction_read_only=on");
    }

    if config.use_tls() {
        let tls = TlsConnector::builder()
            .build()
            .unwrap_or_else(|e| exit_with_error(format!("tls error: {}", e)));
        let connector = MakeTlsConnector::new(tls);
        cfg.connect(connector)
            .unwrap_or_else(|e| exit_with_error(sanitize_pg_error(&e)))
    } else {
        cfg.connect(postgres::NoTls)
            .unwrap_or_else(|e| exit_with_error(sanitize_pg_error(&e)))
    }
}

// ---------------------------------------------------------------------------
// Error sanitisation
// ---------------------------------------------------------------------------

fn sanitize_pg_error(e: &postgres::Error) -> String {
    let msg = e.to_string().to_lowercase();
    if msg.contains("password authentication failed") || msg.contains("authentication failed") {
        "authentication failed".to_string()
    } else if msg.contains("timeout") || msg.contains("timed out") {
        "connection timed out".to_string()
    } else if msg.contains("connection refused") || msg.contains("connection to server") {
        "connection refused".to_string()
    } else if msg.contains("database") && msg.contains("does not exist") {
        "database does not exist".to_string()
    } else if msg.contains("role") && msg.contains("does not exist") {
        "role does not exist".to_string()
    } else if msg.contains("permission denied") || msg.contains("insufficient privilege") {
        "permission denied".to_string()
    } else if msg.contains("ssl") || msg.contains("tls") {
        "ssl error".to_string()
    } else if let Some(db_err) = e.as_db_error() {
        db_err.message().to_string()
    } else {
        "query error".to_string()
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

fn exec_query(config: &ConnConfig, sql: &str) -> Vec<postgres::Row> {
    let mut client = connect(config);
    client
        .query(sql, &[])
        .unwrap_or_else(|e| exit_with_error(sanitize_pg_error(&e)))
}

// ---------------------------------------------------------------------------
// Subcommand implementations
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct QueryResponse {
    rows: Vec<Map<String, Value>>,
    count: usize,
}

pub fn run_query(config: &ConnConfig, sql: &str) {
    if let Some(table) = detect_write_target(sql) {
        assert_write_allowed(config, &table);
    }
    let raw = exec_query(config, sql);
    let rows: Vec<Map<String, Value>> = raw.iter().map(row_to_json).collect();
    let count = rows.len();
    let resp = QueryResponse { rows, count };
    println!("{}", serde_json::to_string(&resp).unwrap());
}

pub fn list_tables(config: &ConnConfig, schema: &str) {
    let sql = format!(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = '{}' ORDER BY table_name",
        schema.replace('\'', "''")
    );
    run_query(config, &sql);
}

pub fn describe_table(config: &ConnConfig, table: &str) {
    let (schema, tbl) = if table.contains('.') {
        let parts: Vec<&str> = table.splitn(2, '.').collect();
        (parts[0], parts[1])
    } else {
        ("public", table)
    };

    let sql = format!(
        "SELECT column_name, data_type, is_nullable, column_default \
         FROM information_schema.columns \
         WHERE table_schema = '{}' AND table_name = '{}' \
         ORDER BY ordinal_position",
        schema.replace('\'', "''"),
        tbl.replace('\'', "''")
    );
    run_query(config, &sql);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_write_insert() {
        assert_eq!(
            detect_write_target("INSERT INTO public.orders VALUES (1)"),
            Some("orders".to_string())
        );
    }

    #[test]
    fn test_detect_write_insert_unqualified() {
        assert_eq!(
            detect_write_target("insert into orders (id) values (1)"),
            Some("orders".to_string())
        );
    }

    #[test]
    fn test_detect_write_update() {
        assert_eq!(
            detect_write_target("UPDATE public.orders SET status = 'done' WHERE id = 1"),
            Some("orders".to_string())
        );
    }

    #[test]
    fn test_detect_write_delete() {
        assert_eq!(
            detect_write_target("DELETE FROM orders WHERE id = 1"),
            Some("orders".to_string())
        );
    }

    #[test]
    fn test_detect_write_truncate() {
        assert_eq!(
            detect_write_target("TRUNCATE TABLE orders"),
            Some("orders".to_string())
        );
        assert_eq!(
            detect_write_target("TRUNCATE orders"),
            Some("orders".to_string())
        );
    }

    #[test]
    fn test_detect_write_select_is_none() {
        assert_eq!(
            detect_write_target("SELECT * FROM orders WHERE id = 1"),
            None
        );
    }

    #[test]
    fn test_assert_write_allowed_permits() {
        let config = ConnConfig {
            host: "h".into(),
            port: 5432,
            database: "d".into(),
            user: "u".into(),
            password: None,
            tls: None,
            writable_tables: Some(vec!["orders".into()]),
        };
        assert_write_allowed(&config, "orders");
        assert_write_allowed(&config, "public.orders");
    }

    #[test]
    fn test_strip_schema() {
        assert_eq!(strip_schema("public.orders"), "orders");
        assert_eq!(strip_schema("orders"), "orders");
        assert_eq!(strip_schema("myschema.my_table"), "my_table");
    }
}
