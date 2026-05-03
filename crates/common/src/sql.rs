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

/// Returns the bare target name (lowercased, schema stripped) for every
/// write or DDL statement detected in `sql`. Empty for read-only input.
///
/// Multi-statement input is walked in full so that
/// `"INSERT INTO allowed; DELETE FROM forbidden"` yields both targets.
pub fn detect_write_targets(sql: &str) -> Vec<String> {
    // ASCII-only uppercasing preserves byte positions: a position found in
    // `upper` is a valid char-boundary index into the original `sql`. Using
    // full Unicode `to_uppercase()` would change byte length for non-ASCII
    // identifiers and could panic when slicing back into `sql`.
    let mut upper = sql.to_string();
    upper.make_ascii_uppercase();
    let upper_bytes = upper.as_bytes();

    // Longest-prefix-wins scan so `"TRUNCATE TABLE foo"` matches only the
    // longer keyword and yields `"foo"`, not `"foo"` plus `"table"`.
    let mut keywords: Vec<&&str> = WRITE_KEYWORDS.iter().collect();
    keywords.sort_by_key(|k| std::cmp::Reverse(k.len()));

    let mut targets = Vec::new();
    let mut i = 0;
    while i < upper_bytes.len() {
        let mut advanced = false;
        for keyword in &keywords {
            let kw_bytes = keyword.as_bytes();
            if upper_bytes[i..].starts_with(kw_bytes) {
                let after_idx = i + kw_bytes.len();
                let after = sql[after_idx..].trim_start();
                let name: String = after
                    .chars()
                    .take_while(|c| !c.is_whitespace() && *c != ';' && *c != '(')
                    .collect();
                if !name.is_empty() {
                    targets.push(strip_schema(&name).to_lowercase());
                }
                i = after_idx;
                advanced = true;
                break;
            }
        }
        if !advanced {
            i += 1;
        }
    }
    targets
}

/// Checks whether a write to `table` is permitted by the given allowlist.
/// Returns a `WriteDenied` error if not.
pub fn assert_write_allowed(writable_tables: Option<&Vec<String>>, table: &str) -> Result<()> {
    let allowed = match writable_tables {
        Some(list) if !list.is_empty() => list,
        _ => {
            return Err(ToolkitError::write_denied(format!(
                "write to '{}' denied",
                table
            )))
        }
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

    // -- detect_write_targets --

    #[test]
    fn test_detect_write_insert() {
        assert_eq!(
            detect_write_targets("INSERT INTO public.orders VALUES (1)"),
            vec!["orders".to_string()]
        );
    }

    #[test]
    fn test_detect_write_insert_unqualified() {
        assert_eq!(
            detect_write_targets("insert into orders (id) values (1)"),
            vec!["orders".to_string()]
        );
    }

    #[test]
    fn test_detect_write_update() {
        assert_eq!(
            detect_write_targets("UPDATE public.orders SET status = 'done' WHERE id = 1"),
            vec!["orders".to_string()]
        );
    }

    #[test]
    fn test_detect_write_delete() {
        assert_eq!(
            detect_write_targets("DELETE FROM orders WHERE id = 1"),
            vec!["orders".to_string()]
        );
    }

    #[test]
    fn test_detect_write_truncate() {
        assert_eq!(
            detect_write_targets("TRUNCATE TABLE orders"),
            vec!["orders".to_string()]
        );
        assert_eq!(
            detect_write_targets("TRUNCATE orders"),
            vec!["orders".to_string()]
        );
    }

    #[test]
    fn test_detect_write_select_is_empty() {
        assert!(detect_write_targets("SELECT * FROM orders WHERE id = 1").is_empty());
    }

    #[test]
    fn test_detect_write_drop_table() {
        assert_eq!(
            detect_write_targets("DROP TABLE orders"),
            vec!["orders".to_string()]
        );
    }

    #[test]
    fn test_detect_write_alter_table() {
        assert_eq!(
            detect_write_targets("ALTER TABLE orders ADD COLUMN status TEXT"),
            vec!["orders".to_string()]
        );
    }

    #[test]
    fn test_detect_write_create_table() {
        assert_eq!(
            detect_write_targets("CREATE TABLE orders (id INT)"),
            vec!["orders".to_string()]
        );
    }

    #[test]
    fn test_detect_write_exec() {
        assert_eq!(
            detect_write_targets("EXEC sp_rename 'orders', 'orders_old'"),
            vec!["sp_rename".to_string()]
        );
    }

    #[test]
    fn test_detect_write_multi_statement() {
        // Both targets must be returned so the caller can authorise each.
        let targets = detect_write_targets(
            "INSERT INTO allowed VALUES (1); DELETE FROM forbidden WHERE id = 1",
        );
        assert!(targets.contains(&"allowed".to_string()));
        assert!(targets.contains(&"forbidden".to_string()));
    }

    #[test]
    fn test_detect_write_repeated_keyword() {
        let targets = detect_write_targets("INSERT INTO a VALUES (1); INSERT INTO b VALUES (2)");
        assert_eq!(targets, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn test_detect_write_non_ascii_identifier_no_panic() {
        // `to_uppercase()` on these chars used to change byte length and
        // panic when slicing back into the original string. ASCII-only
        // uppercasing avoids that.
        let _ = detect_write_targets("UPDATE café SET x = 1");
        let _ = detect_write_targets("INSERT INTO ünïçødé (id) VALUES (1)");
        let _ = detect_write_targets("-- ß is sharp s\nSELECT 1");
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
