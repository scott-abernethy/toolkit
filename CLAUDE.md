# Contributor Guide

Guidance for AI agents and human contributors working on toolkit. The same content is mirrored in `AGENTS.md` so both Claude Code and other agent harnesses pick it up.

## Project Overview

**Toolkit** is a security and access-control layer between AI coding agents and sensitive network services (datastores, execution environments, monitoring systems). It provides CLI tools that enforce safety boundaries (read-only by default, explicit write allowlists), hide credentials from agent context, and produce token-efficient JSON output. Built as a Cargo workspace in Rust.

Tools have two operating modes: **CLI mode** (tools run as the agent's UID, sharing a trust boundary) and **daemon mode** (`toolkit-daemon` runs as a separate `_toolkit` system user that owns the age key and config; CLI tools connect over a UNIX socket with peer-UID enforcement).

## Build & Development Commands

```bash
just build      # cargo build --workspace
just test       # cargo test --workspace
just lint       # cargo clippy --workspace
just fmt        # cargo fmt --all
just install    # installs binaries to ~/.cargo/bin
just audit      # dependency security check
```

Run a single test:
```bash
cargo test -p psql <test_name>     # test in psql crate
cargo test -p msql <test_name>     # test in msql crate
cargo test -p dbr <test_name>      # test in dbr crate
```

Rust toolchain is managed via asdf (see `.tool-versions`).

## Architecture

### Workspace Structure

```
crates/
  common/   # shared library: config loading, error types, daemon protocol + client
  psql/     # tkpsql binary + lib — PostgreSQL query tool
  msql/     # tkmsql binary + lib — MS SQL Server query tool
  dbr/      # tkdbr binary + lib — Databricks CLI wrapper
  daemon/   # toolkit-daemon binary — separate-UID dispatch process
  toolkit/  # toolkit binary — key/config management + CLI guard
hooks/      # harness hook recipes (Claude Code, opencode)
skills/     # SKILL.md definitions for opencode integration
agents/     # *.agent.md definitions for GitHub Copilot CLI
docs/       # user-facing documentation
```

### Library / Transport Split

Each native client crate (`psql`, `msql`, `dbr`) ships both a binary and a `lib.rs` exposing transport-agnostic functions:

- Library functions return `Result<T, ToolkitError>` with no stdout side effects — they don't print, log progress, or assume an argv shape.
- The CLI `main.rs` parses args, sends a JSON request to the daemon, and prints the result.
- `toolkit-daemon` depends on each `tk*` lib and dispatches requests by tool/op name.

Adding a new operation means: add the function to the lib, add a CLI subcommand that calls it, and add a dispatch arm in `crates/daemon/src/dispatch.rs`. The wire protocol (`Request{tool, conn, op, params}`) is the contract between CLI and daemon.

### Common Library (`crates/common`)

All tools share:
- `load_section::<T>(section)` / `load_named_section::<T>(section, conn)` — load a section of `~/.config/toolkit/config.yaml` (override with `TOOLKIT_CONFIG`) into a typed struct
- `ToolkitError` / `Result<T>` — structured errors with caller-distinguishable variants (Auth, Connection, NotFound, Permission, Daemon, Other)
- `ErrorResponse` + `exit_with_error(err)` — binary-entrypoint helper that prints `{"error": "..."}` to stdout and exits 1
- `protocol::{Request, Response}` — daemon wire types
- `client::send(&Request)` — fail-closed UNIX-socket client used by the CLI binaries

If one connection is configured, `--conn` is optional; if multiple exist, `--conn` is required.

### `tkpsql` (PostgreSQL)

- Read-only by default (enforced at DB session level via `default_transaction_read_only=on`)
- Optional per-connection `writable_tables` allowlist in config
- Write detection runs before any query reaches the database
- Type-aware JSON conversion: bool, int, UUID, timestamps, JSONB, etc.
- Commands: `tables`, `describe --table <name>`, `query --sql <stmt>`

### `tkmsql` (MS SQL Server)

- Read-only enforcement relies on the SQL login's role (`db_datareader` for read-only)
- Same write-detection logic and `writable_tables` allowlist as `tkpsql` (shared in `common::sql`)
- Async runtime (`tiberius` over tokio) — distinct from `tkpsql`'s sync `postgres` driver
- Supports on-prem servers with self-signed certs (`trust_cert: true`)
- Commands: `tables`, `describe --table <name>`, `query --sql <stmt>`

### `tkdbr` (Databricks)

- Wraps Databricks CLI and REST API; credentials injected via env vars (`DATABRICKS_HOST`, `DATABRICKS_TOKEN`)
- All read operations are safe by default; `allow_job_runs = true` required to trigger jobs
- Bundle operations: `validate`, `deploy`, `run`
- Commands: `catalogs`, `schemas`, `tables`, `jobs`, `runs`, `clusters`, `warehouses`, `bundle`, `query`, `auth login`
- Output includes sensible defaults (e.g., `--limit 25` for jobs, `--limit 100` for queries)
- `tkdbr auth login` runs the native OAuth U2M (PKCE) flow in user space, then sends tokens to the daemon via socket to store securely in `_toolkit`'s home

### `toolkit guard` (CLI Guard)

- Wraps any CLI with credential injection and command allow/deny rules, configured entirely in YAML
- New services added via config, not new Rust crates — use for CLIs that already produce usable output
- Token-based allow/deny rules with `|` alternatives for plurals/aliases (e.g. `"get pod|pods"`)
- `toolkit install` generates wrapper scripts named `tk<app>-<conn>` into the `install_path` from config (defaults to `$HOME/.local/bin`)
- Agents interact with wrapper scripts directly (e.g. `tkkubectl-dev get pods`) — no awareness of the guard

### `toolkit-daemon` (Separate-UID Transport)

- Long-running process owned by a dedicated `_toolkit` system user; only the daemon UID can read the config and age key
- Listens on a UNIX socket (`/tmp/toolkit.sock` by default; override with `TOOLKIT_SOCKET` or `daemon.socket_path` in config)
- Enforces peer UID via `getpeereid` (macOS) / `SO_PEERCRED` (Linux); optional `daemon.allowed_uids` allowlist
- Dispatches requests to per-tool lib functions; 1 MiB frame limit, 120s read timeout
- Fails closed: stale-socket cleanup refuses to unlink non-socket files; CLI client returns `ToolkitError::Daemon` if the socket is unreachable rather than silently falling back to direct mode
- Setup: see `docs/daemon.md`

### Credential Injection

All credentials must live in toolkit's `config.yaml` — never in external config files (e.g. `~/.databrickscfg`). When a tool wraps an external CLI, it injects credentials via environment variables at invocation time. This ensures a single file to encrypt and no plaintext credential files for agents to discover. New tools that wrap external CLIs must follow this pattern.

### Output Philosophy

- All output is compact JSON — no status messages, decorations, or verbose envelopes
- Errors go to stdout as `{"error": "..."}` (not stderr), exit code 1
- Success output is minimal and high-signal (e.g., `{"rows": [...], "count": 3}`)
- Binary names use `tk` prefix (`tkpsql`, `tkmsql`, `tkdbr`); crate directories omit it
- Library code returns typed values; only binary `main.rs` files print to stdout

### Output Token Efficiency

Every token an agent reads costs time and money. Tools should produce **minimal, high-signal output** by default. Think of this as building [rtk](https://github.com/rtk-ai/rtk)-style compression directly into the tool rather than wrapping after the fact.

1. **Filter noise** — strip boilerplate, decorative output, progress bars, and redundant context.
2. **Group and aggregate** — collapse repeated items rather than listing each individually.
3. **Truncate with escape hatches** — bound output by default (e.g. first N rows); provide `--limit` / `--full` flags.
4. **Compact JSON** — short, consistent key names; omit null/empty fields; prefer flat structures.
5. **Success = minimal** — for mutating operations, a terse acknowledgement is enough (e.g. `{"ok": true}`).
6. **Failure = actionable** — error output should contain exactly what's needed to diagnose: message, location, context.

```json
// BAD — 800 tokens of noise
{"status":"success","message":"Query executed successfully","metadata":{"server":"db1.example.com","port":5432,"database":"mydb","user":"readonly","ssl":true,"protocol_version":3,"server_version":"15.2","query_duration_ms":42,"rows_affected":0,"columns_returned":3},"results":[...]}

// GOOD — 50 tokens, same information
{"rows":[...],"count":3}
```

### AI Integration

- **Skills** (`skills/<tool>/SKILL.md`): Symlinked to `~/.config/opencode/skills/` for opencode agent use
- **Agents** (`agents/*.agent.md`): Symlinked to `~/.copilot/agents/` for GitHub Copilot CLI use; `git-flow.agent.md` enforces commit format (`<type>: <ticket#>: <description>`) and branch naming (`<type>/<ticket#>-kebab-case`)
- **Hooks** (`hooks/`): Defence-in-depth recipes for Claude Code (`permissions.deny` + PreToolUse hooks) and opencode (per-tool deny rules). Install with `just install-hooks`. See `docs/hooks.md`.

## Configuration

All tools share `~/.config/toolkit/config.yaml`:

```yaml
# Top-level settings
install_path: "$HOME/.local/bin"   # where `toolkit install` writes wrapper scripts

psql:
  local:
    host: localhost
    port: 5432
    database: mydb
    user: readonly
    password: secret
    tls: false
    writable_tables: []

dbr:
  dev:
    env:
      DATABRICKS_HOST: https://dbc-abc123.cloud.databricks.com
      DATABRICKS_AUTH_TYPE: pat
      DATABRICKS_TOKEN: dapi...
      DATABRICKS_WAREHOUSE_ID: abc123
    allow_job_runs: false
    bundle_target: dev

# Optional — only relevant when running the daemon
daemon:
  socket_path: /tmp/toolkit.sock
  allowed_uids: [501, 502]
```

In daemon mode the config lives at `~_toolkit/.config/toolkit/config.yaml` (e.g. `/var/lib/toolkit/.config/toolkit/config.yaml`) and is readable only by the `_toolkit` user.

## Adding a New Tool

1. `cargo init crates/<name>` (e.g. `crates/foo` → binary `tkfoo`)
2. Add `"crates/<name>"` to `members` in the root `Cargo.toml`
3. Add `common = { path = "../common" }` to the new crate's dependencies
4. Split the implementation into `lib.rs` (transport-agnostic functions returning `Result<T, ToolkitError>`) and `main.rs` (clap parsing + `print_json` + optional daemon dispatch via `common::client::send`)
5. Add a `[name]` section to `~/.config/toolkit/config.yaml` and load it with `common::load_named_section`
6. If the tool should be daemon-dispatchable, expose a lib target in `Cargo.toml`, add the crate as a dep in `crates/daemon/Cargo.toml`, and add dispatch arms in `crates/daemon/src/dispatch.rs`
7. Tools should be self-documenting via `--help`; prefer subcommands over positional args; fail fast rather than prompting

## Conventions

- **Rust** is the primary language; use stable Rust
- **Tool naming**: binary names lowercase without hyphens, `tk` prefix (e.g. `tkfoo`); crate directory omits the prefix (`crates/foo`)
- **Structured output**: JSON to stdout for agents; stderr is reserved for daemon-side server logs only
- **Exit codes**: 0 on success, 1 on error
- **Error output**: `{"error": "..."}` to stdout (not stderr) so agents can parse it
- **Library purity**: lib functions never print, never read argv, never call `process::exit`. Side effects belong in binaries.
- **Dependencies**: prefer widely-used crates (`clap`, `serde`/`serde_json`, `tokio` where async is needed)

Use `cargo fmt` and `cargo clippy` before committing.
