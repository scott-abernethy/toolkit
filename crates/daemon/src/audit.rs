use std::io::Write;
use std::time::Duration;

use common::protocol::Response;

/// Append one JSONL audit entry to `path`.
///
/// Each line is a JSON object: `{ts, uid, tool, conn, op, ok, error_class, duration_ms}`.
/// `ts` is seconds since the Unix epoch (UTC). Fields unavailable at the time of
/// writing (e.g. tool/op when parsing the request failed) are emitted as `null`.
///
/// Fails silently — audit I/O must never block or propagate as a tool error.
pub fn write(
    path: Option<&str>,
    uid: Option<u32>,
    tool: Option<&str>,
    conn: Option<&str>,
    op: Option<&str>,
    resp: &Response,
    elapsed: Duration,
) {
    let Some(path) = path else { return };

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let entry = serde_json::json!({
        "ts": ts,
        "uid": uid,
        "tool": tool,
        "conn": conn,
        "op": op,
        "ok": resp.ok,
        "error_class": resp.error_class,
        "duration_ms": elapsed.as_millis(),
    });

    let Ok(mut line) = serde_json::to_string(&entry) else {
        return;
    };
    line.push('\n');

    if let Some(parent) = std::path::Path::new(path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}
