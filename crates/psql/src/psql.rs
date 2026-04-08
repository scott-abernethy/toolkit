use common::exit_with_error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;

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
// Query execution via psql
// ---------------------------------------------------------------------------

/// Runs psql and returns the raw stdout as a string.
fn exec_psql(config: &ConnConfig, sql: &str) -> String {
    let mut cmd = Command::new("psql");
    cmd.arg("-h").arg(&config.host)
        .arg("-p").arg(config.port.to_string())
        .arg("-U").arg(&config.user)
        .arg("-d").arg(&config.database)
        .arg("--no-psqlrc")
        .arg("--csv")
        .arg("-c").arg(sql)
        .env("PGOPTIONS", "-c default_transaction_read_only=on");

    if let Some(pw) = &config.password {
        cmd.env("PGPASSWORD", pw);
    }

    let output = cmd.output().unwrap_or_else(|e| {
        exit_with_error(format!("Failed to execute psql: {}", e))
    });

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        exit_with_error(sanitize_psql_error(&stderr));
    }

    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Classify a raw psql stderr message into a concise, credential-safe error string.
fn sanitize_psql_error(stderr: &str) -> String {
    let lower = stderr.to_lowercase();
    // More specific patterns first to avoid being swallowed by broad connection checks.
    if lower.contains("password authentication failed") || lower.contains("authentication failed") {
        "authentication failed".to_string()
    } else if lower.contains("timeout") || lower.contains("timed out") {
        "connection timed out".to_string()
    } else if lower.contains("connection refused") || lower.contains("connection to server") {
        "connection refused".to_string()
    } else if lower.contains("database") && lower.contains("does not exist") {
        "database does not exist".to_string()
    } else if lower.contains("role") && lower.contains("does not exist") {
        "role does not exist".to_string()
    } else if lower.contains("permission denied") || lower.contains("insufficient privilege") {
        "permission denied".to_string()
    } else if lower.contains("ssl") {
        "ssl error".to_string()
    } else {
        // Emit only the first line to avoid multi-line noise; strip any
        // host/port fragments that might appear in unknown messages.
        let first_line = stderr.lines().next().unwrap_or("unknown error");
        // Remove substrings matching common connection detail patterns.
        let cleaned = regex_strip_conn_details(first_line);
        format!("psql error: {}", cleaned.trim())
    }
}

/// Best-effort removal of host/port fragments from an arbitrary psql error line.
/// Strips patterns like `at "hostname"`, `(address)`, and `, port NNNN`.
fn regex_strip_conn_details(s: &str) -> String {
    // We avoid pulling in the `regex` crate — use a simple state-machine strip instead.
    // Patterns to remove: `at "..."`, `(...)`, `, port \d+`
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip `, port <digits>`
        if s[i..].starts_with(", port ") {
            i += 7;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            continue;
        }
        // Skip `at "..."`
        if s[i..].starts_with("at \"") {
            i += 4;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            if i < bytes.len() {
                i += 1; // closing quote
            }
            continue;
        }
        // Skip `(...)` — IP address groups
        if bytes[i] == b'(' {
            while i < bytes.len() && bytes[i] != b')' {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    // Collapse multiple spaces
    let mut result = String::with_capacity(out.len());
    let mut prev_space = false;
    for ch in out.chars() {
        if ch == ' ' {
            if !prev_space {
                result.push(ch);
            }
            prev_space = true;
        } else {
            result.push(ch);
            prev_space = false;
        }
    }
    result
}

// ---------------------------------------------------------------------------
// CSV → JSON conversion
// ---------------------------------------------------------------------------

fn csv_to_json(csv_text: &str) -> Vec<HashMap<String, String>> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(csv_text.as_bytes());

    let headers: Vec<String> = match reader.headers() {
        Ok(h) => h.iter().map(|s| s.to_string()).collect(),
        Err(_) => return vec![],
    };

    reader
        .records()
        .filter_map(|r| r.ok())
        .map(|record| {
            headers
                .iter()
                .enumerate()
                .map(|(i, h)| {
                    (h.clone(), record.get(i).unwrap_or("").to_string())
                })
                .collect()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Subcommand implementations
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct QueryResponse {
    rows: Vec<HashMap<String, String>>,
    count: usize,
}

pub fn run_query(config: &ConnConfig, sql: &str) {
    let raw = exec_psql(config, sql);
    let rows = csv_to_json(&raw);
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
    // Support optional schema qualification
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
    fn test_csv_to_json_basic() {
        let csv = "name,age\nAlice,30\nBob,25\n";
        let rows = csv_to_json(csv);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], "Alice");
        assert_eq!(rows[0]["age"], "30");
        assert_eq!(rows[1]["name"], "Bob");
    }

    #[test]
    fn test_csv_to_json_empty() {
        let csv = "";
        let rows = csv_to_json(csv);
        assert!(rows.is_empty());
    }

    #[test]
    fn test_sanitize_connection_refused() {
        let raw = "psql: error: connection to server at \"localhost\" (::1), port 5454 failed: Connection refused\n\tIs the server running on that host and accepting TCP/IP connections?\nconnection to server at \"localhost\" (127.0.0.1), port 5454 failed: Connection refused\n\tIs the server running on that host and accepting TCP/IP connections?";
        assert_eq!(sanitize_psql_error(raw), "connection refused");
    }

    #[test]
    fn test_sanitize_auth_failed() {
        let raw = "psql: error: connection to server at \"db.example.com\" (1.2.3.4), port 5432 failed: FATAL:  password authentication failed for user \"alice\"";
        assert_eq!(sanitize_psql_error(raw), "authentication failed");
    }

    #[test]
    fn test_sanitize_timeout() {
        let raw = "psql: error: connection to server timed out";
        assert_eq!(sanitize_psql_error(raw), "connection timed out");
    }

    #[test]
    fn test_sanitize_database_not_exist() {
        let raw = "psql: error: FATAL:  database \"mydb\" does not exist";
        assert_eq!(sanitize_psql_error(raw), "database does not exist");
    }

    #[test]
    fn test_strip_conn_details() {
        let s = "connection to server at \"localhost\" (127.0.0.1), port 5454 failed";
        let result = regex_strip_conn_details(s);
        assert!(!result.contains("localhost"));
        assert!(!result.contains("127.0.0.1"));
        assert!(!result.contains("5454"));
    }
}
