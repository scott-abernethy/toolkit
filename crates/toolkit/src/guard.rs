// Guard logic lives in common::guard. This module re-exports for use by the
// toolkit binary and adds the daemon-based config loader.

pub use common::guard::{check_rules, run, ConnConfig};

use common::protocol::Request;
use common::Result;

/// Load guard config from the daemon via UNIX socket.
/// The daemon reads its config (inaccessible to the agent UID) and returns
/// the ConnConfig as JSON. Rule checking and CLI execution happen locally.
pub fn load_config(app: &str, conn: Option<&str>) -> Result<ConnConfig> {
    let req = Request::new(
        "guard",
        conn.map(|s| s.to_string()),
        "config",
        serde_json::json!({ "app": app }),
    );
    let value = common::client::send(&req)?;
    serde_json::from_value(value)
        .map_err(|e| common::ToolkitError::other(format!("invalid guard config from daemon: {e}")))
}
