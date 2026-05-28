# toolkit

[![Release workflow](https://github.com/scott-abernethy/toolkit/actions/workflows/release-private-tap.yml/badge.svg)](https://github.com/scott-abernethy/toolkit/actions/workflows/release-private-tap.yml)
[![Release](https://img.shields.io/github/v/release/scott-abernethy/toolkit)](https://github.com/scott-abernethy/toolkit/releases)
[![License](https://img.shields.io/github/license/scott-abernethy/toolkit)](LICENSE)
[![Homebrew tap](https://img.shields.io/badge/homebrew-tap-orange?logo=homebrew)](https://github.com/scott-abernethy/homebrew-tap)

A safety kit for your tools — reducing the blast radius and leak surface when AI coding agents touch sensitive services.

## The Problem

AI coding agents (Claude Code, GitHub Copilot CLI, opencode, etc.) are increasingly useful for interacting with datastores, execution environments, and monitoring systems. Combining access to the system codebase **and** the execution environment enables the agent to explore and iterate rapidly, solving operational problems or implementing features/fixes. The risks toolkit addresses:

- **Accidental harm** — a well-meaning agent runs a destructive query, triggers an unintended job, or mutates production data
- **Credential leak surface** — connection strings, tokens, and passwords end up in agent context, prompt logs, conversation history, or backups
- **Token waste** — upstream APIs return verbose responses that burn through context windows

## Threat Model

Toolkit addresses two distinct surfaces:

**Leak surface — what the agent can see.** Credentials live in `/var/lib/toolkit/.config/toolkit/config.yaml` (mode 0600, owned by `_toolkit`). The agent UID can't read the file or list the directory, and tools never put credentials in argv, env, or output that the agent reads. CLIs that wrap external tools (Databricks, kubectl) inject credentials at exec time so the agent's home directory doesn't accumulate plaintext config.

**Action surface — what the agent can do.** `toolkit-daemon` listens on a UNIX socket; the agent UID connects from the other side. Peer-UID enforcement (`getpeereid` / `SO_PEERCRED`) gates the connection, and every request runs through a typed dispatch handler — there is no path that takes raw SQL or shell input from the agent and runs it without going through a tool-specific check (write-target allowlist for SQL, allow/deny rule engine for guard).

**What toolkit does NOT enforce:**

- **Per-UID or per-connection authorisation.** `daemon.allowed_uids` is binary: every listed UID can use every connection in the config. If `config.yaml` holds both a `dev` and a `prod` Postgres connection and the developer UID is allowed, the agent can reach prod. To segregate, run a second daemon under a different `socket_path` and config, or split connections across hosts.
- **Network egress.** Once a UID can reach the daemon and the daemon can reach the upstream service, traffic flows. If you need a network boundary, use OS-level controls (firewalls, network namespaces, VPC ACLs).
- **Inference from query results.** Toolkit can stop a write but not the slow reconstruction of schema from `SELECT` output. Read-only is not no-information.

**Defence in layers** (strongest first):

1. **Service-side privileges.** Read-only DB roles, IAM scopes, GRANTs. Enforced where it matters; the only layer that survives a compromise of everything above it.
2. **Toolkit checks.** Session-level read-only on Postgres, write-table allowlists, peer-UID at the socket, allow/deny rules for guarded CLIs. Catch mistakes and contain a misbehaving tool before it reaches the service.
3. **Harness hooks.** Claude Code / opencode deny rules that block reads of `~/.aws`, `.env`, and direct `toolkit` management commands. Stop a request before it's ever made.
4. **Agent sandbox.** sandbox-exec, bwrap, container. Bounds what the agent can do outside toolkit's surface.

Toolkit is meaningful as one layer in that stack — not as a substitute for any of the others.

For the full attacker model, bypass matrix, and deployment guidance, see [docs/threat-model.md](docs/threat-model.md).

## What Toolkit Does

Toolkit is a safety kit that sits between AI agents and upstream services. Each tool in the kit:

1. **Defers write authorisation to the database** — Postgres connections start with `default_transaction_read_only=on` at the server, so writes fail at the engine even if a query slips past the client. MS SQL relies on the SQL login's role (`db_datareader` for read-only); toolkit can't enforce this from the client, so configure your DB user accordingly.
2. **Treats client-side write detection as defence-in-depth** — when a `writable_tables` allowlist is configured, toolkit parses each statement and rejects writes to tables outside the list before sending anything to the database. This is a sanity check on top of the DB-side controls in (1), not a substitute for them.
3. **Reduces credential leak surface** — credentials live in a single config file owned by the `_toolkit` daemon user (mode 0600) and are injected into wrapped CLIs as env vars at exec time. Agents never see credentials in argv, in their config files, or in tool output.
4. **Produces token-efficient output** — compact JSON with no decoration, no verbose metadata envelopes, and sensible default limits. Designed for direct consumption by LLMs.
5. **Fails safely** — errors are returned as structured JSON (not stack traces), with credentials scrubbed from error messages.

## Installation

### Brew (recommended)

```sh
brew tap scott-abernethy/tap
brew install scott-abernethy/tap/toolkit

# Run the privileged setup script (creates _toolkit user, installs LaunchDaemon)
sudo $(brew --prefix)/opt/toolkit/libexec/setup-daemon.sh

# Configure connections
toolkit config edit   # opens daemon config in $EDITOR via sudo

# Verify the daemon is running
toolkit status

# Use it
tkpsql tables
tkpsql query --sql "SELECT id, name FROM users LIMIT 10"
```

## Tools

Toolkit has two kinds of tool: **native clients** that implement protocol-level safety, and a **guard** that wraps any CLI with credential injection and command allow/deny rules.

### Native Clients

| Binary   | Upstream Service | What It Provides |
|----------|-----------------|------------------|
| `tkpsql` | PostgreSQL | Query, describe, list schemas. Read-only by default (session-level enforcement). |
| `tkmsql` | MS SQL Server | Query, describe, list schemas. Read-only enforced via `db_datareader` role. |
| `tkdbr`  | Databricks | Unity Catalog, SQL queries, jobs/clusters/warehouses, bundle management. |

Native clients provide protocol-level safety (e.g., session-level read-only in Postgres) and type-aware JSON conversion. They dispatch requests to `toolkit-daemon` over a UNIX socket. See [docs/usage.md](docs/usage.md) for detailed command reference.

### Guard (`toolkit guard`)

For tools where the main value is credential hiding and command gating, `toolkit guard` wraps any CLI with:

- **Credential injection** — env vars fetched from the daemon, never stored locally.
- **Command allow/deny rules** — token-based matching for gated access.
- **Raw passthrough** — preserves the wrapped CLI's original output format.

Adding a new service requires only a config stanza, not a new Rust crate. See [docs/configuration.md](docs/configuration.md) for guard setup examples.

## Agent Integration

Toolkit includes skill and agent definitions so AI harnesses can discover and use the tools automatically.

- **Skills** (for [opencode](https://opencode.ai)) — teach the agent when and how to invoke each tool.
- **Agents** (for [GitHub Copilot CLI](https://docs.github.com/copilot/concepts/agents/about-copilot-cli)) — specialized sub-agents with focused workflows.

See [skills/README.md](skills/README.md) for setup details.

## Harness Protections

Harness-level hooks are a required layer alongside toolkit's own controls. The `hooks/` directory provides recipes for Claude Code and opencode that block direct file reads of credentials and management commands.

```sh
toolkit init --harness all --scope global
toolkit validate
```

See [docs/hooks.md](docs/hooks.md) for full instructions.

## Documentation

- [Usage examples](docs/usage.md) — detailed command reference
- [Configuration](docs/configuration.md) — config file format
- [Daemon transport](docs/daemon.md) — separate-UID setup and security
- [Threat model](docs/threat-model.md) — attacker model, limitations, and layered controls
- [Harness hooks](docs/hooks.md) — Claude / opencode / Copilot CLI recipes
- [Contributing](docs/contributing.md) — development commands and prerequisites
- [Contributor Guide](AGENTS.md) — architecture, output philosophy, and agent conventions

## License

MIT — see [LICENSE](LICENSE).
