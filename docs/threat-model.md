# Threat Model

Toolkit is a **safety layer** between AI agents and sensitive services. It reduces credential exposure and constrains command/query actions, but it is not a full sandbox.

## Security objectives

1. Keep credentials out of agent-visible files, argv, env, and output.
2. Prevent accidental destructive actions from agent-issued commands.
3. Keep enforcement deterministic and auditable.

## Trust boundaries

### Daemon boundary (credential boundary)

- `toolkit-daemon` runs as `_toolkit`.
- Config lives at `/var/lib/toolkit/.config/toolkit/config.yaml` with daemon-only permissions.
- Agent UIDs talk to the daemon over a UNIX socket.
- Peer UID checks (`getpeereid` / `SO_PEERCRED`) gate socket access.

### Tool boundary (action boundary)

- Native clients (`tkpsql`, `tkmsql`, `tkdbr`) dispatch typed requests via daemon operations.
- `toolkit guard` applies token-based allow/deny rules before invoking wrapped CLIs.
- SQL write checks and table allowlists run before backend execution.

### Harness boundary (defence-in-depth)

- Harness hooks and deny rules can block direct reads of credential paths and direct management commands before execution.
- This is supplementary to daemon/tool boundaries, not a substitute.

## Attacker model

Toolkit is designed for a **non-malicious or semi-trusted agent** that may:

- run unintended commands,
- over-query data,
- accidentally expose secrets in transcripts.

Toolkit is **not** designed to fully defeat:

- a malicious local user controlling the workstation,
- arbitrary code execution under the same privileged user identity as secrets,
- unrestricted network exfiltration once data is already obtained.

## What toolkit enforces

- Credentials are centrally stored and read by daemon user only.
- Client binaries do not directly read daemon config from disk.
- Daemon dispatch requires known tool/op pairs (typed decode + explicit handlers).
- SQL write-intent checks for configured tools.
- Guard command allow/deny enforcement with deterministic token matching.

## What toolkit does not enforce

- **Per-connection authorization by UID.** `allowed_uids` is coarse-grained.
- **Network egress controls.** Use OS/network controls for outbound restrictions.
- **Inference resistance.** Read-only access can still leak sensitive business context.
- **Prompt/content-level PII redaction.** Toolkit focuses on credential and action boundaries.
- **A full host sandbox.** Harness/process sandboxing remains separate.

## Known bypass and failure considerations

- If a user has local root/admin control, daemon/config protections can be altered.
- Harness hooks are configurable by local users and should be treated as defence-in-depth.
- Any direct backend access path outside toolkit controls is out of toolkit scope.

## Recommended layered deployment

1. **Service-side least privilege first** (read-only roles, IAM scopes, GRANTs).
2. **Toolkit daemon + tool enforcement** (credential isolation, typed dispatch, guard rules).
3. **Harness hooks/deny rules** (`toolkit init` + harness-specific controls).
4. **OS/process sandboxing** (container, bwrap, sandbox-exec) where possible.
5. **Audit review** (daemon logs + regular `toolkit validate` checks).
