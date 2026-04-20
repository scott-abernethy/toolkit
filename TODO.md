## TODO

- Output filter pipeline for `toolkit proxy` — configurable per-connection in YAML (strip_ansi, strip_lines regex, max_lines, truncate_lines_at). Declarative filters, not per-tool Rust code.
- Short-circuit output rules — if output matches a pattern, replace with a compact message (e.g. "working tree clean" → "No changes"). Avoids sending verbose no-op output to agents.
- Evaluate hook-based interception (like RTK's PreToolUse hook) instead of/alongside wrapper scripts. Agents currently see `tkkubectl-dev` instead of `kubectl`, which reveals the proxy and lets them attempt to call the underlying binary directly. A hook approach rewrites commands transparently — agents think they're running the real CLI.
