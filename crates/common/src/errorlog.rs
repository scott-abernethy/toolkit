use std::io::Write;

/// Append a raw error message to the toolkit error log.
///
/// The log lives at `$HOME/.local/share/toolkit/errors.log`. When called from
/// the daemon (running as `_toolkit`) this writes to that user's home directory,
/// keeping full error details out of agent-visible output.
///
/// Fails silently — a logging failure must never propagate as a tool error.
pub fn append(context: &str, raw: &str) {
    let Some(path) = log_path() else { return };

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

    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = f.write_all(entry.as_bytes());
    }
}

fn log_path() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        std::path::PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("toolkit")
            .join("errors.log"),
    )
}
