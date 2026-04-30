## TODO

- Output filter pipeline for `toolkit guard` — configurable per-connection in YAML (strip_ansi, strip_lines regex, max_lines, truncate_lines_at). Declarative filters, not per-tool Rust code.
- Short-circuit output rules — if output matches a pattern, replace with a compact message (e.g. "working tree clean" → "No changes"). Avoids sending verbose no-op output to agents.
- Evaluate hook-based interception (like RTK's PreToolUse hook) instead of/alongside wrapper scripts. Agents currently see `tkkubectl-dev` instead of `kubectl`, which reveals the guard layer and lets them attempt to call the underlying binary directly. A hook approach rewrites commands transparently — agents think they're running the real CLI. An agent just did this while testing toolkit, so it's possible: `Bash(env -u CLAUDECODE -u OPENCODE ...)`
- Have a human override for `toolkit guard` which removes restrictions. Alternatively, `toolkit env` which would provide the env vars so that a user can run it infront of a CLI tool
- Apparently agents can break out of the harness sandbox

## Review findings (2026-04-30)

### Correctness bugs

- `assert_write_allowed` only checks the first detected write target. Multi-statement input like `INSERT INTO allowed …; DELETE FROM forbidden` passes if `allowed` is on the list. Walk every match in `detect_write_target` instead of returning on first.
- `detect_write_target` calls `sql.to_uppercase()` then indexes back into the original bytes. Non-ASCII identifiers (e.g. accented chars) can change byte length and panic on UTF-8 boundaries. Switch to ASCII-only uppercasing or work on chars.
- `has_limit_clause` only matches `" LIMIT "` (single spaces). Misses `LIMIT\n10`, tabs, multiple spaces. Use a whitespace-tolerant scan.
- Session-level `default_transaction_read_only=on` is disabled for the entire session as soon as `writable_tables` is non-empty. Keep the session read-only and wrap the allowlisted statement in a transaction with `SET LOCAL transaction_read_write` instead.
- `sanitize_pg_error` falls through to `db_error.message()` verbatim when no string match hits — error messages can echo connection parameters. Whitelist fields out of `db_error()` rather than passing the raw message.
- `write_key_file` backup via `std::fs::copy` may not preserve mode reliably; explicitly chmod the `.bak` to 0600.

### Bundle/CLI brittleness

- `bundle_run` parses stdout to extract the run ID from `Run URL: …`. Use the API instead.
- `cmd_install` discovers guarded apps by scanning for any conn with a `binary` field — fragile if `dbr` (or another native client) ever gains a `binary` field. Use an explicit marker (e.g. `kind: guard`) or a list under `guard:`.
- `cmd_config_template` for `dbr` shows `allow: []` / `deny: []` but `dbr::ConnConfig` doesn't have those fields — stale template.

### Hygiene / ergonomics

- `tkpsql` opens a fresh PG connection per query. Probably fine; flag if latency complaints emerge.
- `is_agent` / `reject_if_agent` re-parse the YAML config on every command. Memoize.
- JSON output shapes are slightly inconsistent (`{"rows": …}` for `tkpsql tables` and `describe`, which aren't really rows). Cosmetic.
- SQL travels via argv. Add `--sql -` (stdin) or `--sql-file` to remove it from `/proc/PID/cmdline` and shell history. Doesn't fix harness-transcript exposure (structural to CLI mode).

### Two async runtimes

- `tkpsql` is sync (`postgres`); `tkmsql` is async (`tiberius` + tokio). Real but small cost (compile time, binary size). Only worth merging if SQL paths get consolidated into a shared crate.
