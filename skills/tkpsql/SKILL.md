---
name: tkpsql
description: PostgreSQL query tool for agents — query data safely with read-only defaults and optional write allowlists
compatibility: opencode
---

## What I do

- Query PostgreSQL databases via named connections configured in toolkit
- List tables and schemas without needing to write raw SQL
- Describe table schemas to understand column names and types
- Execute SELECT queries safely (write operations rejected by default)
- Optionally permit writes to specific tables via configuration

## When to use me

Use this proactively when you need to:
- Explore database schemas and metadata
- Query data to understand what's in the database
- Validate data migrations or transformations
- Check application state by querying production read-replicas

Do **not** use for:
- Manual schema migrations (ask the user instead)
- Large exploratory queries that might timeout
- Sensitive data extraction without user consent

## Usage

### List tables

```bash
# On the default (single configured) connection
tkpsql tables

# On a named connection
tkpsql --conn staging tables
tkpsql --conn staging tables --schema analytics
```

**Output:** Compact JSON with table names

```json
{"rows": [{"table_name": "orders"}, {"table_name": "products"}, {"table_name": "users"}], "count": 3}
```

### Describe a table

```bash
tkpsql --conn staging describe --table users
tkpsql --conn staging describe --table analytics.fact_orders
```

**Output:** Column names, types, nullable, and defaults

```json
{
  "rows": [
    {"column_name": "id", "data_type": "bigint", "is_nullable": "NO", "column_default": null},
    {"column_name": "email", "data_type": "text", "is_nullable": "NO", "column_default": null},
    {"column_name": "created_at", "data_type": "timestamp without time zone", "is_nullable": "YES", "column_default": null}
  ],
  "count": 3
}
```

### Run a query

```bash
# Simple SELECT
tkpsql --conn staging query --sql "SELECT id, name FROM users LIMIT 10"

# With WHERE clause
tkpsql --conn staging query --sql "SELECT * FROM orders WHERE status = 'pending' LIMIT 50"
```

**Output:** Rows as compact JSON

```json
{"rows": [{"id": "1", "name": "Alice"}, {"id": "2", "name": "Bob"}], "count": 2}
```
