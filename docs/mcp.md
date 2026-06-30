# Toolkit MCP Server

`toolkit-mcp` exposes the toolkit surface over the [Model Context Protocol](https://modelcontextprotocol.io)
(MCP). It lets any MCP-capable harness — and the developers, QA engineers, or
helpdesk staff who configure one — use toolkit's safe database and Databricks
access without learning the `tk*` CLIs.

It is a thin **stdio JSON-RPC proxy** in front of the daemon. It does not hold
credentials and it does not enforce anything itself: every call is forwarded to
`toolkit-daemon` over the UNIX socket, exactly like the `tk*` CLIs. All the
safety controls (read-only sessions, write-target allowlists, peer-UID auth)
stay behind the daemon.

```
 MCP harness                toolkit-mcp                toolkit-daemon (_toolkit UID)
 ───────────                ───────────                ────────────────────────────
 tools/call ───stdio JSON-RPC──►  Request  ───UNIX socket──►  libtoolkit::dispatch
 {name,arguments}           {tool,conn,op,params}            reads config, runs query
                  ◄── MCP result ──┘  ◄── Response ──┘
```

## Trust boundary

`toolkit-mcp` has the **same privileges as `tkpsql`**: it links only the daemon
client and wire protocol, never the dispatch core, and never reads
`config.yaml`. A compromised harness can call only the operations the daemon
already permits — it cannot read credentials or reach a service the daemon
can't.

## Tool surface

The exposed tools mirror the CLIs one-for-one. Each MCP tool maps to one
toolkit `(tool, op)` and takes an optional `conn` argument (omit it when only
one connection is configured):

| MCP tool | Toolkit op | Notes |
|----------|-----------|-------|
| `psql_query`, `psql_tables`, `psql_describe` | `psql/*` | Read-only PostgreSQL |
| `msql_query`, `msql_tables`, `msql_describe` | `msql/*` | Read-only MS SQL Server |
| `dbr_jobs_*`, `dbr_runs_*` | `dbr jobs/*`, `dbr runs/*` | Jobs and runs |
| `dbr_clusters_*`, `dbr_warehouses_*` | `dbr clusters/*`, `dbr warehouses/*` | Compute |
| `dbr_catalogs_*`, `dbr_schemas_*`, `dbr_tables_*` | `dbr catalogs/*` … | Unity Catalog |
| `dbr_query` | `dbr query` | SQL warehouse query |
| `dbr_bundle_*` | `dbr bundle/*` | Asset Bundle ops |

Internal operations are deliberately **not** exposed: the Databricks OAuth
helpers (`auth/store_tokens`, `auth/get_host`) belong to the user-run
`tkdbr auth login` flow, and the `guard` ops back the wrapper-script machinery.

Run `tools/list` against the server to see the full, authoritative catalog with
JSON Schemas.

## Errors

- **Protocol errors** (unknown method, malformed JSON, unknown tool) come back
  as JSON-RPC errors.
- **Upstream errors** — a denied write, an unknown connection, an unreachable
  daemon — come back as a normal tool result with `isError: true` and the
  daemon's structured `{"error": "..."}` JSON in the text block, so the model
  can read and react to them.

## Credentials and daemon setup

`toolkit-mcp` does not introduce any new credential storage or daemon setup.
It is purely an additional transport in front of the daemon you already
configured for `tkpsql`/`tkmsql`/`tkdbr`. If `toolkit status` reports the
daemon is up and the native CLIs already work, `toolkit-mcp` works too —
nothing in `config.yaml` or daemon setup needs to change. See
[docs/daemon.md](daemon.md) for daemon setup and [docs/configuration.md](configuration.md)
for the config file format.

## Installing the binary

`toolkit-mcp` is built and packaged alongside the other binaries:

```sh
cargo build --release -p toolkit-mcp
# binary at target/release/toolkit-mcp
```

`brew install scott-abernethy/tap/toolkit` installs `toolkit-mcp` to the same
`bin` directory as `toolkit`, `tkpsql`, `tkmsql`, `tkdbr`, and
`toolkit-daemon` — no separate package.

## MCP mode vs. CLI-daemon mode

MCP mode is **additive, not a replacement**. The `tk*` CLIs and `toolkit-mcp`
are two transports in front of the same daemon — you don't migrate away from
one to use the other, and both can run side by side. Use the native CLIs for
shell-driven workflows (skills, agents, scripts) and `toolkit-mcp` for
harnesses that speak MCP and don't shell out to `tk*` binaries directly. There
is nothing to uninstall or reconfigure when adding MCP mode: keep the daemon
and `config.yaml` as they are and just register `toolkit-mcp` with the
harness, as shown below.

## Registering with a harness

`toolkit-mcp` is spawned by the harness and talks over stdio. Point your MCP
client at the binary. Example (Claude Desktop / `mcp.json`-style config):

```json
{
  "mcpServers": {
    "toolkit": {
      "command": "toolkit-mcp"
    }
  }
}
```

If the daemon uses a non-default socket, pass it through the environment (the
proxy reads `TOOLKIT_SOCKET`, same as the CLIs):

```json
{
  "mcpServers": {
    "toolkit": {
      "command": "toolkit-mcp",
      "env": { "TOOLKIT_SOCKET": "/run/toolkit/toolkit.sock" }
    }
  }
}
```

The daemon must be running and reachable for tool calls to succeed; see
[docs/daemon.md](daemon.md).

### GitHub Copilot CLI

Copilot CLI keeps its own MCP server list in `~/.copilot/mcp-config.json`
(distinct from the `mcp.json`-style examples above). Register `toolkit-mcp`
with the `copilot mcp add` subcommand:

```sh
copilot mcp add toolkit -- toolkit-mcp
```

With a non-default socket:

```sh
copilot mcp add toolkit -e TOOLKIT_SOCKET=/run/toolkit/toolkit.sock -- toolkit-mcp
```

Or interactively: run `/mcp add` inside a Copilot CLI session, set **Server
Type** to `Local` or `STDIO`, **Command** to `toolkit-mcp`, and add
`TOOLKIT_SOCKET` under **Environment Variables** only if you've overridden the
socket path. Either method writes the same entry to
`~/.copilot/mcp-config.json`; you can also edit that file by hand. Verify with
`/mcp` or `/env` inside a session to confirm `toolkit` is listed and connected.

## Smoke test

You can drive the server by hand to confirm it responds (no daemon needed for
`initialize`/`tools/list`):

```sh
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  | toolkit-mcp
```
