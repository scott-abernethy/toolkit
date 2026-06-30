//! toolkit-mcp — a Model Context Protocol (MCP) server that fronts the toolkit
//! daemon.
//!
//! It speaks newline-delimited JSON-RPC 2.0 over stdio (the MCP stdio transport)
//! and maps each `tools/call` onto a toolkit wire [`Request`], forwarding it to
//! the daemon via `common::client::send`. The set of tools is the static
//! [`catalog`], which mirrors the `tk*` CLIs one-for-one.
//!
//! **Trust boundary:** this binary is agent-facing and has the *exact* same
//! privileges as `tkpsql` — it links only `common` (the daemon client +
//! protocol), never `libtoolkit`, and never reads the daemon config. All
//! credentials and all enforcement stay behind the daemon socket. stdout is
//! reserved for the protocol; diagnostics go to stderr.

mod catalog;

use std::io::{BufRead, Write};

use common::protocol::Request;
use serde_json::{json, Map, Value};

/// MCP protocol version this server defaults to when a client does not request
/// one. When the client does send `protocolVersion`, we echo it back: as a thin
/// proxy we depend on no version-specific behaviour, so echoing maximises
/// interoperability.
const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";

// JSON-RPC 2.0 error codes.
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// Transport to the toolkit daemon. Abstracted so the message handler is unit-
/// testable without a running daemon.
trait Backend {
    fn send(&self, req: &Request) -> common::Result<Value>;
}

/// Production backend: forwards to the daemon over the UNIX socket.
struct DaemonBackend;

impl Backend for DaemonBackend {
    fn send(&self, req: &Request) -> common::Result<Value> {
        common::client::send(req)
    }
}

fn main() {
    let backend = DaemonBackend;
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("toolkit-mcp: stdin read error: {e}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_message(&backend, &line) {
            if writeln!(out, "{response}")
                .and_then(|_| out.flush())
                .is_err()
            {
                break; // stdout closed — client went away.
            }
        }
    }
}

/// Handle one JSON-RPC message line. Returns the response line to write, or
/// `None` for notifications (no `id`) and other no-reply cases.
fn handle_message(backend: &dyn Backend, line: &str) -> Option<String> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Some(error_response(
                Value::Null,
                PARSE_ERROR,
                &format!("parse error: {e}"),
            ));
        }
    };

    let method = msg.get("method").and_then(Value::as_str);
    // A request has an `id`; a notification does not. Per JSON-RPC, never reply
    // to a notification.
    let id = msg.get("id").cloned();
    let is_notification = id.is_none();

    let Some(method) = method else {
        if is_notification {
            return None;
        }
        return Some(error_response(
            id.unwrap_or(Value::Null),
            INVALID_REQUEST,
            "missing method",
        ));
    };

    let params = msg.get("params").cloned().unwrap_or(Value::Null);

    // Notifications: act if relevant, never respond.
    if is_notification {
        // `notifications/initialized`, `notifications/cancelled`, etc. require
        // no action from a stateless proxy.
        return None;
    }

    let id = id.unwrap_or(Value::Null);

    match method {
        "initialize" => Some(success_response(id, handle_initialize(&params))),
        "ping" => Some(success_response(id, json!({}))),
        "tools/list" => Some(success_response(id, handle_tools_list())),
        "tools/call" => match handle_tools_call(backend, &params) {
            Ok(result) => Some(success_response(id, result)),
            Err((code, msg)) => Some(error_response(id, code, &msg)),
        },
        other => Some(error_response(
            id,
            METHOD_NOT_FOUND,
            &format!("method not found: {other}"),
        )),
    }
}

fn handle_initialize(params: &Value) -> Value {
    let protocol_version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_PROTOCOL_VERSION);

    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            // The catalog is static, so we never emit list_changed notifications.
            "tools": { "listChanged": false }
        },
        "serverInfo": {
            "name": "toolkit-mcp",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": "Safe, read-only-by-default access to databases and \
            Databricks via the toolkit daemon. Credentials are never exposed; \
            write authorisation is enforced server-side.",
    })
}

fn handle_tools_list() -> Value {
    let tools: Vec<Value> = catalog::CATALOG.iter().map(|d| d.descriptor()).collect();
    json!({ "tools": tools })
}

/// Translate an MCP `tools/call` into a toolkit request and forward it.
///
/// Returns `Err((code, message))` only for protocol-level failures (unknown
/// tool, malformed params). A tool that runs but fails upstream — e.g. a denied
/// write or an unknown connection — is reported as a normal result with
/// `isError: true`, so the model sees the daemon's structured error.
fn handle_tools_call(backend: &dyn Backend, params: &Value) -> Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or((INVALID_PARAMS, "missing tool name".to_string()))?;

    let def = catalog::find(name).ok_or((INVALID_PARAMS, format!("unknown tool: {name}")))?;

    // Pull arguments; default to an empty object for paramless tools.
    let mut args: Map<String, Value> = match params.get("arguments") {
        Some(Value::Object(m)) => m.clone(),
        Some(Value::Null) | None => Map::new(),
        Some(_) => return Err((INVALID_PARAMS, "arguments must be an object".to_string())),
    };

    // `conn` is a transport concern, not a tool param — lift it out.
    let conn = match args.remove("conn") {
        Some(Value::String(s)) => Some(s),
        Some(Value::Null) | None => None,
        Some(_) => return Err((INVALID_PARAMS, "conn must be a string".to_string())),
    };

    // Remaining args become the toolkit params verbatim; the daemon performs the
    // authoritative typed validation, so we don't duplicate it here.
    let req = Request::new(def.tool, conn, def.op, Value::Object(args));

    match backend.send(&req) {
        Ok(value) => Ok(tool_result(&value, false)),
        Err(e) => Ok(tool_result(&json!({ "error": e.message() }), true)),
    }
}

/// Build an MCP tool result whose single text block is the compact JSON payload.
fn tool_result(payload: &Value, is_error: bool) -> Value {
    let text = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());
    json!({
        "content": [ { "type": "text", "text": text } ],
        "isError": is_error,
    })
}

fn success_response(id: Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

fn error_response(id: Value, code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Records the last request and returns a canned value/error.
    struct MockBackend {
        last: RefCell<Option<Request>>,
        reply: common::Result<Value>,
    }

    impl MockBackend {
        fn ok(value: Value) -> Self {
            Self {
                last: RefCell::new(None),
                reply: Ok(value),
            }
        }
        fn err(e: common::ToolkitError) -> Self {
            Self {
                last: RefCell::new(None),
                reply: Err(e),
            }
        }
    }

    impl Backend for MockBackend {
        fn send(&self, req: &Request) -> common::Result<Value> {
            *self.last.borrow_mut() = Some(Request::new(
                req.tool.clone(),
                req.conn.clone(),
                req.op.clone(),
                req.params.clone(),
            ));
            self.reply
                .as_ref()
                .map(|v| v.clone())
                .map_err(|e| common::ToolkitError::other(e.message().to_string()))
        }
    }

    fn parse(line: &str) -> Value {
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn notification_yields_no_response() {
        let backend = MockBackend::ok(json!({}));
        let out = handle_message(
            &backend,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        );
        assert!(out.is_none());
    }

    #[test]
    fn initialize_echoes_protocol_version() {
        let backend = MockBackend::ok(json!({}));
        let out = handle_message(
            &backend,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
        )
        .unwrap();
        let v = parse(&out);
        assert_eq!(v["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(v["result"]["serverInfo"]["name"], "toolkit-mcp");
        assert_eq!(v["id"], 1);
    }

    #[test]
    fn initialize_without_version_uses_default() {
        let backend = MockBackend::ok(json!({}));
        let out = handle_message(
            &backend,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        )
        .unwrap();
        assert_eq!(
            parse(&out)["result"]["protocolVersion"],
            DEFAULT_PROTOCOL_VERSION
        );
    }

    #[test]
    fn tools_list_returns_full_catalog() {
        let backend = MockBackend::ok(json!({}));
        let out = handle_message(
            &backend,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        )
        .unwrap();
        let v = parse(&out);
        let tools = v["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), catalog::CATALOG.len());
        assert!(tools.iter().any(|t| t["name"] == "psql_query"));
    }

    #[test]
    fn tools_call_translates_to_request() {
        let backend = MockBackend::ok(json!({ "rows": [], "count": 0 }));
        let out = handle_message(
            &backend,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"psql_query","arguments":{"conn":"prod","sql":"SELECT 1"}}}"#,
        )
        .unwrap();

        // The request reached the backend with conn lifted out of params.
        let req = backend.last.borrow();
        let req = req.as_ref().unwrap();
        assert_eq!(req.tool, "psql");
        assert_eq!(req.op, "query");
        assert_eq!(req.conn.as_deref(), Some("prod"));
        assert_eq!(req.params["sql"], "SELECT 1");
        assert!(req.params.get("conn").is_none());

        // The result is a non-error text block carrying the daemon payload.
        let v = parse(&out);
        assert_eq!(v["result"]["isError"], false);
        let text = v["result"]["content"][0]["text"].as_str().unwrap();
        assert_eq!(parse(text)["count"], 0);
    }

    #[test]
    fn dbr_slash_op_is_used_for_underscore_tool_name() {
        let backend = MockBackend::ok(json!({ "jobs": [] }));
        handle_message(
            &backend,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"dbr_jobs_list","arguments":{}}}"#,
        )
        .unwrap();
        let req = backend.last.borrow();
        assert_eq!(req.as_ref().unwrap().op, "jobs/list");
    }

    #[test]
    fn daemon_error_becomes_is_error_result() {
        let backend = MockBackend::err(common::ToolkitError::permission("write denied"));
        let out = handle_message(
            &backend,
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"psql_query","arguments":{"sql":"DELETE FROM x"}}}"#,
        )
        .unwrap();
        let v = parse(&out);
        // Protocol-level success, tool-level error.
        assert!(v.get("error").is_none());
        assert_eq!(v["result"]["isError"], true);
        let text = v["result"]["content"][0]["text"].as_str().unwrap();
        assert!(parse(text)["error"].as_str().unwrap().contains("denied"));
    }

    #[test]
    fn unknown_tool_is_invalid_params() {
        let backend = MockBackend::ok(json!({}));
        let out = handle_message(
            &backend,
            r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"nope","arguments":{}}}"#,
        )
        .unwrap();
        assert_eq!(parse(&out)["error"]["code"], INVALID_PARAMS);
    }

    #[test]
    fn unknown_method_is_method_not_found() {
        let backend = MockBackend::ok(json!({}));
        let out = handle_message(
            &backend,
            r#"{"jsonrpc":"2.0","id":7,"method":"resources/list"}"#,
        )
        .unwrap();
        assert_eq!(parse(&out)["error"]["code"], METHOD_NOT_FOUND);
    }

    #[test]
    fn malformed_json_is_parse_error() {
        let backend = MockBackend::ok(json!({}));
        let out = handle_message(&backend, "{not json").unwrap();
        let v = parse(&out);
        assert_eq!(v["error"]["code"], PARSE_ERROR);
        assert_eq!(v["id"], Value::Null);
    }
}
