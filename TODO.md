## TODO

- Output filter pipeline for `toolkit guard` — configurable per-connection in YAML (strip_ansi, strip_lines regex, max_lines, truncate_lines_at). Declarative filters, not per-tool Rust code.
- Short-circuit output rules — if output matches a pattern, replace with a compact message (e.g. "working tree clean" → "No changes"). Avoids sending verbose no-op output to agents.
- Evaluate hook-based interception (like RTK's PreToolUse hook) instead of/alongside wrapper scripts. Agents currently see `tkkubectl-dev` instead of `kubectl`, which reveals the guard layer and lets them attempt to call the underlying binary directly. A hook approach rewrites commands transparently — agents think they're running the real CLI. An agent just did this while testing toolkit, so it's possible: `Bash(env -u CLAUDECODE -u OPENCODE ...)`
- Have a human override for `toolkit guard` which removes restrictions. Alternatively, `toolkit env` which would provide the env vars so that a user can run it infront of a CLI tool
- Apparently agents can break out of the harness sandbox
