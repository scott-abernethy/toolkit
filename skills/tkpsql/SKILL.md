---
name: tkpsql
description: PostgreSQL query tool for agents — query data safely with read-only defaults and optional write allowlists
compatibility: opencode
---

## What I do

- Query PostgreSQL databases configured in `~/.config/toolkit/config.toml`
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
{"tables": ["users", "orders", "products"], "count": 3}
```

### Describe a table

```bash
tkpsql --conn staging describe --table users
tkpsql --conn staging describe --table analytics.fact_orders
```

**Output:** Column names, types, nullable, and defaults

```json
{
  "columns": [
    {"name": "id", "type": "bigint", "nullable": false},
    {"name": "email", "type": "text", "nullable": false},
    {"name": "created_at", "type": "timestamp", "nullable": true}
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
