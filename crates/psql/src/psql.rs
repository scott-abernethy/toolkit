use common::exit_with_error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: Option<String>,
}

pub fn load_config() -> Config {
    common::load_section::<Config>("psql")
}

// ---------------------------------------------------------------------------
// Query execution via psql
// ---------------------------------------------------------------------------

/// Wraps the user query in a read-only transaction.
fn readonly_sql(sql: &str) -> String {
    // Strip trailing semicolons/whitespace so we can wrap cleanly
    let trimmed = sql.trim().trim_end_matches(';').trim();
    format!(
        "BEGIN TRANSACTION READ ONLY;\n{trimmed};\nCOMMIT;"
    )
}

/// Runs psql and returns the raw stdout as a string.
fn exec_psql(config: &Config, sql: &str) -> String {
    let wrapped = readonly_sql(sql);

    let mut cmd = Command::new("psql");
    cmd.arg("-h").arg(&config.host)
        .arg("-p").arg(config.port.to_string())
        .arg("-U").arg(&config.user)
        .arg("-d").arg(&config.database)
        .arg("--no-psqlrc")
        .arg("--csv")
        .arg("-c").arg(&wrapped);

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

pub fn run_query(config: &Config, sql: &str) {
    let raw = exec_psql(config, sql);
    // psql --csv output with BEGIN/COMMIT will have "BEGIN" and "COMMIT" lines
    let csv_part = extract_csv(&raw);
    let rows = csv_to_json(&csv_part);
    let count = rows.len();
    let resp = QueryResponse { rows, count };
    println!("{}", serde_json::to_string(&resp).unwrap());
}

pub fn list_tables(config: &Config, schema: &str) {
    let sql = format!(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = '{}' ORDER BY table_name",
        schema.replace('\'', "''")
    );
    run_query(config, &sql);
}

pub fn describe_table(config: &Config, table: &str) {
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

/// Extract the CSV portion from psql output, skipping BEGIN/COMMIT markers.
fn extract_csv(raw: &str) -> String {
    raw.lines()
        .filter(|line| {
            let l = line.trim();
            l != "BEGIN" && l != "COMMIT"
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_readonly_sql_wraps_query() {
        let sql = "SELECT * FROM users;";
        let wrapped = readonly_sql(sql);
        assert!(wrapped.starts_with("BEGIN TRANSACTION READ ONLY;"));
        assert!(wrapped.contains("SELECT * FROM users;"));
        assert!(wrapped.ends_with("COMMIT;"));
    }

    #[test]
    fn test_readonly_sql_strips_trailing_semicolons() {
        let sql = "SELECT 1;;;  ";
        let wrapped = readonly_sql(sql);
        assert!(wrapped.contains("SELECT 1;"));
        // Should not have triple semicolons
        assert!(!wrapped.contains(";;;"));
    }

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
    fn test_extract_csv_strips_markers() {
        let raw = "BEGIN\nname,age\nAlice,30\nCOMMIT\n";
        let csv = extract_csv(raw);
        assert_eq!(csv, "name,age\nAlice,30");
    }
}
