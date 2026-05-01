# Configuration

All tools share a single config file at `/var/lib/toolkit/.config/toolkit/config.yaml`, owned by the `_toolkit` daemon user. The config is plaintext (mode 0600, readable only by `_toolkit`).

The config file path can be overridden with the `TOOLKIT_CONFIG` environment variable (used by the daemon process).

If only one connection is configured for a tool, `--conn` can be omitted. If multiple connections exist, `--conn` is required — the tool will list available connections if it is missing.

## Setup

Run `toolkit config edit` to open the daemon config in `$EDITOR` via sudo. Use `toolkit config template <app>` to see example config for each tool.

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

## tkmsql

`tkmsql` supports multiple named connections to MS SQL Server:

```yaml
msql:
  onprem:
    host: sql-server.internal
    port: 1433
    database: mydb
    user: readonly
    password: secret
    tls: true          # enable TLS (default: true)
    trust_cert: false  # trust self-signed certs (default: false)

  # Connection with selective write access — only the listed tables can be mutated.
  # The database user should also have the corresponding privileges (e.g. db_datawriter role).
  # For read-only connections, use a user with only the db_datareader role.
  migration:
    host: sql-server.internal
    port: 1433
    database: mydb
    user: migrationuser
    password: secret
    tls: true
    trust_cert: true
    writable_tables:
      - migration_status
```

## tkdbr

`tkdbr` supports multiple named Databricks connections. Credentials are stored in `config.yaml` under an `env:` map and injected as environment variables when invoking the Databricks CLI — no `~/.databrickscfg` file is needed.

```yaml
dbr:
  dev:
    env:
      DATABRICKS_HOST: https://dbc-abc123.cloud.databricks.com
      DATABRICKS_AUTH_TYPE: pat
      DATABRICKS_TOKEN: dapi...          # personal access token
      DATABRICKS_WAREHOUSE_ID: abc123    # default SQL warehouse for queries
    allow_job_runs: false                # permit jobs trigger (default: false)
    bundle_target: dev                   # bundle target (default: "local")

  # OAuth browser flow (no token required — run `tkdbr auth login` to authenticate)
  prod:
    env:
      DATABRICKS_HOST: https://dbc-def456.cloud.databricks.com
      DATABRICKS_AUTH_TYPE: external-browser
      DATABRICKS_ACCOUNT_ID: 00000000-0000-0000-0000-000000000000
      DATABRICKS_WAREHOUSE_ID: abc123def456
    allow_job_runs: false
    bundle_target: prod
```

## daemon

`toolkit-daemon` reads a `daemon:` section from its config. See [docs/daemon.md](daemon.md) for full setup.

```yaml
daemon:
  socket_path: /tmp/toolkit.sock   # default; can also be overridden with $TOOLKIT_SOCKET
  allowed_uids: [501, 502]         # UIDs permitted to connect; omit/empty = all local users
```

Resolution order for the socket path:

| Side    | Order                                              |
|---------|----------------------------------------------------|
| Daemon  | `daemon.socket_path` → `$TOOLKIT_SOCKET` → default |
| Client  | `$TOOLKIT_SOCKET` → default                        |

The CLI client never reads the daemon's config (the agent UID has no read access). If you customise `socket_path`, set `TOOLKIT_SOCKET` in the agent's environment so its CLIs find the socket.
