# Configuration

All tools share a single config file at `~/.config/toolkit/config.yaml`. Each tool has its own section within that file.

The config file path can be overridden with the `TOOLKIT_CONFIG` environment variable.

If only one connection is configured for a tool, `--conn` can be omitted. If multiple connections exist, `--conn` is required — the tool will list available connections if it is missing.

## Setup

Run `toolkit init` once to generate an age keypair, then use `toolkit config edit` to open the config in `$EDITOR` via sops. The file is encrypted on save.

## tkpsql

`tkpsql` supports multiple named connections:

```yaml
psql:
  local:
    host: localhost
    port: 5432
    database: mydb
    user: readonly
    password: secret

  prod:
    host: prod.example.com
    port: 5432
    database: mydb
    user: readonly
    password: secret
    tls: true   # enable TLS (default: false)

  # Connection with selective write access — only the listed tables can be mutated.
  # The database user should also be granted the corresponding privileges.
  migration:
    host: localhost
    port: 5432
    database: mydb
    user: migrationuser
    password: secret
    writable_tables:
      - migration_fc_aggregate_ids
      - migration_fc_party_ids
```

## tkdbr

`tkdbr` supports multiple named Databricks connections. Credentials are stored directly in `config.yaml` and injected via environment variables when invoking the Databricks CLI — no `~/.databrickscfg` file is needed.

```yaml
dbr:
  dev:
    host: https://dbc-abc123.cloud.databricks.com
    token: dapi...                     # personal access token
    warehouse_id: 9f9919ede4d8f98d     # default SQL warehouse for queries
    allow_job_runs: false              # permit jobs trigger (default: false)
    bundle_target: dev                 # bundle target (default: "local")

  prod:
    host: https://dbc-def456.cloud.databricks.com
    token: dapi...
    warehouse_id: abc123def456
    allow_job_runs: false
    bundle_target: prod
```
