use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Wire protocol version. Bumped when an incompatible change is made.
/// Older requests that omit `version` are treated as version 1.
pub const PROTOCOL_VERSION: u32 = 1;

/// A request sent from a CLI tool to the daemon over the UNIX socket.
/// Serialised as a single-line JSON object followed by a newline.
#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    /// Wire protocol version. Defaults to 1 when omitted, so a daemon built
    /// from this revision still accepts requests from older CLI builds that
    /// pre-date the field. Incompatible changes must bump `PROTOCOL_VERSION`.
    #[serde(default = "default_version")]
    pub version: u32,
    /// Which tool handles this request ("psql", "msql", "dbr", "guard").
    pub tool: String,
    /// Named connection (e.g. "local", "prod"). None → single-connection auto-select.
    pub conn: Option<String>,
    /// Operation name (e.g. "query", "tables", "describe", "jobs/list").
    pub op: String,
    /// Operation-specific parameters. Decoded by the daemon into a typed enum
    /// per tool — the wire shape stays loosely-typed so older or newer fields
    /// don't break parsing on a mismatched build.
    #[serde(default)]
    pub params: Value,
}

fn default_version() -> u32 {
    1
}

impl Request {
    /// Build a request with the current protocol version.
    pub fn new(
        tool: impl Into<String>,
        conn: Option<String>,
        op: impl Into<String>,
        params: Value,
    ) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            tool: tool.into(),
            conn,
            op: op.into(),
            params,
        }
    }
}

/// Response returned from the daemon to the caller.
#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Server-side error category, used by the daemon for audit logging.
    /// Skipped on the wire — clients only ever see `error`.
    #[serde(skip)]
    pub error_class: Option<&'static str>,
}

impl Response {
    pub fn ok(result: Value) -> Self {
        Self {
            ok: true,
            result: Some(result),
            error: None,
            error_class: None,
        }
    }

    /// Build an error response without classifying the failure. The audit log
    /// will record this as `"unclassified"`. Prefer `err_class` for known
    /// categories so logs and metrics can aggregate on them.
    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(msg.into()),
            error_class: None,
        }
    }

    /// Build an error response with a stable category tag for audit logs.
    pub fn err_class(msg: impl Into<String>, class: &'static str) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(msg.into()),
            error_class: Some(class),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_request_omitted_version_defaults_to_one() {
        let req: Request = serde_json::from_value(json!({
            "tool": "psql",
            "conn": null,
            "op": "tables",
            "params": {}
        }))
        .unwrap();
        assert_eq!(req.version, 1);
    }

    #[test]
    fn test_request_explicit_version_preserved() {
        let req: Request = serde_json::from_value(json!({
            "version": 1,
            "tool": "psql",
            "conn": null,
            "op": "tables",
            "params": {}
        }))
        .unwrap();
        assert_eq!(req.version, 1);
    }

    #[test]
    fn test_request_new_uses_current_version() {
        let req = Request::new("psql", None, "tables", json!({}));
        assert_eq!(req.version, PROTOCOL_VERSION);
    }

    #[test]
    fn test_request_omitted_params_defaults_to_null() {
        let req: Request = serde_json::from_value(json!({
            "tool": "guard",
            "conn": null,
            "op": "list"
        }))
        .unwrap();
        assert!(req.params.is_null());
    }
}
