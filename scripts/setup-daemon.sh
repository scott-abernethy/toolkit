#!/usr/bin/env bash
#
# setup-daemon.sh — Privileged macOS setup for toolkit-daemon.
#
# Creates the _toolkit system user, installs a root-owned daemon binary, and
# registers a LaunchDaemon plist so the daemon starts at boot.
#
# Usage (run once after `brew install toolkit`; safe to re-run on upgrade):
#   sudo $(brew --prefix)/opt/toolkit/libexec/setup-daemon.sh
#
set -euo pipefail

TOOLKIT_USER="_toolkit"
TOOLKIT_HOME="/var/lib/toolkit"
DAEMON_BIN="/usr/local/bin/toolkit-daemon"
PLIST_PATH="/Library/LaunchDaemons/com.toolkit.daemon.plist"
LOG_PATH="/var/log/toolkit-daemon.log"
CONFIG_DIR="${TOOLKIT_HOME}/.config/toolkit"
CONFIG_FILE="${CONFIG_DIR}/config.yaml"

# ---------------------------------------------------------------------------
# Preflight checks
# ---------------------------------------------------------------------------

if [[ $EUID -ne 0 ]]; then
    echo "Error: This script must be run as root." >&2
    echo "       Use: sudo $0" >&2
    exit 1
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "Error: This script is for macOS only." >&2
    exit 1
fi

# Find the Homebrew-installed toolkit-daemon binary
BREW_DAEMON="$(command -v toolkit-daemon 2>/dev/null || true)"
if [[ -z "${BREW_DAEMON}" ]]; then
    echo "Error: toolkit-daemon not found in PATH." >&2
    echo "       Run 'brew install <tap>/toolkit' first." >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# Group and user creation (idempotent)
# ---------------------------------------------------------------------------

if id -u "${TOOLKIT_USER}" &>/dev/null; then
    echo "✓ ${TOOLKIT_USER} user already exists — skipping user/group creation."
    toolkit_uid="$(id -u "${TOOLKIT_USER}")"
    toolkit_gid="$(id -g "${TOOLKIT_USER}")"
else
    # Find a UID/GID pair that's free for both users and groups.
    candidate=400
    while dscl . -list /Users UniqueID | awk '{print $2}' | grep -qx "${candidate}" || \
          dscl . -list /Groups PrimaryGroupID | awk '{print $2}' | grep -qx "${candidate}"; do
        candidate=$((candidate + 1))
    done
    toolkit_uid="${candidate}"
    toolkit_gid="${candidate}"

    # Create group if it doesn't already exist
    if ! dscl . -read /Groups/"${TOOLKIT_USER}" &>/dev/null; then
        echo "Creating group ${TOOLKIT_USER} with GID ${toolkit_gid}..."
        dscl . -create /Groups/"${TOOLKIT_USER}"
        dscl . -create /Groups/"${TOOLKIT_USER}" PrimaryGroupID "${toolkit_gid}"
        dscl . -create /Groups/"${TOOLKIT_USER}" RealName "_Toolkit Daemon"
        dscl . -create /Groups/"${TOOLKIT_USER}" Password "*"
    fi

    echo "Creating user ${TOOLKIT_USER} with UID ${toolkit_uid}..."
    dscl . -create /Users/"${TOOLKIT_USER}"
    dscl . -create /Users/"${TOOLKIT_USER}" UniqueID "${toolkit_uid}"
    dscl . -create /Users/"${TOOLKIT_USER}" PrimaryGroupID "${toolkit_gid}"
    dscl . -create /Users/"${TOOLKIT_USER}" UserShell /usr/bin/false
    dscl . -create /Users/"${TOOLKIT_USER}" NFSHomeDirectory "${TOOLKIT_HOME}"
    dscl . -create /Users/"${TOOLKIT_USER}" RealName "_Toolkit Daemon"
    dscl . -create /Users/"${TOOLKIT_USER}" Password "*"
fi

# ---------------------------------------------------------------------------
# Home directory
# ---------------------------------------------------------------------------

if [[ ! -d "${TOOLKIT_HOME}" ]]; then
    echo "Creating ${TOOLKIT_HOME}..."
    mkdir -p "${TOOLKIT_HOME}"
fi
chown "${TOOLKIT_USER}:${TOOLKIT_USER}" "${TOOLKIT_HOME}"
chmod 700 "${TOOLKIT_HOME}"

# ---------------------------------------------------------------------------
# Config directory and template config
# ---------------------------------------------------------------------------

echo "Setting up config directory..."
sudo -u "${TOOLKIT_USER}" mkdir -p "${CONFIG_DIR}"
sudo -u "${TOOLKIT_USER}" chmod 700 "${CONFIG_DIR}"

if [[ ! -f "${CONFIG_FILE}" ]]; then
    echo "Writing template config to ${CONFIG_FILE}..."
    sudo -u "${TOOLKIT_USER}" tee "${CONFIG_FILE}" > /dev/null <<'CONF'
# toolkit daemon config — owned by _toolkit, not readable by agent UIDs.
# Edit with: toolkit config edit
#
# Example PostgreSQL connection:
# psql:
#   prod:
#     host: db.example.com
#     port: 5432
#     database: mydb
#     user: readonly
#     password: "s3cr3t"
#
# Example Databricks connection:
# dbr:
#   dev:
#     env:
#       DATABRICKS_HOST: https://dbc-abc123.cloud.databricks.com
#       DATABRICKS_TOKEN: dapi...
#
# Optional: restrict which agent UIDs may connect (omit to allow all local users):
# daemon:
#   allowed_uids: [501]
CONF
    sudo -u "${TOOLKIT_USER}" chmod 600 "${CONFIG_FILE}"
fi

# ---------------------------------------------------------------------------
# Daemon binary (root-owned, outside Homebrew prefix)
# ---------------------------------------------------------------------------

echo "Installing daemon binary to ${DAEMON_BIN}..."
cp "${BREW_DAEMON}" "${DAEMON_BIN}"
chown root:wheel "${DAEMON_BIN}"
chmod 755 "${DAEMON_BIN}"

# ---------------------------------------------------------------------------
# Log file
# ---------------------------------------------------------------------------

touch "${LOG_PATH}"
chown "${TOOLKIT_USER}:wheel" "${LOG_PATH}"
chmod 640 "${LOG_PATH}"

# ---------------------------------------------------------------------------
# LaunchDaemon plist
# ---------------------------------------------------------------------------

echo "Installing LaunchDaemon plist to ${PLIST_PATH}..."
cat > "${PLIST_PATH}" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>             <string>com.toolkit.daemon</string>
  <key>ProgramArguments</key>
  <array>
    <string>${DAEMON_BIN}</string>
  </array>
  <key>UserName</key>          <string>${TOOLKIT_USER}</string>
  <key>RunAtLoad</key>         <true/>
  <key>KeepAlive</key>         <true/>
  <key>EnvironmentVariables</key>
  <dict>
    <key>HOME</key>            <string>${TOOLKIT_HOME}</string>
  </dict>
  <key>StandardErrorPath</key> <string>${LOG_PATH}</string>
</dict>
</plist>
PLIST
chown root:wheel "${PLIST_PATH}"
chmod 644 "${PLIST_PATH}"

# ---------------------------------------------------------------------------
# Load (or reload) the daemon
# ---------------------------------------------------------------------------

echo "Loading daemon..."
if launchctl list 2>/dev/null | grep -q "com.toolkit.daemon"; then
    launchctl unload "${PLIST_PATH}" 2>/dev/null || true
fi
launchctl load "${PLIST_PATH}"

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------

echo ""
echo "✓ Daemon setup complete."
echo ""
echo "Next steps:"
echo "  1. Configure agent harness protections:"
echo "       toolkit init"
echo "  2. Add your connections to the daemon config:"
echo "       toolkit config edit"
echo "  3. Verify the daemon is running:"
echo "       toolkit status"
echo ""
echo "After 'brew upgrade toolkit', re-run setup to update the daemon binary:"
echo "  toolkit setup"
