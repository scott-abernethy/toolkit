# Harness Hook Recipes

Defence-in-depth configuration snippets for AI agent harnesses. These sit **outside** toolkit — they live in your harness settings and act before any toolkit binary runs.

## Why hooks matter

Toolkit's built-in protections stop agents from running `toolkit config show` or similar privileged commands directly. But the agent still shares your UID, so an agent could:

- Read the toolkit config at `~/.config/toolkit/config.yaml` using the harness's **Read** tool
- Read `.env` files or other credential stores unrelated to toolkit

Harness hooks address these gaps by blocking specific operations before they reach the filesystem.

## Scope and limitations

These recipes are **defence-in-depth**, not a sandbox:

- **Read tool hooks** (Claude) and **read deny rules** (opencode) provide strong per-path blocking for direct file reads.
- **Bash hooks** provide best-effort blocking for common read verbs (`cat`, `head`, `tail`, etc.) against known credential directories. They do **not** catch: shell redirections (`< ~/.aws/credentials`), `awk`/`sed`, scripting languages, `find -exec`, or other indirect access.
- `toolkit` already blocks agents from running its own management commands (via `reject_if_agent()` which detects known harness env vars).
- These recipes do not protect secrets that appear in shell history, log files, or agent transcripts.

The structural fix for a hostile agent is running the agent under a separate UID (see issue #6, step c — daemon transport).

## What is protected

| Path | Notes |
|------|-------|
| `~/.config/toolkit/` | Toolkit config (contains all credentials, owned by `_toolkit` daemon user) |
| `~/.ssh/` | SSH private keys |
| `~/.aws/` | AWS credentials and config |
| `~/.gnupg/` | GPG keys |
| `~/.kube/` | Kubernetes kubeconfig (may contain cluster credentials) |
| `~/.azure/` | Azure CLI auth tokens |
| `~/.config/gcloud/` | Google Cloud credentials |
| `~/.config/gh/` | GitHub CLI auth token |
| `~/.databrickscfg` | Databricks CLI config |
| `~/.netrc` | netrc credentials |
| `~/.npmrc` | npm auth tokens |
| `~/.pypirc` | PyPI upload credentials |
| `~/.git-credentials` | Git credential store |
| `~/.docker/config.json` | Docker registry auth |
| `.env`, `.env.*` | Project env files (excluding `.env.example`) |

## Claude Code

### Prerequisites

- `jq` installed (`brew install jq`)
- Hooks installed: `just install-hooks`

### Hook scripts

Two hook scripts are installed to `~/.config/toolkit/hooks/`:

**`read-guard`** (primary) — blocks the Claude Code `Read` tool from accessing credential paths listed above. Fail-closed: blocks on internal error.

**`bash-guard`** (secondary) — blocks the `Bash` tool from running `toolkit` directly, and blocks common file-reading commands (`cat`, `head`, `tail`, `less`, `more`, `bat`, `nano`, `vim`, `emacs`) when targeting credential directories. Best-effort only.

### Settings

The `permissions.deny` list stops the most common attack vectors without needing a hook script:

```json
{
  "permissions": {
    "deny": [
      "Bash(toolkit:*)"
    ]
  }
}
```

The hooks add a second layer for more nuanced cases. The full snippet is in `hooks/claude-code/settings.snippet.json`.

### Applying the settings

```sh
# Install hook scripts first
just install-hooks

# If you have no existing ~/.claude/settings.json:
cp hooks/claude-code/settings.snippet.json ~/.claude/settings.json

# If you have an existing settings file, merge the blocks manually.
# The snippet adds two top-level keys: "permissions" and "hooks".
```

Note: if you already have `permissions.deny` entries or `hooks.PreToolUse` entries, merge carefully — you need to combine the arrays rather than replace them.

The `_comment` key in the snippet is not a Claude Code feature; remove it after merging.

## opencode

opencode's permission system provides granular per-tool allow/deny rules in `~/.config/opencode/opencode.json`.

### Key settings

```json
{
  "permission": {
    "bash": {
      "*": "ask",
      "toolkit *": "deny"
    },
    "read": {
      "*": "allow",
      "*.env": "deny",
      "*.env.*": "deny",
      "*.env.example": "allow",
      "~/.config/toolkit/**": "deny",
      "~/.ssh/**": "deny",
      "~/.aws/**": "deny"
    },
    "external_directory": {
      "*": "ask"
    }
  }
}
```

**Rule evaluation:** last matching rule wins. `.env.example` must appear after `.env.*` to override it.

**`bash: {"*": "ask"}`** means the agent asks permission before running any shell command. This is the conservative setting. To allow bash freely but still deny specific commands, swap `"*": "ask"` for `"*": "allow"`.

The full snippet is in `hooks/opencode/opencode.snippet.json`. Merge the `permission` block into your existing `opencode.json`.

### Applying the settings

```sh
# Merge the permission block into your opencode.json.
# The snippet is a full opencode.json fragment — take the "permission" value
# and merge it into your existing "permission" config.
jq -s '.[0] * {"permission": (.[0].permission + .[1].permission)}' \
  ~/.config/opencode/opencode.json hooks/opencode/opencode.snippet.json \
  > /tmp/merged.json
mv /tmp/merged.json ~/.config/opencode/opencode.json
```

## GitHub Copilot CLI

GitHub Copilot CLI does not expose a per-command deny list in `~/.copilot/settings.json`.

**Available protections:**

1. **`toolkit reject_if_agent()`** — the `toolkit` binary detects the `COPILOT_CLI` and `COPILOT_RUN_APP` environment variables and refuses to run management commands (config, init, install) when they are set. This is the primary programmatic control.

2. **`copilot-instructions.md`** — `~/.copilot/copilot-instructions.md` can instruct the agent to avoid specific files and commands. This is advisory (model-level), not enforced at the tool level, but reduces accidental access:

```markdown
## Security constraints

Do not read files outside the current project directory without explicit user instruction.
Do not run: toolkit config.
Do not read: ~/.config/toolkit, ~/.ssh, ~/.aws, ~/.gnupg, .env files.
```

3. **`trustedFolders`** — only project directories listed in `~/.copilot/settings.json` under `trustedFolders` are treated as trusted. Avoid adding `~/.config` or home directory paths.

For stronger protection under Copilot CLI, the structural answer is the daemon transport which moves credentials behind a different UID.

## macOS: Touch ID boundary

The hook recipes above protect against accidental access. For the stronger guarantee (defeating a hostile agent), the daemon transport with Touch ID-gated `sudo` is the intended boundary.

Key points:
- `_toolkit` system user owns the config file (mode 0600)
- `toolkit-admin` requires Touch ID (`pam_tid.so` in `/etc/pam.d/sudo_local`)
- GUI screen-sharing and SSH sessions do **not** get Touch ID — document this gap prominently
