use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use serde_json::Value;

use crate::error::{Result, ToolkitError};
use crate::protocol::{Request, Response};

/// Default socket path. Override with `TOOLKIT_SOCKET` environment variable.
pub const DEFAULT_SOCKET: &str = "/tmp/toolkit.sock";

/// Send a request to the toolkit daemon and return the result value.
///
/// **Fail-closed**: if the daemon is unreachable, returns `Err(ToolkitError::Daemon)`.
/// There is no automatic fallback to direct mode — use `--direct` for that.
pub fn send(req: &Request) -> Result<Value> {
    let socket_path = std::env::var("TOOLKIT_SOCKET")
        .unwrap_or_else(|_| DEFAULT_SOCKET.to_owned());

    let mut stream = UnixStream::connect(&socket_path).map_err(|e| {
        ToolkitError::daemon(format!(
            "cannot reach toolkit daemon at {socket_path}: {e}. \
             Start the daemon with `toolkit-daemon` or pass --direct to bypass."
        ))
    })?;

    stream
        .set_read_timeout(Some(Duration::from_secs(120)))
        .map_err(|e| ToolkitError::daemon(format!("socket timeout: {e}")))?;

    let line = serde_json::to_string(req)
        .map_err(|e| ToolkitError::other(format!("request encode: {e}")))?;

    stream
        .write_all(format!("{line}\n").as_bytes())
        .map_err(|e| ToolkitError::daemon(format!("write to socket: {e}")))?;

    let mut reader = BufReader::new(stream);
    let mut response_line = String::new();
    let n = reader
        .read_line(&mut response_line)
        .map_err(|e| ToolkitError::daemon(format!("read from socket: {e}")))?;
    if n > 1 << 20 {
        return Err(ToolkitError::daemon("daemon response too large (> 1 MiB)"));
    }

    let resp: Response = serde_json::from_str(response_line.trim()).map_err(|e| {
        ToolkitError::daemon(format!("invalid response from daemon: {e}"))
    })?;

    if resp.ok {
        Ok(resp.result.unwrap_or(Value::Null))
    } else {
        Err(ToolkitError::other(
            resp.error.unwrap_or_else(|| "daemon returned error".into()),
        ))
    }
}
