# Toolkit Daemon

The toolkit daemon (`toolkit-daemon`) is a long-running process that holds credentials
and dispatches tool requests over a UNIX socket. AI agents connect to the socket; the
daemon reads the config file and calls the appropriate library on their behalf.

All toolkit operations route through the daemon — native clients (`tkpsql`, `tkmsql`,
`tkdbr`) send requests directly, and `toolkit guard` fetches config from the daemon
before executing wrapped CLIs locally.

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

### Quick start (Homebrew — macOS)

```sh
# 1. Install toolkit via your private tap
brew install <tap>/toolkit

# 2. Run the privileged setup script (creates _toolkit user, installs LaunchDaemon)
sudo $(brew --prefix)/opt/toolkit/libexec/setup-daemon.sh

# 3. Add your connections to the daemon config
toolkit config edit

# 4. Verify the daemon is running
toolkit status
```

The setup script is idempotent — safe to re-run. After `brew upgrade toolkit`,
re-run it to update the root-owned daemon binary at `/usr/local/bin/toolkit-daemon`.

> **Security note:** The setup script lives in the Homebrew prefix (user-writable).
> Run it immediately after `brew install` — before starting any agent session — to
> prevent a hostile agent from tampering with it prior to the `sudo` invocation.

For Databricks OAuth login, run as yourself (the browser opens on your desktop):
```sh
tkdbr auth login --conn <name>
```
The PKCE flow runs locally, and the resulting tokens are stored by the daemon in its secure home directory. Tokens are automatically refreshed before expiry on subsequent calls.

---

### Manual setup

#### 1. Create the `_toolkit` system user

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

#### 2. Write the config file as `_toolkit`

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

#### 3. Install the daemon binary

```sh
cargo build --release -p toolkit-daemon
sudo cp target/release/toolkit-daemon /usr/local/bin/toolkit-daemon
sudo chown root:root /usr/local/bin/toolkit-daemon
```

#### 4. Start the daemon

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

#### 5. Verify

```sh
# From your agent UID:
tkpsql tables            # routes through daemon
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

## Databricks OAuth login (`tkdbr auth login`)

`tkdbr auth login` runs the native Databricks OAuth U2M (PKCE) flow entirely in Rust — no `databricks` CLI auth required. It:

1. Generates a PKCE verifier/challenge pair and random state
2. Prints an authorization URL for the user to open in a browser
3. Listens locally on port 8020–8030 for the OAuth redirect callback
4. Exchanges the code for access + refresh tokens
5. Sends the tokens to the daemon via socket (`auth/store_tokens`)
6. Daemon stores tokens at `/var/lib/toolkit/.config/toolkit/dbr-oauth/<conn>.json` (mode 0600, readable only by `_toolkit`)

Tokens are auto-refreshed before expiry on every `tkdbr` call, so `tkdbr auth login` only needs to be re-run when the refresh token expires (typically after 30–90 days depending on workspace policy).

Run as yourself — the browser opens on your desktop:

```sh
tkdbr auth login --conn dev
```

## Known limitations

- `toolkit-admin` tooling for managing the `_toolkit` config is not yet fully implemented.
  Use `sudo -u _toolkit` for initial setup.
