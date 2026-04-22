use common::exit_with_error;
use common::sql::{self, QueryResponse};
use serde::Deserialize;
use serde_json::{Map, Value};
use tiberius::{AuthMethod, Client, ColumnData, Config, EncryptionLevel};
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ConnConfig {
    pub host: String,
    pub port: Option<u16>,
    pub database: String,
    pub user: String,
    pub password: Option<String>,
    /// Use TLS for this connection (default: true).
    pub tls: Option<bool>,
    /// Trust the server certificate without validation (common for on-prem
    /// servers with self-signed certs). Default: false.
    pub trust_cert: Option<bool>,
    /// Tables the agent is permitted to write to (INSERT/UPDATE/DELETE/TRUNCATE).
    /// If absent or empty, the connection is treated as strictly read-only.
    pub writable_tables: Option<Vec<String>>,
}

impl ConnConfig {
    fn port(&self) -> u16 {
        self.port.unwrap_or(1433)
    }

    fn use_tls(&self) -> bool {
        self.tls.unwrap_or(true)
    }

    fn trust_cert(&self) -> bool {
        self.trust_cert.unwrap_or(false)
    }
}

pub fn load_config(conn: Option<&str>) -> ConnConfig {
    sql::load_named_config("msql", conn)
}

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

async fn connect(config: &ConnConfig) -> Client<tokio_util::compat::Compat<TcpStream>> {
    let mut cfg = Config::new();
    cfg.host(&config.host);
    cfg.port(config.port());
    cfg.database(&config.database);
    cfg.authentication(AuthMethod::sql_server(
        &config.user,
        config.password.as_deref().unwrap_or(""),
    ));

    if config.trust_cert() {
        cfg.trust_cert();
    }

    if !config.use_tls() {
        cfg.encryption(EncryptionLevel::NotSupported);
    }

    let addr = format!("{}:{}", config.host, config.port());
    let tcp = TcpStream::connect(&addr)
        .await
        .unwrap_or_else(|e| exit_with_error(sanitize_connect_error(&e)));

    tcp.set_nodelay(true).ok();

    Client::connect(cfg, tcp.compat_write())
        .await
        .unwrap_or_else(|e| exit_with_error(sanitize_tds_error(&e)))
}

// ---------------------------------------------------------------------------
// Error sanitisation
// ---------------------------------------------------------------------------

fn sanitize_connect_error(e: &std::io::Error) -> String {
    let msg = e.to_string().to_lowercase();
    if msg.contains("connection refused") {
        "connection refused".to_string()
    } else if msg.contains("timed out") {
        "connection timed out".to_string()
    } else {
        "connection failed".to_string()
    }
}

fn sanitize_tds_error(e: &tiberius::error::Error) -> String {
    let msg = e.to_string().to_lowercase();
    if msg.contains("login failed") || msg.contains("authentication") {
        "authentication failed".to_string()
    } else if msg.contains("cannot open database") {
        "database does not exist".to_string()
    } else if msg.contains("permission denied") || msg.contains("not allowed") {
        "permission denied".to_string()
    } else if msg.contains("ssl") || msg.contains("tls") {
        "ssl error".to_string()
    } else {
        // Take only the first line to avoid leaking verbose error details.
        e.to_string()
            .lines()
            .next()
            .unwrap_or("query error")
            .to_string()
    }
}

// ---------------------------------------------------------------------------
// Type → JSON mapping
// ---------------------------------------------------------------------------

fn cell_to_json(col: &ColumnData<'_>) -> Value {
    match col {
        ColumnData::U8(v) => v.map(|n| Value::Number(n.into())).unwrap_or(Value::Null),
        ColumnData::I16(v) => v.map(|n| Value::Number(n.into())).unwrap_or(Value::Null),
        ColumnData::I32(v) => v.map(|n| Value::Number(n.into())).unwrap_or(Value::Null),
        ColumnData::I64(v) => v.map(|n| Value::Number(n.into())).unwrap_or(Value::Null),
        ColumnData::F32(v) => v
            .and_then(|n| serde_json::Number::from_f64(n as f64).map(Value::Number))
            .unwrap_or(Value::Null),
        ColumnData::F64(v) => v
            .and_then(|n| serde_json::Number::from_f64(n).map(Value::Number))
            .unwrap_or(Value::Null),
        ColumnData::Bit(v) => v.map(Value::Bool).unwrap_or(Value::Null),
        ColumnData::String(v) => v
            .as_deref()
            .map(|s| Value::String(s.to_string()))
            .unwrap_or(Value::Null),
        ColumnData::Numeric(v) => v
            .map(|n| Value::String(n.to_string()))
            .unwrap_or(Value::Null),
        ColumnData::Guid(v) => v
            .map(|g| Value::String(g.to_string()))
            .unwrap_or(Value::Null),
        ColumnData::Binary(v) => v
            .as_deref()
            .map(|b| Value::String(hex::encode(b)))
            .unwrap_or(Value::Null),
        ColumnData::Xml(v) => v
            .as_deref()
            .map(|x| Value::String(x.to_string()))
            .unwrap_or(Value::Null),
        ColumnData::DateTime(v) => v
            .map(|dt| {
                let date = chrono::NaiveDate::from_ymd_opt(1900, 1, 1).unwrap()
                    + chrono::Duration::days(dt.days() as i64);
                let time = chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap()
                    + chrono::Duration::nanoseconds(
                        dt.seconds_fragments() as i64 * (1_000_000_000 / 300),
                    );
                Value::String(chrono::NaiveDateTime::new(date, time).to_string())
            })
            .unwrap_or(Value::Null),
        ColumnData::SmallDateTime(v) => v
            .map(|dt| {
                let date = chrono::NaiveDate::from_ymd_opt(1900, 1, 1).unwrap()
                    + chrono::Duration::days(dt.days() as i64);
                let time = chrono::NaiveTime::from_num_seconds_from_midnight_opt(
                    dt.seconds_fragments() as u32 * 60,
                    0,
                )
                .unwrap();
                Value::String(chrono::NaiveDateTime::new(date, time).to_string())
            })
            .unwrap_or(Value::Null),
        ColumnData::Time(v) => v
            .map(|t| {
                let ns = t.increments() as i64 * 10i64.pow(9 - t.scale() as u32);
                let time = chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap()
                    + chrono::Duration::nanoseconds(ns);
                Value::String(time.to_string())
            })
            .unwrap_or(Value::Null),
        ColumnData::Date(v) => v
            .map(|d| {
                let date = chrono::NaiveDate::from_ymd_opt(1, 1, 1).unwrap()
                    + chrono::Duration::days(d.days() as i64);
                Value::String(date.to_string())
            })
            .unwrap_or(Value::Null),
        ColumnData::DateTime2(v) => v
            .map(|dt| {
                let date = chrono::NaiveDate::from_ymd_opt(1, 1, 1).unwrap()
                    + chrono::Duration::days(dt.date().days() as i64);
                let ns = dt.time().increments() as i64
                    * 10i64.pow(9 - dt.time().scale() as u32);
                let time = chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap()
                    + chrono::Duration::nanoseconds(ns);
                Value::String(chrono::NaiveDateTime::new(date, time).to_string())
            })
            .unwrap_or(Value::Null),
        ColumnData::DateTimeOffset(v) => v
            .map(|dto| {
                let dt2 = dto.datetime2();
                let date = chrono::NaiveDate::from_ymd_opt(1, 1, 1).unwrap()
                    + chrono::Duration::days(dt2.date().days() as i64);
                let ns = dt2.time().increments() as i64
                    * 10i64.pow(9 - dt2.time().scale() as u32);
                let time = chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap()
                    + chrono::Duration::nanoseconds(ns);
                let naive = chrono::NaiveDateTime::new(date, time);
                let offset =
                    chrono::FixedOffset::east_opt(dto.offset() as i32 * 60).unwrap();
                let dt = chrono::DateTime::<chrono::FixedOffset>::from_naive_utc_and_offset(
                    naive, offset,
                );
                Value::String(dt.to_rfc3339())
            })
            .unwrap_or(Value::Null),
    }
}

// ---------------------------------------------------------------------------
// Query execution
// ---------------------------------------------------------------------------

async fn exec_query(config: &ConnConfig, sql: &str) -> Vec<Map<String, Value>> {
    let mut client = connect(config).await;
    let stream = client
        .simple_query(sql)
        .await
        .unwrap_or_else(|e| exit_with_error(sanitize_tds_error(&e)));

    let rows = stream
        .into_first_result()
        .await
        .unwrap_or_else(|e| exit_with_error(sanitize_tds_error(&e)));

    rows.iter()
        .map(|row| {
            row.cells()
                .map(|(col, data)| (col.name().to_string(), cell_to_json(data)))
                .collect()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Subcommand implementations
// ---------------------------------------------------------------------------

pub async fn run_query(config: &ConnConfig, sql: &str) {
    if let Some(table) = sql::detect_write_target(sql) {
        sql::assert_write_allowed(config.writable_tables.as_ref(), &table);
    }
    let rows = exec_query(config, sql).await;
    QueryResponse::from_rows(rows).print();
}

pub async fn list_tables(config: &ConnConfig, schema: &str) {
    let sql = format!(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = '{}' ORDER BY table_name",
        schema.replace('\'', "''")
    );
    run_query(config, &sql).await;
}

pub async fn describe_table(config: &ConnConfig, table: &str) {
    let (schema, tbl) = if table.contains('.') {
        let parts: Vec<&str> = table.splitn(2, '.').collect();
        (parts[0], parts[1])
    } else {
        ("dbo", table)
    };

    let sql = format!(
        "SELECT column_name, data_type, is_nullable, column_default \
         FROM information_schema.columns \
         WHERE table_schema = '{}' AND table_name = '{}' \
         ORDER BY ordinal_position",
        schema.replace('\'', "''"),
        tbl.replace('\'', "''")
    );
    run_query(config, &sql).await;
}
