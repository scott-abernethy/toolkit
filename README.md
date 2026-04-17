# toolkit

A security and access-control layer between AI coding agents and sensitive network services.

## The Problem

AI coding agents (Claude Code, GitHub Copilot CLI, opencode, etc.) are increasingly useful for interacting with datastores, execution environments, and monitoring systems. Combining access to the system codebase **and** the execution environment enables the agent to explore and iterate rapidly, solving operational problems or implementing features/fixes. But giving an LLM direct access to these services is risky:

- **Accidental harm** — an agent could run destructive queries, trigger unintended jobs, or mutate production data
- **Credential exposure** — connection strings, tokens, and passwords can leak into agent context, logs, or conversation history
- **Token waste** — upstream APIs return verbose responses that burn through context windows

## What Toolkit Does

Toolkit provides a set of tools that sit between the agent and the upstream service. Each tool:

1. **Enforces safety boundaries** — read-only by default, with explicit per-connection allowlists for any write or mutating operation. Write detection happens at the tool level, before queries reach the upstream service.
2. **Hides credentials** — the agent never sees connection strings, passwords, or tokens. Configuration lives in a local file (`~/.config/toolkit/config.toml`) that the agent doesn't read; the tool loads it internally.
3. **Produces token-efficient output** — compact JSON with no decoration, no verbose metadata envelopes, and sensible default limits. Designed for direct consumption by LLMs.
4. **Fails safely** — errors are returned as structured JSON (not stack traces), with credentials scrubbed from error messages.

## Tools

| Binary   | Upstream Service | What It Provides |
|----------|-----------------|------------------|
| `tkpsql` | PostgreSQL | Query, describe tables, list schemas. Read-only by default; optional per-table write allowlists. |
| `tkdbr`  | Databricks | Unity Catalog exploration, SQL queries, job/cluster/warehouse inspection, bundle management. Job triggering requires explicit opt-in. |

## Quick Start

```sh
# Install prerequisites
brew install just
asdf plugin add rust && asdf install

# Build and install
just install

# Configure a connection
mkdir -p ~/.config/toolkit
cat >> ~/.config/toolkit/config.toml << 'EOF'
[psql.local]
host     = "localhost"
port     = 5432
database = "mydb"
user     = "readonly"
password = "secret"
EOF

# Use it
tkpsql tables
tkpsql query --sql "SELECT id, name FROM users LIMIT 10"
```

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

## Landscape & Motivation

Toolkit occupies a gap in the current ecosystem. Existing approaches each solve part of the problem:

**MCP database servers** ([pgmcp](https://github.com/subnetmarco/pgmcp), [postgres-mcp](https://github.com/crystaldba/postgres-mcp), AWS Aurora MCP) provide gated database access with read-only enforcement, but are limited to basic statement-type filtering, don't shape output for token efficiency, and are tied to the MCP protocol. Their config files (typically JSON) are readable by agents, so credentials remain exposed.

**Agent guardrail frameworks** ([LlamaFirewall](https://github.com/meta-llama/PurpleLlama/tree/main/LlamaFirewall), Lakera Guard) focus on prompt injection and code safety analysis — they don't address tool-use gating or credential hiding.

**Sandboxing** (E2B, Bunnyshell) isolates code execution in containers but doesn't solve database access control or credential exposure.

**What's missing everywhere:**

- **Credential hiding as a first-class feature.** [Research by Knostic](https://www.knostic.ai/blog/claude-cursor-env-file-secret-leakage) documented coding agents silently loading `.env` files and leaking API keys. The industry consensus is to treat agents as untrusted processes, but almost nobody ships tooling for it.
- **Token-efficient output.** No existing tool treats context window cost as a design constraint. Verbose API responses are the norm.
- **Semantic write detection.** Every existing solution uses basic statement-type filtering (reject anything that isn't SELECT). Nobody detects writes inside CTEs, function calls with side effects, or schema-qualified edge cases.
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

**Current position:** CLI-first, with MCP as a potential future transport layer rather than a replacement. The core logic (safety boundaries, credential hiding, output shaping) lives in library crates that could serve both a CLI and an MCP server. As toolkit expands to more services (Kubernetes, Datadog, Jira, etc.), the balance may shift — but only if MCP implementations can be kept complete enough that agents don't hit capability walls mid-workflow.

## Documentation

- [Usage examples](docs/usage.md) — detailed command reference for each tool
- [Configuration](docs/configuration.md) — config file format with examples for all tools
- [Contributing](docs/contributing.md) — development commands, prerequisites, and how to add new tools
- [Agent conventions](AGENTS.md) — output format, token efficiency guidelines, and project structure for contributors
