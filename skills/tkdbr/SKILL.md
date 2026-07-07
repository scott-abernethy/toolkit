---
name: tkdbr
description: Databricks CLI wrapper for querying tables, exploring Unity Catalog metadata, and managing jobs/clusters/bundles
compatibility: opencode
---

## What I do

- Execute SQL queries against Databricks SQL warehouses
- Query Databricks Unity Catalog (catalogs, schemas, tables, columns)
- List and inspect jobs, runs, clusters, and SQL warehouses
- Trigger job runs (with explicit permission via config)
- Validate, deploy, and run Databricks bundles
- All read operations are safe by default; write operations require explicit opt-in

## When to use me

Use this proactively when you need to:
- Run SQL queries against Databricks tables
- Explore data assets in Unity Catalog
- Understand job and workflow definitions
- Check cluster status and configurations
- Validate or deploy bundle-based workflows
- Query job history and run outputs

Do **not** use for:
- Creating or modifying jobs directly (editing YAML is better)
- Cluster or warehouse provisioning (ask the user instead)
- Very large data exports (results are capped by --limit; use Databricks UI for bulk exports)

## Usage

## Connections

When multiple connections are configured (e.g. `dev`, `tst`, `prod`), pass `--conn <conn>` to every command.
The flag is `--conn`, NOT `--profile`.

```bash
# Select a named connection
tkdbr query --conn <conn> --sql "..."
tkdbr catalogs list --conn <conn>
tkdbr schemas list --conn <conn> --catalog my_catalog
```

If only one connection is configured, `--conn` can be omitted.

### Query Tables

SQL table references **must be fully qualified** as `<catalog>.<schema>.<table>`.
Schema-only names like `my_schema.my_table` will fail. Always discover the real catalog name first
(see "Explore Unity Catalog" below) unless you already know it.

```bash
# Run a SQL query — always use fully-qualified three-part names
tkdbr query --conn <conn> --sql "SELECT * FROM my_catalog.my_schema.my_table LIMIT 5"

# With explicit warehouse and row limit
tkdbr query --conn dev --sql "SELECT id, name FROM my_catalog.my_schema.my_table WHERE status = 'active'" --warehouse-id 9f9919ede4d8f98d --limit 50

# Find your warehouse_id
tkdbr warehouses list --conn <conn>
```

### Explore Unity Catalog

When the catalog name is unknown, list catalogs first to find the correct name before querying.

```bash
# 1. Discover catalogs (do this first when catalog name is unknown)
tkdbr catalogs list --conn <conn>
tkdbr catalogs get --conn <conn> --catalog my_catalog

# 2. List schemas in a catalog
tkdbr schemas list --conn <conn> --catalog my_catalog [--limit 100]
tkdbr schemas get --conn <conn> --catalog my_catalog --schema my_schema

# 3. List tables in a schema
tkdbr tables list --conn <conn> --catalog my_catalog --schema my_schema [--limit 100]

# Get full table schema (columns, data types, descriptions)
tkdbr tables get --conn <conn> --catalog my_catalog --schema my_schema --table my_table

# Omit column definitions for lighter responses
tkdbr tables list --conn <conn> --catalog my_catalog --schema my_schema --omit-columns
```

### Query Jobs and Runs

```bash
# List all jobs
tkdbr jobs list --conn <conn> [--limit 25]
tkdbr jobs get --conn <conn> --job-id 123

# List recent runs for a job (Databricks-style)
tkdbr jobs list-runs --conn <conn> --job-id 123 [--limit 10]
tkdbr jobs get-run --conn <conn> --run-id 456
tkdbr jobs get-run-output --conn <conn> --run-id 456

# Legacy aliases (still supported)
tkdbr runs list --conn <conn> --job-id 123 [--limit 10]
tkdbr runs get --conn <conn> --run-id 456
tkdbr runs output --conn <conn> --run-id 456

# Trigger a job run
tkdbr jobs trigger --conn <conn> --job-id 123
```

### Inspect Clusters and Warehouses

```bash
# List clusters
tkdbr clusters list --conn <conn>
tkdbr clusters get --conn <conn> --cluster-id abc-123

# List SQL warehouses
tkdbr warehouses list --conn <conn>
```

### Manage Databricks Bundles

Bundles are deployed workflows defined in YAML. The bundle target (e.g., `local`, `dev`, `prod`) is configured per connection.

```bash
# Validate the bundle (checks YAML syntax and references)
tkdbr bundle validate --conn <conn>

# Deploy the bundle to the configured target
tkdbr bundle deploy --conn <conn>

# Force-deploy, overriding a remote-modified resource (e.g. a dashboard edited outside the bundle)
tkdbr bundle deploy --conn <conn> --force

# Run a named resource from the bundle
tkdbr bundle run my-job --conn <conn>

# Destroy deployed bundle resources on the configured target
tkdbr bundle destroy --conn <conn>
```

## Output Format

All commands return compact JSON optimized for token efficiency:

```json
{"rows": [...], "count": 5}
{"jobs": [...], "count": 10}
{"catalogs": [...], "count": 25}
{"ok": true}
```
