## TODO

- Output filter pipeline for `toolkit guard` — configurable per-connection in YAML (strip_ansi, strip_lines regex, max_lines, truncate_lines_at). Declarative filters, not per-tool Rust code.
- Short-circuit output rules — if output matches a pattern, replace with a compact message (e.g. "working tree clean" → "No changes"). Avoids sending verbose no-op output to agents.
- Evaluate hook-based interception (like RTK's PreToolUse hook) instead of/alongside wrapper scripts. Agents currently see `tkkubectl-dev` instead of `kubectl`, which reveals the guard layer and lets them attempt to call the underlying binary directly. A hook approach rewrites commands transparently — agents think they're running the real CLI. An agent just did this while testing toolkit, so it's possible: `Bash(env -u CLAUDECODE -u OPENCODE ...)`
- Have a human override for `toolkit guard` which removes restrictions. Alternatively, `toolkit env` which would provide the env vars so that a user can run it infront of a CLI tool
- Apparently agents can break out of the harness sandbox
- `bundle_run` parses stdout to extract the run ID from `Run URL: …`. Use the REST API instead.
- `cmd_config_template` for `dbr` shows `allow: []` / `deny: []` but those fields don't exist on `dbr::ConnConfig` — stale template copy from guard.
- `tkpsql` opens a fresh PG connection per query. Probably fine; revisit if latency becomes a complaint.
- SQL travels via argv. Add `--sql -` (stdin) or `--sql-file` to remove it from `/proc/PID/cmdline` and shell history. Doesn't fix harness-transcript exposure (structural to CLI mode).
- JSON output shapes are slightly inconsistent (`{"rows": …}` for `tkpsql tables` and `describe`, which aren't really rows). Cosmetic.
- Per-UID access policy (design decision pending): currently all authenticated UIDs get identical access — no way to say "agent A may query prod read-only, agent B may also trigger jobs." Options include per-UID allowlists in the daemon config keyed by tool/conn/op, or a capabilities token the caller presents. Needs a design decision before implementation.
- Two async runtimes: `tkpsql` is sync (`postgres`); `tkmsql` is async (`tiberius` + tokio). Real but small cost (compile time, binary size). Only worth merging if SQL paths get consolidated into a shared crate.
