use std::io::Write;
use std::path::PathBuf;

use crate::client;
use crate::protocol::Request;

const DAEMON_HOME: &str = "/var/lib/toolkit";

/// Cap on the flattened, single-line entry we persist or relay to the daemon.
/// Bundle/build failures can be very verbose; keep the tail since the actual
/// error usually appears after progress noise (e.g. "Building wheel...").
const MAX_MESSAGE_LEN: usize = 32 * 1024;

/// Append a raw error message to the toolkit error log.
///
/// The log lives under the daemon-owned `_toolkit` home directory so agent-side
/// processes never write into the invoking user's home directory.
///
/// Tries a direct write first — the fast path when this runs inside the
/// daemon process, which owns `/var/lib/toolkit`. Callers that execute as a
/// different UID (e.g. `tkdbr bundle deploy`, which runs the databricks CLI
/// locally to preserve streaming and the bundle's working directory) can't
/// write there directly; on failure this relays the entry to the daemon over
/// the socket, which writes it with its own privileges.
///
/// Fails silently either way — a logging failure must never propagate as a
/// tool error.
pub fn append(context: &str, raw: &str) {
    let flat = flatten(raw);
    if write_local(context, &flat) {
        return;
    }
    relay_to_daemon(context, &flat);
}

/// Flatten multi-line output onto one line for easy grep/tail usage, and cap
/// its length, keeping the tail where the real error usually lives.
fn flatten(raw: &str) -> String {
    let flat = raw
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" | ");

    if flat.len() > MAX_MESSAGE_LEN {
        let start = flat.len() - MAX_MESSAGE_LEN;
        format!("...[truncated]... {}", &flat[start..])
    } else {
        flat
    }
}

/// Write a pre-flattened entry directly to the log file. Returns `true` on success.
fn write_local(context: &str, flat: &str) -> bool {
    let path = path();

    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let entry = format!("{ts} [{context}] {flat}\n");

    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    match options.open(&path) {
        Ok(mut f) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
            f.write_all(entry.as_bytes()).is_ok()
        }
        Err(_) => false,
    }
}

/// Best-effort relay to the daemon, which can write the log with its own
/// (`_toolkit`) privileges. Never surfaces an error to the caller.
fn relay_to_daemon(context: &str, flat: &str) {
    let req = Request::new(
        "meta",
        None,
        "log/append",
        serde_json::json!({"context": context, "message": flat}),
    );
    let _ = client::send(&req);
}

/// Absolute path to the daemon-owned error log.
pub fn path() -> PathBuf {
    PathBuf::from(DAEMON_HOME)
        .join(".local")
        .join("share")
        .join("toolkit")
        .join("errors.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_log_path_is_daemon_owned() {
        assert_eq!(
            path(),
            PathBuf::from("/var/lib/toolkit/.local/share/toolkit/errors.log")
        );
    }

    #[test]
    fn flatten_joins_lines_and_trims() {
        assert_eq!(flatten("  a  \n\nb\n  c  "), "a | b | c");
    }

    #[test]
    fn flatten_truncates_long_input_keeping_tail() {
        let raw = "x".repeat(MAX_MESSAGE_LEN + 100);
        let flat = flatten(&raw);
        assert!(flat.starts_with("...[truncated]... "));
        assert!(flat.len() <= MAX_MESSAGE_LEN + "...[truncated]... ".len());
        assert!(flat.ends_with('x'));
    }
}
