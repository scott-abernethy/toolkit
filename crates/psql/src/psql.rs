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
        exit_with_error(format!("psql error: {}", stderr.trim()));
    }

    String::from_utf8_lossy(&output.stdout).into_owned()
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
}
