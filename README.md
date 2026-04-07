# toolkit

A collection of CLI tools for team and AI agent use, built as a [Cargo workspace](https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html) in Rust.

Tools are designed to be invoked by AI agents (e.g. from [opencode](https://opencode.ai)) as well as directly by developers. All tools output JSON to stdout so agents can parse results reliably, and are intentionally minimal in their output to reduce token consumption.

## Tools

| Binary    | Crate          | Description                                      |
|-----------|----------------|--------------------------------------------------|
| `tkpsql`  | `crates/psql`  | Read-only PostgreSQL query tool — hides credentials, enforces read-only transactions |

## Prerequisites

- [asdf](https://asdf-vm.com) with the rust plugin: `asdf plugin add rust && asdf install`
- [just](https://github.com/casey/just): `brew install just`
- `~/.cargo/bin` on your `PATH`: add `export PATH="$HOME/.cargo/bin:$PATH"` to your `~/.zshrc`
- **For `tkpsql`**: `brew install libpq && brew link --force libpq`

## Install

```sh
just install
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
```

All queries are automatically wrapped in `BEGIN TRANSACTION READ ONLY` — write statements will be rejected by PostgreSQL regardless of the database user's permissions.

Output is compact JSON:

```json
{"rows":[{"id":"1","name":"Alice"},{"id":"2","name":"Bob"}],"count":2}
```

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

Override the config file path with `TOOLKIT_CONFIG=/path/to/other.toml`.

## Development

```sh
just build    # build all tools
just test     # run all tests
just lint     # clippy
just fmt      # format
```

## Adding a New Tool

1. `cargo init crates/<name>` (e.g. `crates/foo` → binary `tkfoo`)
2. Add `"crates/<name>"` to `members` in the root `Cargo.toml`
3. Add `common = { path = "../common" }` to the new crate's dependencies
4. Add a `[name]` section to `~/.config/toolkit/config.toml` if the tool needs config
5. Use `common::load_section::<MyConfig>("name")` to load it
6. See `AGENTS.md` for output and conventions guidelines
