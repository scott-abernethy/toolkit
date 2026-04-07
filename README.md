# toolkit

A collection of CLI tools for team and AI agent use, built as a [Cargo workspace](https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html) in Rust.

Tools are designed to be invoked by AI agents (e.g. from [opencode](https://opencode.ai)) as well as directly by developers. All tools output JSON to stdout so agents can parse results reliably, and are intentionally minimal in their output to reduce token consumption.

## Tools

| Binary    | Crate          | Description                                      |
|-----------|----------------|--------------------------------------------------|
| `tkpsql`  | `crates/psql`  | Read-only PostgreSQL query tool — hides credentials, enforces read-only transactions |

## Prerequisites

**Rust toolchain** via [asdf](https://asdf-vm.com):

```sh
asdf plugin add rust
asdf install        # reads .tool-versions in this repo
```

**For `tkpsql`**: `psql` must be installed and on your `PATH`.

```sh
brew install libpq
brew link --force libpq   # puts psql on PATH
```

## Building

```sh
# Build all tools
cargo build --workspace

# Build a specific tool
cargo build -p tkpsql

# Build optimised release binaries
cargo build --workspace --release
```

Binaries are output to `target/debug/` or `target/release/`.

## Installing

`cargo install` compiles a release binary and places it in your Cargo bin directory. When using asdf-managed Rust this is `~/.asdf/installs/rust/<version>/bin/` rather than the default `~/.cargo/bin/` — this is normal.

After installing, run `asdf reshim rust` to make the binary available on your PATH via asdf's shim layer.

```sh
# Install a specific tool from a local clone
cargo install --path crates/psql
asdf reshim rust

# Install all tools at once
for crate in crates/*/; do cargo install --path "$crate"; done
asdf reshim rust

# Verify
tkpsql --help

# Uninstall
cargo uninstall tkpsql
asdf reshim rust
```

> **Updating**: re-run `cargo install --path ...` and `asdf reshim rust` after pulling new changes.

## Configuration

All tools share a single config file at `~/.config/toolkit/config.toml`. Each tool has its own `[section]`:

```toml
# ~/.config/toolkit/config.toml

[psql]
host     = "localhost"
port     = 5432
database = "mydb"
user     = "readonly"
password = "secret"
```

To use a different config file (e.g. for CI or multiple environments):

```sh
TOOLKIT_CONFIG=/path/to/other.toml tkpsql tables
```

## Usage

### tkpsql

```sh
# List tables in the public schema
tkpsql tables

# List tables in a specific schema
tkpsql tables --schema myschema

# Run a SQL query
tkpsql query --sql "SELECT id, name FROM users LIMIT 10"

# Describe a table's columns
tkpsql describe --table users
tkpsql describe --table myschema.users   # schema-qualified

# Help
tkpsql --help
tkpsql query --help
```

All queries are automatically wrapped in `BEGIN TRANSACTION READ ONLY` — write statements will be rejected by PostgreSQL regardless of the database user's permissions.

Output is compact JSON:

```json
{"rows":[{"id":"1","name":"Alice"},{"id":"2","name":"Bob"}],"count":2}
```

## Development

```sh
# Run all tests
cargo test --workspace

# Lint
cargo clippy --workspace

# Format
cargo fmt --all

# Run a tool without installing
cargo run -p tkpsql -- tables
```

## Adding a New Tool

1. `cargo init crates/<name>` (e.g. `crates/foo` → binary `tkfoo`)
2. Add `"crates/<name>"` to `members` in the root `Cargo.toml`
3. Add `common = { path = "../common" }` to the new crate's dependencies
4. Add a `[name]` section to `~/.config/toolkit/config.toml` if the tool needs config
5. Use `common::load_section::<MyConfig>("name")` to load it
6. See `AGENTS.md` for output and conventions guidelines
