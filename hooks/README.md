# Harness Hook Recipes

Defence-in-depth configuration for AI agent harnesses. These recipes add a second layer of protection around toolkit's own runtime checks.

## Coverage

| Threat | Mechanism |
|--------|-----------|
| Agent runs `sops`/`age` directly | `permissions.deny` (Claude) / bash deny (opencode) |
| Agent reads `~/.config/toolkit` or `~/.config/sops` via Read tool | Read hook / opencode read deny |
| Agent reads `~/.ssh`, `~/.aws`, `~/.gnupg`, etc. | Read hook / opencode read deny |
| Agent reads `.env` files | Read hook / opencode read deny |
| Agent runs `toolkit config show` | Blocked by toolkit's `reject_if_agent()` + `Bash(toolkit:*)` deny |
| Agent uses `cat`/`head`/`tail` on credential paths | Bash hook (best-effort) |

**Not covered:** awk, python, shell redirects, `find -exec`, and other indirect access paths. The Read tool hook provides the primary per-path protection; the bash hook is supplementary.

## Installation

### 1. Install hook scripts

```sh
just install-hooks
```

This copies `bash-guard` and `read-guard` to `~/.config/toolkit/hooks/`.

### 2. Apply harness configuration

**Claude Code** — merge `claude-code/settings.snippet.json` into `~/.claude/settings.json`:

```sh
# If you have no existing settings:
cp hooks/claude-code/settings.snippet.json ~/.claude/settings.json

# If you have existing settings, merge manually (jq example):
jq -s '.[0] * .[1]' ~/.claude/settings.json hooks/claude-code/settings.snippet.json > /tmp/merged.json
mv /tmp/merged.json ~/.claude/settings.json
```

**opencode** — merge the `permission` block from `opencode/opencode.snippet.json` into `~/.config/opencode/opencode.json`.

**Copilot CLI** — no deny-list equivalent in settings. The `toolkit` binary's built-in `reject_if_agent()` check covers the primary threat (agents running `toolkit config show`). See `docs/hooks.md` for details.
