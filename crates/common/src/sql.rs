use crate::error::{Result, ToolkitError};
use serde::Serialize;
use serde_json::{Map, Value};

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
/// Returns a `WriteDenied` error if not.
pub fn assert_write_allowed(writable_tables: Option<&Vec<String>>, table: &str) -> Result<()> {
    let allowed = match writable_tables {
        Some(list) if !list.is_empty() => list,
        _ => return Err(ToolkitError::write_denied(format!("write to '{}' denied", table))),
    };

    let normalised = strip_schema(table).to_lowercase();
    if !allowed
        .iter()
        .any(|t| strip_schema(t).to_lowercase() == normalised)
    {
        return Err(ToolkitError::write_denied(format!(
            "write to '{}' denied",
            table
        )));
    }
    Ok(())
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
        assert!(assert_write_allowed(Some(&tables), "orders").is_ok());
        assert!(assert_write_allowed(Some(&tables), "public.orders").is_ok());
    }

    #[test]
    fn test_assert_write_allowed_denies_when_empty() {
        let tables: Vec<String> = vec![];
        match assert_write_allowed(Some(&tables), "orders") {
            Err(ToolkitError::WriteDenied(_)) => {}
            other => panic!("expected WriteDenied, got {:?}", other),
        }
    }

    #[test]
    fn test_assert_write_allowed_denies_when_none() {
        match assert_write_allowed(None, "orders") {
            Err(ToolkitError::WriteDenied(_)) => {}
            other => panic!("expected WriteDenied, got {:?}", other),
        }
    }

    #[test]
    fn test_assert_write_allowed_denies_table_not_in_list() {
        let tables = vec!["orders".into()];
        match assert_write_allowed(Some(&tables), "users") {
            Err(ToolkitError::WriteDenied(_)) => {}
            other => panic!("expected WriteDenied, got {:?}", other),
        }
    }
}
