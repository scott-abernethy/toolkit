# Toolkit Daemon

The toolkit daemon (`toolkit-daemon`) is a long-running process that holds credentials
and dispatches tool requests over a UNIX socket. AI agents connect to the socket; the
daemon reads the config file and calls the appropriate library on their behalf.

## Threat model

| What the daemon protects against |
|---|
| Agent reads `~/.config/toolkit/config.yaml` directly |
| Agent exfiltrates credentials via `env`, `cat`, or file-read hooks |
| Agent bypasses write-protect by constructing raw DB connections |

The OS enforces the boundary: the daemon runs as a separate OS user (`_toolkit`) whose
config directory is mode `0700`. The agent UID cannot read it.

## How it works

```
 Agent UID (e.g. 501)          _toolkit UID (e.g. 400)
 ────────────────────          ───────────────────────
 tkpsql query --sql "…"        toolkit-daemon
     │                              │
     │  {"tool":"psql","op":"query",│
     │   "params":{"sql":"…"}}      │
     └──────────── UNIX socket ────►│
                                    │ reads ~_toolkit/.config/toolkit/config.yaml
                                    │ calls tkpsql::run_query(...)
                                    │
     {"rows":[…],"count":3}         │
     ◄──────────────────────────────┘
```

## Setup

### 1. Create the `_toolkit` system user

**macOS:**
```sh
# Pick a UID not already in use (check with: dscl . list /Users UniqueID)
sudo dscl . create /Users/_toolkit
sudo dscl . create /Users/_toolkit UniqueID 400
sudo dscl . create /Users/_toolkit PrimaryGroupID 400
sudo dscl . create /Users/_toolkit UserShell /usr/bin/false
sudo dscl . create /Users/_toolkit NFSHomeDirectory /var/lib/toolkit
sudo mkdir -p /var/lib/toolkit
sudo chown -R _toolkit:_toolkit /var/lib/toolkit
```

**Linux (Debian/Ubuntu):**
```sh
sudo adduser --system --no-create-home --home /var/lib/toolkit \
             --shell /usr/sbin/nologin _toolkit
sudo mkdir -p /var/lib/toolkit
sudo chown -R _toolkit:_toolkit /var/lib/toolkit
```

### 2. Write the config file as `_toolkit`

```sh
sudo -u _toolkit mkdir -p /var/lib/toolkit/.config/toolkit
sudo -u _toolkit chmod 700 /var/lib/toolkit/.config/toolkit
```

Create `/var/lib/toolkit/.config/toolkit/config.yaml`:

```yaml
# Written as _toolkit; never readable by the agent UID.
psql:
  prod:
    host: db.example.com
    port: 5432
    database: mydb
    user: readonly
    password: "s3cr3t"

dbr:
  dev:
    env:
      DATABRICKS_HOST: https://dbc-abc123.cloud.databricks.com
      DATABRICKS_TOKEN: dapi...

# Optional: restrict which UIDs may connect (omit to allow all local users).
daemon:
  allowed_uids: [501, 502]
```

```sh
sudo chown _toolkit:_toolkit /var/lib/toolkit/.config/toolkit/config.yaml
sudo chmod 600 /var/lib/toolkit/.config/toolkit/config.yaml
```

### 3. Install the daemon binary

```sh
cargo build --release -p toolkit-daemon
sudo cp target/release/toolkit-daemon /usr/local/bin/toolkit-daemon
sudo chown root:root /usr/local/bin/toolkit-daemon
```

### 4. Start the daemon

**macOS — launchd plist** (`/Library/LaunchDaemons/com.toolkit.daemon.plist`):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>             <string>com.toolkit.daemon</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/local/bin/toolkit-daemon</string>
  </array>
  <key>UserName</key>          <string>_toolkit</string>
  <key>RunAtLoad</key>         <true/>
  <key>KeepAlive</key>         <true/>
  <key>EnvironmentVariables</key>
  <dict>
    <key>HOME</key>            <string>/var/lib/toolkit</string>
  </dict>
  <key>StandardErrorPath</key> <string>/var/log/toolkit-daemon.log</string>
</dict>
</plist>
```

```sh
sudo launchctl load /Library/LaunchDaemons/com.toolkit.daemon.plist
```

**Linux — systemd unit** (`/etc/systemd/system/toolkit-daemon.service`):

```ini
[Unit]
Description=Toolkit credential daemon
After=network.target

[Service]
User=_toolkit
ExecStart=/usr/local/bin/toolkit-daemon
Restart=on-failure
Environment=HOME=/var/lib/toolkit

[Install]
WantedBy=multi-user.target
```

```sh
sudo systemctl enable --now toolkit-daemon
```

### 5. Verify

```sh
# From your agent UID:
tkpsql tables            # routes through daemon
tkpsql --direct tables   # bypasses daemon (requires config read access)
```

## Configuration reference

The optional `[daemon]` section in `config.yaml`:

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `socket_path` | string | `/tmp/toolkit.sock` | UNIX socket path |
| `allowed_uids` | list of integers | (all) | UIDs permitted to connect |

The socket path can also be overridden at runtime with the `TOOLKIT_SOCKET` environment
variable (checked by both daemon and CLI tools).

**Asymmetry to be aware of**: the daemon resolves the socket path as
`daemon.socket_path` (config) → `$TOOLKIT_SOCKET` → default. The CLI client only
reads `$TOOLKIT_SOCKET` → default — it deliberately does not read the daemon's
config (the agent UID has no read access). If you customise `socket_path` in
the daemon config, you must also set `TOOLKIT_SOCKET` in the agent's
environment (e.g. via the user's shell profile) so its CLIs reach the socket.

## Touch ID / sudo authentication (macOS)

To require Touch ID for agent-to-daemon connections, use `sudo` as the transport wrapper:

1. Add the agent user to `sudoers` with `NOPASSWD` for only `toolkit-daemon` operations,
   OR configure `pam_tid.so` in `/etc/pam.d/sudo` for biometric confirmation.

This is out of scope for the daemon itself but is a natural next layer.

## `--direct` flag

All toolkit CLI tools accept `--direct` to bypass the daemon and call the library
directly. This requires the calling user to have read access to the config file.

Use `--direct` for:
- Initial daemon setup and testing
- `tkdbr auth login` (interactive browser flow, not suitable for daemon dispatch)
- Troubleshooting

## Known limitations

- `tkdbr auth login` performs an interactive browser OAuth flow and must be run with
  `--direct` by the `_toolkit` user directly, not via the daemon.
- `toolkit-admin` tooling for managing the `_toolkit` config is not yet implemented.
  Use `sudo -u _toolkit` for initial setup.
