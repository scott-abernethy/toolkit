# toolkit

A collection of CLI tools for team and AI agent use, built as a [Cargo workspace](https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html) in Rust.

Tools are designed to be invoked by AI agents (e.g. from [opencode](https://opencode.ai)) as well as directly by developers. All tools output JSON to stdout so agents can parse results reliably, and are intentionally minimal in their output to reduce token consumption.

## Tools

| Binary    | Crate          | Description                                      |
|-----------|----------------|--------------------------------------------------|
| `tkpsql`  | `crates/psql`  | PostgreSQL query tool for agents — hides credentials, enforces per-connection write allowlists |
| `tkdbr`   | `crates/dbr`   | Databricks CLI wrapper for exploring Unity Catalog metadata and managing jobs/clusters (read-only) |

## Prerequisites

- [asdf](https://asdf-vm.com) with the rust plugin: `asdf plugin add rust && asdf install`
- [just](https://github.com/casey/just): `brew install just`
- `~/.cargo/bin` on your `PATH`: add `export PATH="$HOME/.cargo/bin:$PATH"` to your `~/.zshrc`

## Install

```sh
just install
```

## Agent Skills

To enable agent support (for [opencode](https://opencode.ai) and similar tools), link the skills to your opencode config:

```sh
# Link all skills
for skill in skills/*/; do
  ln -s "$(pwd)/$skill" ~/.config/opencode/skills/$(basename "$skill")
done
```

See [skills/README.md](skills/README.md) for details.

## Usage

### tkpsql

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

By default all connections are strictly read-only — write statements are rejected at both the tool level and the PostgreSQL session level. To permit writes on specific tables, add a `writable_tables` list to the connection config (see below). Writes to any table not in that list are rejected by the tool before the query reaches the database.

Output is compact JSON:

```json
{"rows":[{"id":"1","name":"Alice"},{"id":"2","name":"Bob"}],"count":2}
```

### tkdbr

```sh
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
tkdbr --conn prod bundle validate
tkdbr --conn prod bundle deploy
tkdbr --conn prod bundle run my-job
```

Output is compact, agent-friendly JSON optimized for token efficiency. All read operations are safe; only `jobs trigger` requires explicit permission via `allow_job_runs = true` in config.

## Configuration

All tools share a single config file at `~/.config/toolkit/config.toml`. Each tool has its own `[section]`.

### tkpsql

`tkpsql` supports multiple named connections:

```toml
[psql.local]
host     = "localhost"
port     = 5432
database = "mydb"
user     = "readonly"
password = "secret"

[psql.prod]
host     = "prod.example.com"
port     = 5432
database = "mydb"
user     = "readonly"
password = "secret"
tls      = true   # enable TLS (default: false)

# Connection with selective write access — only the listed tables can be mutated.
# The database user should also be granted the corresponding privileges.
[psql.migration]
host            = "localhost"
port            = 5432
database        = "mydb"
user            = "migrationuser"
password        = "secret"
writable_tables = ["migration_fc_aggregate_ids", "migration_fc_party_ids"]
```

### tkdbr

`tkdbr` supports multiple named Databricks connections:

```toml
[dbr.dev]
profile = "databricks-dev"           # profile name from ~/.databrickscfg
allow_job_runs = false               # permit jobs trigger (default: false)
bundle_target = "dev"                # bundle target (default: "local")

[dbr.prod]
profile = "databricks-prod"
allow_job_runs = false
bundle_target = "prod"

# Optional: override workspace host (useful for staging/testing)
[dbr.staging]
profile = "databricks-prod"
host = "staging-workspace.databricks.com"
bundle_target = "staging"
```

If only one connection is configured, `--conn` can be omitted. Override the config file path with `TOOLKIT_CONFIG=/path/to/other.toml`.

## Development

```sh
just build    # build all tools
just test     # run all tests
just lint     # clippy
just fmt      # format
```

## Adding a New Tool

1. `cargo init crates/<name>` (e.g. `crates/foo` → binary `tkfoo`)
2. Add `"crates/<name>"` to `members` in the root `Cargo.toml`
3. Add `common = { path = "../common" }` to the new crate's dependencies
4. Add a `[name]` section to `~/.config/toolkit/config.toml` if the tool needs config
5. Use `common::load_section::<MyConfig>("name")` to load it
6. See `AGENTS.md` for output and conventions guidelines
