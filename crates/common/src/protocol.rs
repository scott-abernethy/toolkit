use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A request sent from a CLI tool to the daemon over the UNIX socket.
/// Serialised as a single-line JSON object followed by a newline.
#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    /// Which tool handles this request ("psql", "msql", "dbr").
    pub tool: String,
    /// Named connection (e.g. "local", "prod"). None → single-connection auto-select.
    pub conn: Option<String>,
    /// Operation name (e.g. "query", "tables", "describe", "catalogs").
    pub op: String,
    /// Operation-specific parameters (e.g. {"sql": "SELECT 1"}, {"table": "users"}).
    pub params: Value,
}

/// Response returned from the daemon to the caller.
#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok(result: Value) -> Self {
        Self {
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(msg.into()),
        }
    }
}
