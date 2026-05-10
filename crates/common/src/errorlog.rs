use std::io::Write;
use std::path::PathBuf;

const DAEMON_HOME: &str = "/var/lib/toolkit";

/// Append a raw error message to the toolkit error log.
///
/// The log lives under the daemon-owned `_toolkit` home directory so agent-side
/// processes never write into the invoking user's home directory.
///
/// Fails silently — a logging failure must never propagate as a tool error.
pub fn append(context: &str, raw: &str) {
    let path = path();

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Flatten multi-line output onto one log line for easy grep/tail usage.
    let flat = raw
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" | ");

    let entry = format!("{ts} [{context}] {flat}\n");

    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    if let Ok(mut f) = options.open(&path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        let _ = f.write_all(entry.as_bytes());
    }
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
}
