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

### Query Tables

```bash
# Run a SQL query (uses warehouse_id from config)
tkdbr query --sql "SELECT * FROM my_catalog.my_schema.my_table"

# With explicit warehouse and row limit
tkdbr query --sql "SELECT id, name FROM catalog.schema.table WHERE status = 'active'" --warehouse-id 9f9919ede4d8f98d --limit 50

# Find your warehouse_id
tkdbr warehouses list
```

### Explore Unity Catalog

```bash
# List catalogs
tkdbr catalogs list [--limit 100]
tkdbr catalogs get --catalog my_catalog

# List schemas in a catalog
tkdbr schemas list --catalog my_catalog [--limit 100]
tkdbr schemas get --catalog my_catalog --schema analytics

# List tables in a schema
tkdbr tables list --catalog my_catalog --schema analytics [--limit 100]

# Get full table schema (columns, data types, descriptions)
tkdbr tables get --catalog my_catalog --schema analytics --table fact_orders

# Omit column definitions for lighter responses
tkdbr tables list --catalog my_catalog --schema analytics --omit-columns
```

### Query Jobs and Runs

```bash
# List all jobs
tkdbr jobs list [--limit 25]
tkdbr jobs get --job-id 123

# List recent runs for a job
tkdbr runs list --job-id 123 [--limit 10]
tkdbr runs get --run-id 456
tkdbr runs output --run-id 456

# Trigger a job run
tkdbr jobs trigger --job-id 123
```

### Inspect Clusters and Warehouses

```bash
# List clusters
tkdbr clusters list
tkdbr clusters get --cluster-id abc-123

# List SQL warehouses
tkdbr warehouses list
```

### Manage Databricks Bundles

Bundles are deployed workflows defined in YAML. The bundle target (e.g., `local`, `dev`, `prod`) is configured per connection.

```bash
# Validate the bundle (checks YAML syntax and references)
tkdbr bundle validate

# Deploy the bundle to the configured target
tkdbr bundle deploy

# Run a named resource from the bundle
tkdbr bundle run my-job
```

## Output Format

All commands return compact JSON optimized for token efficiency:

```json
{"rows": [...], "count": 5}
{"jobs": [...], "count": 10}
{"catalogs": [...], "count": 25}
{"ok": true}
```
