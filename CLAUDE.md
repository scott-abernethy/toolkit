# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Toolkit** is a security and access-control layer between AI coding agents and sensitive network services (datastores, execution environments, monitoring systems). It provides CLI tools that enforce safety boundaries (read-only by default, explicit write allowlists), hide credentials from agent context, and produce token-efficient JSON output. Built as a Cargo workspace in Rust.

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
cargo test -p dbr <test_name>      # test in dbr crate
```

Rust toolchain is managed via asdf (see `.tool-versions`).

## Architecture

### Workspace Structure

```
crates/
  common/   # shared library: config loading, error handling
  psql/     # tkpsql binary — PostgreSQL query tool
  dbr/      # tkdbr binary — Databricks CLI wrapper
skills/     # SKILL.md definitions for opencode integration
agents/     # *.agent.md definitions for GitHub Copilot CLI
```

### Common Library (`crates/common`)

All tools share:
- `load_section::<T>(section: &str) -> T` — loads a section from `~/.config/toolkit/config.yaml` (or `TOOLKIT_CONFIG` env var) into a typed struct
- `ErrorResponse` — standard JSON error struct
- `exit_with_error(msg)` — prints `{"error": "..."}` to stdout and exits 1

Each tool defines its own config struct and deserializes its named section. If one connection is configured, `--conn` is optional; if multiple exist, `--conn` is required.

### `tkpsql` (PostgreSQL Tool)

- Read-only by default (enforced at DB session level)
- Optional per-connection `writable_tables` allowlist in config
- Write detection runs before any query reaches the database
- Type-aware JSON conversion: bool, int, UUID, timestamps, JSONB, etc.
- Commands: `tables`, `describe --table <name>`, `query --sql <stmt>`

### `tkdbr` (Databricks Tool)

- Wraps Databricks CLI and REST API; credentials injected via env vars (`DATABRICKS_HOST`, `DATABRICKS_TOKEN`)
- All read operations are safe by default; `allow_job_runs = true` required to trigger jobs
- Bundle operations: `validate`, `deploy`, `run`
- Commands: `catalogs`, `schemas`, `tables`, `jobs`, `runs`, `clusters`, `warehouses`, `bundle`, `query`
- Output includes sensible defaults (e.g., `--limit 25` for jobs, `--limit 100` for queries)

### Credential Injection

All credentials must live in toolkit's `config.yaml` — never in external config files (e.g. `~/.databrickscfg`). When a tool wraps an external CLI, it injects credentials via environment variables at invocation time. This ensures a single file to encrypt and no plaintext credential files for agents to discover. New tools that wrap external CLIs must follow this pattern.

### Output Philosophy

- All output is compact JSON — no status messages, decorations, or verbose envelopes
- Errors go to stdout as `{"error": "..."}` (not stderr), exit code 1
- Success output is minimal and high-signal (e.g., `{"rows": [...], "count": 3}`)
- Binary names use `tk` prefix (`tkpsql`, `tkdbr`); crate directories omit it

### AI Integration

- **Skills** (`skills/<tool>/SKILL.md`): Symlinked to `~/.config/opencode/skills/` for opencode agent use
- **Agents** (`agents/*.agent.md`): Symlinked to `~/.copilot/agents/` for GitHub Copilot CLI use; `git-flow.agent.md` enforces commit format (`<type>: <ticket#>: <description>`) and branch naming (`<type>/<ticket#>-kebab-case`)

## Configuration

All tools share `~/.config/toolkit/config.yaml`:

```yaml
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
    host: https://dbc-abc123.cloud.databricks.com
    token: dapi...
    warehouse_id: abc123
    allow_job_runs: false
    bundle_target: dev
```
