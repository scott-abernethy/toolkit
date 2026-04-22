---
name: tkmsql
description: MS SQL Server query tool for agents — query data safely with read-only defaults and optional write allowlists
compatibility: opencode
---

## What I do

- Query MS SQL Server databases configured in `~/.config/toolkit/config.yaml`
- List tables and schemas without needing to write raw SQL
- Describe table schemas to understand column names and types
- Execute SELECT queries safely (write and DDL operations rejected by default)
- Optionally permit writes to specific tables via configuration

## When to use me

Use this proactively when you need to:
- Explore database schemas and metadata
- Query data to understand what's in the database
- Validate data migrations or transformations
- Check application state by querying on-prem SQL Server instances

Do **not** use for:
- Manual schema migrations (ask the user instead)
- Large exploratory queries that might timeout
- Sensitive data extraction without user consent

## Usage

### List tables

```bash
# On the default (single configured) connection
tkmsql tables

# On a named connection
tkmsql --conn onprem tables
tkmsql --conn onprem tables --schema analytics
```

**Output:** Compact JSON with table names

```json
{"rows": [{"table_name": "users"}, {"table_name": "orders"}], "count": 2}
```

### Describe a table

```bash
tkmsql --conn onprem describe --table users
tkmsql --conn onprem describe --table dbo.users
```

**Output:** Column names, types, nullable, and defaults

```json
{"rows": [{"column_name": "id", "data_type": "int", "is_nullable": "NO", "column_default": null}], "count": 1}
```

### Run a query

```bash
# Simple SELECT
tkmsql --conn onprem query --sql "SELECT TOP 10 id, name FROM users"

# With WHERE clause
tkmsql --conn onprem query --sql "SELECT * FROM orders WHERE status = 'pending'"
```

**Output:** Rows as compact JSON

```json
{"rows": [{"id": 1, "name": "Alice"}, {"id": 2, "name": "Bob"}], "count": 2}
```

## Notes

- Default schema is `dbo` (unlike PostgreSQL's `public`)
- Use `SELECT TOP N` instead of `LIMIT N` for row limits
- Write operations (INSERT, UPDATE, DELETE), DDL (DROP, ALTER, CREATE), and EXEC are rejected unless the target table is in the `writable_tables` allowlist
- For strongest read-only protection, configure the SQL login with only the `db_datareader` role
