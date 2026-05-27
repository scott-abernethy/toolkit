# Usage

`tkpsql`, `tkmsql`, and `tkdbr` share one common flag: `--conn <name>` selects a named connection from config. Each CLI dispatches to `toolkit-daemon` over a UNIX socket. If the daemon isn't running, the call fails with a clear error rather than silently falling back. See [daemon.md](daemon.md) for setup.

## tkpsql

```sh
# List tables (only one connection configured)
tkpsql tables

# List tables on a named connection
tkpsql --conn prod tables

# List tables in a specific schema
tkpsql --conn prod tables --schema myschema

# Run a SQL query
tkpsql --conn local query --sql "SELECT id, name FROM users LIMIT 10"

# Describe a table's columns
tkpsql --conn prod describe --table users
tkpsql --conn prod describe --table myschema.users   # schema-qualified
```

By default all connections are strictly read-only — write statements are rejected at both the tool level and the PostgreSQL session level. To permit writes on specific tables, add a `writable_tables` list to the connection config (see [configuration](configuration.md)). Writes to any table not in that list are rejected by the tool before the query reaches the database.

Output is compact JSON:

```json
{"rows":[{"id":"1","name":"Alice"},{"id":"2","name":"Bob"}],"count":2}
```

## tkmsql

```sh
# List tables (only one connection configured)
tkmsql tables

# List tables on a named connection
tkmsql --conn onprem tables

# List tables in a specific schema
tkmsql --conn onprem tables --schema myschema

# Run a SQL query
tkmsql --conn onprem query --sql "SELECT TOP 10 id, name FROM users"

# Describe a table's columns
tkmsql --conn onprem describe --table users
tkmsql --conn onprem describe --table dbo.users   # schema-qualified
```

By default all connections are strictly read-only — write and DDL statements (INSERT, UPDATE, DELETE, DROP, ALTER, CREATE, EXEC) are rejected at the tool level before the query reaches the database. For strongest protection, use a SQL login with only the `db_datareader` role. To permit writes on specific tables, add a `writable_tables` list to the connection config (see [configuration](configuration.md)).

Output is compact JSON, same format as `tkpsql`:

```json
{"rows":[{"id":1,"name":"Alice"},{"id":2,"name":"Bob"}],"count":2}
```

## toolkit (Daemon Management)

The `toolkit` binary provides commands for managing the daemon and installing wrappers. These commands are blocked for AI agents.

```sh
# Show daemon status (socket path and reachability)
toolkit status

# Bootstrap harness protections (hooks/deny rules)
toolkit init --harness all --scope global

# Validate daemon + harness integration
toolkit validate

# Open the daemon config in $EDITOR (requires sudo)
toolkit config edit

# Show masked config (requires sudo)
toolkit config show

# Show config template for an app
toolkit config template psql

# Read or follow daemon error logs (requires sudo)
toolkit logs [--tail 50] [--follow]

# Generate guarded wrapper scripts (e.g. tkkubectl-dev)
toolkit install
```

## tkdbr

```sh
# Run a SQL query (uses warehouse_id from config)
tkdbr --conn dev query --sql "SELECT * FROM catalog.schema.table"

# With explicit warehouse and row limit
tkdbr --conn dev query --sql "SELECT id, name FROM catalog.schema.table" --warehouse-id abc --limit 50

# List all accessible catalogs
tkdbr --conn prod catalogs list [--limit 100]

# Get catalog details
tkdbr --conn prod catalogs get --catalog my_catalog

# List schemas in a catalog
tkdbr --conn prod schemas list --catalog my_catalog [--limit 100]

# Get schema details
tkdbr --conn prod schemas get --catalog my_catalog --schema my_schema

# List tables in a schema
tkdbr --conn prod tables list --catalog my_catalog --schema my_schema [--limit 100]

# List tables without column info (lighter response)
tkdbr --conn prod tables list --catalog my_catalog --schema my_schema --omit-columns

# Get full table schema and metadata
tkdbr --conn prod tables get --catalog my_catalog --schema my_schema --table my_table

# List jobs
tkdbr --conn prod jobs list [--limit 25]

# Get job details
tkdbr --conn prod jobs get --job-id 123

# Trigger a job run (requires allow_job_runs = true in config)
tkdbr --conn prod jobs trigger --job-id 123

# List and inspect job runs
tkdbr --conn prod runs list --job-id 123 [--limit 10]
tkdbr --conn prod runs get --run-id 456
tkdbr --conn prod runs output --run-id 456

# List and inspect clusters
tkdbr --conn prod clusters list
tkdbr --conn prod clusters get --cluster-id abc-123

# List and inspect SQL warehouses
tkdbr --conn prod warehouses list
tkdbr --conn prod warehouses get --warehouse-id abc-123

# Manage Databricks bundles (uses bundle_target from config, defaults to "local")
# Bundle commands run in the caller's current directory so they can see the local bundle files.
tkdbr --conn prod bundle validate
tkdbr --conn prod bundle deploy
tkdbr --conn prod bundle run my-job
```

Output is compact, agent-friendly JSON optimized for token efficiency. All read operations are safe; only `jobs trigger` requires explicit permission via `allow_job_runs = true` in config.
