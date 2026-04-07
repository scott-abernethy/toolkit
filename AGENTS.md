# AGENTS.md

## Project Overview

This is a Cargo workspace containing CLI tools for team use. The tools are designed to be invoked by AI agents (e.g. from opencode) as well as directly by developers.

## Repository Structure

```
toolkit/
  Cargo.toml          # Workspace root
  Cargo.lock           # Shared lockfile
  crates/
    <tool-name>/       # Each tool is its own crate
      Cargo.toml
      src/
        main.rs
```

- Every tool lives under `crates/` as an independent binary crate.
- Shared utilities live in `crates/common` as a library crate.
- The workspace root `Cargo.toml` lists all member crates.

## Conventions

- **Rust** is the primary language. Use stable Rust.
- **Tool naming**: binary names should be lowercase without hyphens, prefixed with `tk` (e.g. `tkfoo`).
- **Crate naming**: crate directory names omit the `tk` prefix (e.g. `crates/foo` produces binary `tkfoo`).
- **Structured output**: tools invoked by agents should output JSON to stdout. Use stderr for human-readable logs/progress.
- **Exit codes**: 0 for success, non-zero for failure. Prefer well-known codes where applicable.
- **Error output**: write errors as JSON to stdout (not stderr) so agents can parse them. Use `{ "error": "message" }` format.
- **Dependencies**: prefer widely-used crates (`clap` for args, `serde`/`serde_json` for serialization, `anyhow` for error handling).

## Configuration

All tools share a single config file: `~/.config/toolkit/config.toml`.

Each tool has its own `[section]` within that file:

```toml
# ~/.config/toolkit/config.toml

[psql]
host = "localhost"
port = 5432
database = "mydb"
user = "readonly"
password = "secret"

[anothertool]
some_setting = "value"
```

- The config file path can be overridden with the `TOOLKIT_CONFIG` environment variable.
- Config loading is handled by `common::load_section::<T>("section")` — each tool defines its own config struct and requests its slice.
- Tools should never expose credentials in their output.

## Building & Testing

```sh
# Build everything
cargo build --workspace

# Build a specific tool
cargo build -p <tool-name>

# Run all tests
cargo test --workspace

# Run a specific tool
cargo run -p <tool-name> -- <args>
```

## Adding a New Tool

1. Create a new crate: `cargo init crates/<tool-name>`
2. Add it to the workspace members in the root `Cargo.toml`
3. Implement the CLI using `clap` for argument parsing
4. Ensure the tool works well for both human and agent callers (structured output, clear exit codes)
5. Add tests

## Agent Usage Guidelines

- Tools should be self-documenting via `--help`
- Prefer subcommands over positional args for discoverability
- Keep invocations simple — agents work best with explicit flags rather than implicit behavior
- If a tool reads from stdin, also support a `--input` flag as an alternative
- Tools should fail fast and loudly rather than prompting for input

## Output Token Efficiency

Every token an agent reads costs time and money. Tools should produce **minimal, high-signal output** by default. Think of this as building [rtk](https://github.com/rtk-ai/rtk)-style compression directly into the tool rather than wrapping after the fact.

### Principles

1. **Filter noise** — strip boilerplate, decorative output, progress bars, and redundant context. Only emit what the agent needs to act on.
2. **Group and aggregate** — collapse repeated items (e.g. group files by directory, errors by type/rule) rather than listing each individually.
3. **Truncate with escape hatches** — show a bounded amount by default (e.g. first N rows, top N errors). Provide `--limit` / `--full` flags when the agent needs more.
4. **Compact JSON** — use short, consistent key names. Omit null/empty fields. Prefer flat structures over deeply nested ones where practical.
5. **Success = minimal** — for mutating operations, a terse acknowledgement is enough (e.g. `{"ok": true}`). Save verbosity for failures.
6. **Failure = actionable** — error output should contain exactly what's needed to diagnose: message, location, context. No stack traces unless requested.

### Example: Good vs. Bad

```json
// BAD — 800 tokens of noise
{"status":"success","message":"Query executed successfully","metadata":{"server":"db1.example.com","port":5432,"database":"mydb","user":"readonly","ssl":true,"protocol_version":3,"server_version":"15.2","query_duration_ms":42,"rows_affected":0,"columns_returned":3},"results":[...]}

// GOOD — 50 tokens, same information
{"rows":[...],"count":3}
```

## Toolchain

- Rust toolchain is managed via `asdf` (see `.tool-versions`)
- Use `cargo fmt` and `cargo clippy` before committing
