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
tkdbr --conn dev query --sql "SELECT * FROM my_catalog.my_schema.my_table"

# With explicit warehouse and row limit
tkdbr --conn dev query --sql "SELECT id, name FROM catalog.schema.table WHERE status = 'active'" --warehouse-id 9f9919ede4d8f98d --limit 50

# LIMIT is auto-appended (default 100) if not present in the SQL
tkdbr --conn dev query --sql "SELECT count(*) FROM catalog.schema.table" --limit 1

# Find your warehouse_id
tkdbr --conn dev warehouses list
```

Query output is compact: `{"columns":["id","name"],"rows":[["1","alice"],["2","bob"]],"count":2}`

Long-running queries are polled automatically (up to 2 minutes).

### Explore Unity Catalog

```bash
# List catalogs
tkdbr --conn prod catalogs list [--limit 100]
tkdbr --conn prod catalogs get --catalog my_catalog

# List schemas in a catalog
tkdbr --conn prod schemas list --catalog my_catalog [--limit 100]
tkdbr --conn prod schemas get --catalog my_catalog --schema analytics

# List tables in a schema
tkdbr --conn prod tables list --catalog my_catalog --schema analytics [--limit 100]

# Get full table schema (columns, data types, descriptions)
tkdbr --conn prod tables get --catalog my_catalog --schema analytics --table fact_orders

# Omit column definitions for lighter responses
tkdbr --conn prod tables list --catalog my_catalog --schema analytics --omit-columns
```

### Query Jobs and Runs

```bash
# List all jobs
tkdbr --conn prod jobs list [--limit 25]
tkdbr --conn prod jobs get --job-id 123

# List recent runs for a job
tkdbr --conn prod runs list --job-id 123 [--limit 10]
tkdbr --conn prod runs get --run-id 456
tkdbr --conn prod runs output --run-id 456

# Trigger a job run (requires allow_job_runs = true in config)
tkdbr --conn prod jobs trigger --job-id 123
```

### Inspect Clusters and Warehouses

```bash
# List clusters
tkdbr --conn prod clusters list
tkdbr --conn prod clusters get --cluster-id abc-123

# List SQL warehouses
tkdbr --conn prod warehouses list
tkdbr --conn prod warehouses get --warehouse-id xyz-789
```

### Manage Databricks Bundles

Bundles are deployed workflows defined in YAML. The bundle target (e.g., `local`, `dev`, `prod`) is configured per connection.

```bash
# Validate the bundle (checks YAML syntax and references)
tkdbr --conn dev bundle validate

# Deploy the bundle to the configured target
tkdbr --conn dev bundle deploy

# Run a named resource from the bundle
tkdbr --conn dev bundle run my-job
tkdbr --conn dev bundle run ml-pipeline
```

## Output Format

All commands return compact JSON optimized for token efficiency:

```json
{"rows": [...], "count": 5}
{"jobs": [...], "count": 10}
{"catalogs": [...], "count": 25}
{"ok": true}
```
