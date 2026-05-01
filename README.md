# toolkit

A safety kit for your tools — reducing the blast radius and leak surface when AI coding agents touch sensitive services.

## The Problem

AI coding agents (Claude Code, GitHub Copilot CLI, opencode, etc.) are increasingly useful for interacting with datastores, execution environments, and monitoring systems. Combining access to the system codebase **and** the execution environment enables the agent to explore and iterate rapidly, solving operational problems or implementing features/fixes. The risks toolkit addresses:

- **Accidental harm** — a well-meaning agent runs a destructive query, triggers an unintended job, or mutates production data
- **Credential leak surface** — connection strings, tokens, and passwords end up in agent context, prompt logs, conversation history, or backups
- **Token waste** — upstream APIs return verbose responses that burn through context windows

## Threat Model

Toolkit has two operating modes with different ceilings:

- **CLI mode (default install)**: tools run as the same OS user as the agent. Toolkit raises the floor against an *agent's mistakes* — accidental writes, exploratory queries that hit prod, leaking credentials by reading the wrong file — but a hostile agent that *wants* to read `~/.config/sops/age/keys.txt` or invoke `sops` directly still can. Pair this mode with the harness hook recipes in `hooks/` to close the casual-leak path.
- **Daemon mode (separate UID)**: `toolkit-daemon` runs as a dedicated `_toolkit` system user that owns the age key and config; the agent UID has no read access. CLI tools (`tkpsql` et al.) connect over a UNIX socket with peer-UID enforcement. Combined with sandboxing the agent itself (sandbox-exec, bwrap, container), this is the configuration that actually defeats a hostile agent on a single box. See [docs/daemon.md](docs/daemon.md) for setup.

In both modes, defence is layered: toolkit's own checks sit on top of (a) harness denylists like `Bash(toolkit:*)` and `Bash(sops:*)`, (b) per-tool agent hooks that block access to `~/.config/toolkit` and `~/.config/sops`, and (c) DB-side GRANTs / read-only roles. Treat toolkit's own checks as defence-in-depth on top of those, not as the only line.

## What Toolkit Does

Toolkit is a safety kit that sits between AI agents and upstream services. Each tool in the kit:

1. **Enforces a default-deny posture for writes** — Postgres connections are session-level read-only at the server (the strongest control here, enforced by Postgres itself); MS SQL relies on `db_datareader` role. An optional `writable_tables` allowlist enables specific writes; client-side write detection is a sanity check on top of DB-side privileges, not a substitute for them.
2. **Reduces credential leak surface** — credentials live in a single sops-encrypted file and are injected into wrapped CLIs as env vars at exec time. Agents never see credentials in argv, in their config files, or in tool output. In CLI mode this protects against backups, accidental commits, prompt logs, and incurious agents — not against an agent that decides to read the key file. In daemon mode the agent UID can't read the file at all.
3. **Produces token-efficient output** — compact JSON with no decoration, no verbose metadata envelopes, and sensible default limits. Designed for direct consumption by LLMs.
4. **Fails safely** — errors are returned as structured JSON (not stack traces), with credentials scrubbed from error messages.

## Installation

### Brew (recommended)

```sh
brew tap scott-abernethy/tap
brew install scott-abernethy/tap/toolkit

brew install sops   # required for config encryption

# Initialize (generates an age keypair for config encryption)

toolkit init

# Configure a connection
toolkit config edit   # creates ~/.config/toolkit/config.yaml and opens $EDITOR

# Use it
tkpsql tables
tkpsql query --sql "SELECT id, name FROM users LIMIT 10"
```

## Tools

Toolkit has two kinds of tool: **native clients** that implement protocol-level safety, and a **guard** that wraps any CLI with credential injection and command allow/deny rules.

### Native Clients

| Binary   | Upstream Service | What It Provides |
|----------|-----------------|------------------|
| `tkpsql` | PostgreSQL | Query, describe tables, list schemas. Read-only by default (session-level enforcement); optional per-table write allowlists. |
| `tkmsql` | MS SQL Server | Query, describe tables, list schemas. Read-only enforced via `db_datareader` role (configure your DB user accordingly); optional per-table write allowlists. Supports on-prem servers with self-signed certs (`trust_cert`). |
| `tkdbr`  | Databricks | Unity Catalog exploration, SQL queries, job/cluster/warehouse inspection, bundle management. Job triggering requires explicit opt-in. |

Native clients earn their complexity — `tkpsql` enforces read-only at the Postgres session level and does type-aware JSON conversion; `tkmsql` provides the same for MS SQL Server via the TDS protocol; `tkdbr` compacts verbose Databricks API responses into token-efficient output. `tkpsql` and `tkmsql` share write-detection and config-loading logic via `common::sql`. These are worth maintaining as dedicated crates because the upstream services need protocol-level handling that a generic wrapper can't provide.

Each native client is a thin CLI over a transport-agnostic library. The CLI dispatches to `toolkit-daemon` over a UNIX socket; the daemon holds all credentials and calls the library on the agent's behalf. See [docs/daemon.md](docs/daemon.md).

### Guard (`toolkit guard`)

For CLI tools where the main value is credential hiding and command gating — not protocol-level safety or output reshaping — `toolkit guard` wraps any CLI with:

- **Credential injection** — env vars from config, never passed as arguments
- **Command allow/deny rules** — token-based matching with `|` alternatives for plurals/aliases
- **Raw passthrough** — stdout/stderr forwarded as-is; the wrapped CLI handles its own output format

Adding a new service requires only a config stanza, not a new Rust crate:

```yaml
kubectl:
  dev:
    command: kubectl
    env:
      KUBECONFIG: /path/to/dev.kubeconfig
    allow:
      - "get pod|pods"
      - "get deploy|deployment|deployments"
      - "describe pod|pods"
      - "logs"
    deny:
      - "secret|secrets"
      - "exec"
      - "delete"
      - "--kubeconfig"
```

`toolkit install` generates wrapper scripts so agents interact with guarded tools naturally:

```sh
toolkit install
# Generates ~/.config/toolkit/bin/tkkubectl-dev

# Agent just runs:
tkkubectl-dev get pods -o json
```

**When to use the guard vs a native client:** Use `toolkit guard` when the upstream CLI already produces usable output (e.g. `kubectl -o json`, `pup --json`) and you just need credential hiding and command gating. Build a native client when you need protocol-level enforcement (session-level read-only), semantic analysis (SQL write detection), or significant output transformation (type-aware JSON conversion).

#### Rule Engine

Rules are space-separated token groups matched with AND semantics. Each group can contain `|`-separated alternatives (OR within the group). A rule matches if every group has at least one alternative present as an exact token in the command args.

```
Rule: "get pod|pods"
Args: ["get", "pods", "-o", "json"]
→ "get" present ✓, "pods" matches "pod|pods" ✓ → MATCH

Rule: "get pod|pods"
Args: ["get", "deployments"]
→ "get" present ✓, neither "pod" nor "pods" present ✗ → NO MATCH
```

Deny rules are checked first. If any deny rule matches, the command is rejected. Then at least one allow rule must match (unless the allow list is empty, which permits all non-denied commands).

## Agent Integration

Toolkit includes skill and agent definitions so AI harnesses can discover and use the tools automatically.

**Skills** (for [opencode](https://opencode.ai)) — teach the agent when and how to invoke each tool:

```sh
for skill in skills/*/; do
  ln -sf "$(pwd)/$skill" ~/.config/opencode/skills/$(basename "$skill")
done
```

**Agents** (for [GitHub Copilot CLI](https://docs.github.com/copilot/concepts/agents/about-copilot-cli)) — specialized sub-agents with focused workflows (e.g. `git-flow` for commit/branch/PR conventions):

```sh
for agent in agents/*.agent.md; do
  ln -sf "$(pwd)/$agent" ~/.copilot/agents/$(basename "$agent")
done
```

See [skills/README.md](skills/README.md) for full setup details and troubleshooting.

## Harness Protections

Toolkit's threat model explicitly calls out that harness-level hooks are a required layer alongside toolkit's own controls. The `hooks/` directory provides ready-to-use recipes for Claude Code and opencode that block:

- Direct `sops`, `age`, and `toolkit` management commands via the Bash tool
- File reads to `~/.config/toolkit`, `~/.config/sops`, `~/.ssh`, `~/.aws`, `~/.gnupg`, and other credential stores via the Read tool
- `.env` file reads (project-local secrets)

```sh
# Install hook scripts
just install-hooks

# Then merge the harness-specific snippet into your settings:
#   Claude Code:  hooks/claude-code/settings.snippet.json → ~/.claude/settings.json
#   opencode:     hooks/opencode/opencode.snippet.json    → ~/.config/opencode/opencode.json
```

See [docs/hooks.md](docs/hooks.md) for full instructions, coverage details, and Copilot CLI notes.

## Daemon Mode (separate UID)

`toolkit-daemon` runs as a dedicated `_toolkit` system user that owns the age key and config. CLI tools connect to it over a UNIX socket; the daemon checks the peer UID, reads the config that the agent UID can't see, and dispatches the call. This is the configuration that closes the structural gap of CLI mode — a hostile agent under its own UID cannot read `~_toolkit/.config/toolkit/config.yaml` or invoke `sops` against the age key.

```sh
# After setup (see docs/daemon.md):
tkpsql tables            # routes through the daemon
```

See [docs/daemon.md](docs/daemon.md) for full setup, including peer-UID enforcement details.

## Landscape & Motivation

Toolkit occupies a gap in the current ecosystem. Existing approaches each solve part of the problem:

**MCP database servers** ([pgmcp](https://github.com/subnetmarco/pgmcp), [postgres-mcp](https://github.com/crystaldba/postgres-mcp), AWS Aurora MCP) provide gated database access with read-only enforcement, but typically rely on statement-type filtering rather than DB-side privileges, don't shape output for token efficiency, and are tied to the MCP protocol. When run locally, their config files are also readable by the agent, leaving credentials on disk in the same trust boundary as the agent itself.

**Agent guardrail frameworks** ([LlamaFirewall](https://github.com/meta-llama/PurpleLlama/tree/main/LlamaFirewall), Lakera Guard) focus on prompt injection and code safety analysis — they don't address tool-use gating or credential hiding.

**Sandboxing** (E2B, Bunnyshell) isolates code execution in containers but doesn't solve database access control or credential exposure.

**What's missing everywhere:**

- **Credential leak-surface reduction as a first-class feature.** [Research by Knostic](https://www.knostic.ai/blog/claude-cursor-env-file-secret-leakage) documented coding agents silently loading `.env` files and leaking API keys. Toolkit puts credentials in one encrypted file and injects them into wrapped CLIs at exec time — closing the casual-leak path (logs, transcripts, backups, accidental commits) without claiming to defeat a determined hostile agent.
- **Token-efficient output.** Verbose API responses are the norm. Inspiration for this came from [rtk](https://github.com/rtk-ai/rtk).
- **DB-side privileges as the control, not statement parsing.** Most existing solutions try to police writes by parsing SQL — a losing arms race against CTEs, side-effecting functions, and dialect quirks. Toolkit defers to the database: Postgres connections enable `default_transaction_read_only` server-side, and MS SQL relies on `db_datareader`. Client-side write detection is present but only as defence-in-depth on top of DB-side privileges.
- **Protocol independence.** MCP servers only work with MCP-compatible hosts. CLI tools work with any agent harness that can shell out.

The [OWASP AI Agent Security Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/AI_Agent_Security_Cheat_Sheet.html) codifies the practices toolkit implements: least privilege, action sandboxing, approval gates, and audit logging.

## CLI vs MCP: Architecture Decision

Toolkit currently ships as CLI tools. This is a deliberate choice informed by the [inner loop / outer loop framework](https://circleci.com/blog/mcp-vs-cli/):

**Why CLI works well for toolkit today:**
- **Zero schema overhead** — every token spent on MCP schema discovery is a token the agent can't use to reason about actual work. CLIs have no startup cost.
- **LLM familiarity** — agents already know how to compose CLI tools with pipes and flags. No schema discovery needed.
- **Composability** — piping output through `jq`, `head`, or other tools is natural and well-understood by agents.
- **Simplicity** — no server process to run, no connection lifecycle, no protocol versioning.

**Where MCP could add value as toolkit grows:**
- **Centralized auth** — as more tools and team members are added, managing per-developer credentials becomes harder. An MCP server could handle auth once at the server level.
- **Cross-system coordination** — workflows spanning databases, Kubernetes, monitoring, and ticketing benefit from session state that CLIs don't naturally maintain.
- **Discovery** — an agent connecting to a single MCP server learns all available operations, vs. needing to know about each CLI tool independently.

**Current position:** CLI-first, with MCP as a potential future transport layer rather than a replacement. The core logic (safety boundaries, credential hiding, output shaping) lives in library crates that could serve both a CLI and an MCP server. The introduction of `toolkit guard` means adding new services no longer requires new Rust crates — only config — so the scaling concern that might push toward MCP is less pressing. MCP remains worth revisiting if centralized auth or cross-system session state become real requirements.

## Documentation

- [Usage examples](docs/usage.md) — detailed command reference for each tool
- [Configuration](docs/configuration.md) — config file format with examples for all tools
- [Daemon transport](docs/daemon.md) — separate-UID setup that closes the hostile-agent gap
- [Harness hooks](docs/hooks.md) — Claude Code / opencode / Copilot CLI deny-list recipes
- [Contributing](docs/contributing.md) — development commands, prerequisites, and how to add new tools
- [Agent conventions](AGENTS.md) — output format, token efficiency guidelines, and project structure for contributors

## License

MIT — see [LICENSE](LICENSE).
