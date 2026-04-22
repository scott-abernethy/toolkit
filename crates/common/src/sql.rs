use crate::exit_with_error;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Write-permission guard
// ---------------------------------------------------------------------------

/// Strips any schema prefix from a table name: `public.foo` → `foo`.
pub fn strip_schema(name: &str) -> &str {
    name.rfind('.').map_or(name, |i| &name[i + 1..])
}

/// SQL keywords that indicate a write or DDL operation.
/// Each entry is the keyword followed by a space (or end of string for single-word commands).
const WRITE_KEYWORDS: &[&str] = &[
    "INSERT INTO ",
    "UPDATE ",
    "DELETE FROM ",
    "TRUNCATE TABLE ",
    "TRUNCATE ",
    "DROP TABLE ",
    "DROP INDEX ",
    "DROP VIEW ",
    "DROP PROCEDURE ",
    "DROP FUNCTION ",
    "DROP SCHEMA ",
    "DROP DATABASE ",
    "ALTER TABLE ",
    "ALTER INDEX ",
    "ALTER VIEW ",
    "ALTER PROCEDURE ",
    "ALTER FUNCTION ",
    "ALTER SCHEMA ",
    "ALTER DATABASE ",
    "CREATE TABLE ",
    "CREATE INDEX ",
    "CREATE VIEW ",
    "CREATE PROCEDURE ",
    "CREATE FUNCTION ",
    "CREATE SCHEMA ",
    "CREATE DATABASE ",
    "EXEC ",
    "EXECUTE ",
];

/// If `sql` is a write or DDL statement, returns the bare target table/object name.
/// Returns `None` for read-only statements.
pub fn detect_write_target(sql: &str) -> Option<String> {
    let upper = sql.to_uppercase();

    for keyword in WRITE_KEYWORDS {
        if let Some(pos) = upper.find(keyword) {
            let after = sql[pos + keyword.len()..].trim_start();
            let name: String = after
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != ';' && *c != '(')
                .collect();
            if !name.is_empty() {
                return Some(strip_schema(&name).to_lowercase());
            }
        }
    }

    None
}

/// Checks whether a write to `table` is permitted by the given allowlist.
/// Exits with an error if not.
pub fn assert_write_allowed(writable_tables: Option<&Vec<String>>, table: &str) {
    let allowed = match writable_tables {
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
// Shared response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct QueryResponse {
    pub rows: Vec<Map<String, Value>>,
    pub count: usize,
}

impl QueryResponse {
    pub fn from_rows(rows: Vec<Map<String, Value>>) -> Self {
        let count = rows.len();
        Self { rows, count }
    }

    pub fn print(self) {
        println!("{}", serde_json::to_string(&self).unwrap());
    }
}

// ---------------------------------------------------------------------------
// Config loading
// ---------------------------------------------------------------------------

/// Load a named connection from a config section.
/// If `conn` is None and exactly one connection is configured, that one is used.
/// If `conn` is None and multiple connections are configured, exits with an error
/// listing the available names.
pub fn load_named_config<T: DeserializeOwned>(section: &str, conn: Option<&str>) -> T {
    let mut configs = crate::load_section::<HashMap<String, T>>(section);

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

fn sorted_keys<T>(map: &HashMap<String, T>) -> Vec<String> {
    let mut keys: Vec<String> = map.keys().cloned().collect();
    keys.sort();
    keys
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- detect_write_target --

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
    fn test_detect_write_drop_table() {
        assert_eq!(
            detect_write_target("DROP TABLE orders"),
            Some("orders".to_string())
        );
    }

    #[test]
    fn test_detect_write_alter_table() {
        assert_eq!(
            detect_write_target("ALTER TABLE orders ADD COLUMN status TEXT"),
            Some("orders".to_string())
        );
    }

    #[test]
    fn test_detect_write_create_table() {
        assert_eq!(
            detect_write_target("CREATE TABLE orders (id INT)"),
            Some("orders".to_string())
        );
    }

    #[test]
    fn test_detect_write_exec() {
        assert_eq!(
            detect_write_target("EXEC sp_rename 'orders', 'orders_old'"),
            Some("sp_rename".to_string())
        );
    }

    // -- strip_schema --

    #[test]
    fn test_strip_schema() {
        assert_eq!(strip_schema("public.orders"), "orders");
        assert_eq!(strip_schema("orders"), "orders");
        assert_eq!(strip_schema("myschema.my_table"), "my_table");
    }

    // -- assert_write_allowed --

    #[test]
    fn test_assert_write_allowed_permits() {
        let tables = vec!["orders".into()];
        assert_write_allowed(Some(&tables), "orders");
        assert_write_allowed(Some(&tables), "public.orders");
    }

    // Note: assert_write_allowed denial cannot be tested directly because
    // exit_with_error calls process::exit(1), which terminates the process
    // rather than panicking.
}
