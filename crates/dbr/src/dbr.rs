use common::{Result, ToolkitError};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Command;
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ConnConfig {
    /// Environment variables to inject when running the Databricks CLI.
    /// Expected keys: DATABRICKS_HOST, DATABRICKS_TOKEN (or other auth vars),
    /// and optionally DATABRICKS_WAREHOUSE_ID.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Allow triggering job runs via `jobs trigger` (default: false)
    pub allow_job_runs: Option<bool>,
    /// Bundle target for bundle commands (e.g. "local", "dev", "prod")
    pub bundle_target: Option<String>,
    /// Connection name (not from config file — set by load_config)
    #[serde(skip)]
    pub conn_name: String,
}

impl ConnConfig {
    fn can_trigger_runs(&self) -> bool {
        self.allow_job_runs.unwrap_or(false)
    }

    fn get_bundle_target(&self) -> String {
        self.bundle_target
            .clone()
            .unwrap_or_else(|| "local".to_string())
    }

    pub fn warehouse_id(&self) -> Option<&str> {
        self.env.get("DATABRICKS_WAREHOUSE_ID").map(|s| s.as_str())
    }
}

/// Load a named connection from the [dbr] section of the shared config.
/// If `conn` is None and exactly one connection is configured, that one is used.
pub fn load_config(conn: Option<&str>) -> Result<ConnConfig> {
    let (name, mut c) = common::load_named_section_with_name::<ConnConfig>("dbr", conn)?;
    c.conn_name = name;
    Ok(c)
}

// ---------------------------------------------------------------------------
// CLI invocation
// ---------------------------------------------------------------------------

/// Get the effective Databricks access token for this connection.
///
/// Priority:
/// 1. PAT from config.env DATABRICKS_TOKEN (non-empty) — used as-is, no refresh
/// 2. OAuth token file ($HOME/.config/toolkit/dbr-oauth/<conn>.json)
///    - if near expiry and refresh_token present: refresh via /oidc/v1/token
///    - if expired with no refresh or failed refresh: return Err
/// 3. None — no token found (caller proceeds and may get an auth error from CLI)
pub fn get_effective_token(config: &ConnConfig) -> Result<Option<String>> {
    // 1. PAT in config
    if let Some(t) = config.env.get("DATABRICKS_TOKEN") {
        if !t.is_empty() {
            return Ok(Some(t.clone()));
        }
    }

    // 2. OAuth token file
    let path = match crate::oauth::token_file_path(&config.conn_name) {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    if !path.exists() {
        return Ok(None);
    }
    let mut tokens = match crate::oauth::read_token_file(&path) {
        Ok(t) => t,
        Err(_) => return Ok(None), // corrupted file — don't block
    };

    if crate::oauth::is_near_expiry(tokens.expires_at) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if let Some(refresh_token) = tokens.refresh_token.clone() {
            let host = config
                .env
                .get("DATABRICKS_HOST")
                .ok_or_else(|| ToolkitError::config("DATABRICKS_HOST not set"))?;
            match crate::oauth::refresh_tokens(host, &refresh_token) {
                Ok(new_tokens) => {
                    let _ = crate::oauth::write_token_file(&path, &new_tokens);
                    tokens = new_tokens;
                }
                Err(e) => {
                    if tokens.expires_at > now {
                        // Near expiry but still valid — use it
                    } else {
                        return Err(ToolkitError::auth(format!(
                            "Databricks token expired and refresh failed: {}. Run: toolkit dbr login --conn {}",
                            e, config.conn_name
                        )));
                    }
                }
            }
        } else if tokens.expires_at <= now {
            return Err(ToolkitError::auth(format!(
                "Databricks token expired. Run: toolkit dbr login --conn {}",
                config.conn_name
            )));
        }
    }

    Ok(Some(tokens.access_token))
}

/// Run a `databricks` subcommand and return parsed JSON output.
/// Global flags (--output) are prepended; subcommand args follow.
fn run_databricks(config: &ConnConfig, args: &[&str]) -> Result<Value> {
    let mut cmd = Command::new("databricks");

    cmd.arg("--output").arg("json");

    // Subcommand and its args
    cmd.args(args);

    // Inject credentials via env vars; prevent CLI from reading any default config
    cmd.envs(&config.env);
    cmd.env("DATABRICKS_CONFIG_FILE", "/dev/null");
    if let Some(token) = get_effective_token(config)? {
        cmd.env("DATABRICKS_TOKEN", token);
    }

    let output = cmd
        .output()
        .map_err(|e| ToolkitError::cli(format!("Failed to run databricks CLI: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Prefer stderr for error message; fall back to stdout
        let raw_msg = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            stdout.trim().to_string()
        };
        return Err(sanitize_cli_error(&raw_msg));
    }

    serde_json::from_slice::<Value>(&output.stdout)
        .map_err(|e| ToolkitError::cli(format!("Failed to parse CLI output: {}", e)))
}

/// Run a databricks command that doesn't produce JSON output (e.g. bundle commands).
/// Returns (stdout, stderr) if successful.
fn run_databricks_no_json(config: &ConnConfig, args: &[&str]) -> Result<(String, String)> {
    let mut cmd = Command::new("databricks");

    // Subcommand and its args
    cmd.args(args);

    // Inject credentials via env vars; prevent CLI from reading any default config
    cmd.envs(&config.env);
    cmd.env("DATABRICKS_CONFIG_FILE", "/dev/null");
    if let Some(token) = get_effective_token(config)? {
        cmd.env("DATABRICKS_TOKEN", token);
    }

    let output = cmd
        .output()
        .map_err(|e| ToolkitError::cli(format!("Failed to run databricks CLI: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Prefer stderr for error message; fall back to stdout
        let raw_msg = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            stdout.trim().to_string()
        };
        return Err(sanitize_cli_error(&raw_msg));
    }

    Ok((
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    ))
}

/// Run `databricks api post <path>` with a JSON body and return parsed JSON output.
fn run_databricks_api_post(config: &ConnConfig, path: &str, body: &Value) -> Result<Value> {
    let body_str = serde_json::to_string(body).unwrap();
    let mut cmd = Command::new("databricks");

    cmd.args(["api", "post", path, "--json", &body_str]);

    // Inject credentials via env vars; prevent CLI from reading any default config
    cmd.envs(&config.env);
    cmd.env("DATABRICKS_CONFIG_FILE", "/dev/null");
    if let Some(token) = get_effective_token(config)? {
        cmd.env("DATABRICKS_TOKEN", token);
    }

    let output = cmd
        .output()
        .map_err(|e| ToolkitError::cli(format!("Failed to run databricks CLI: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let raw_msg = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            stdout.trim().to_string()
        };
        return Err(sanitize_cli_error(&raw_msg));
    }

    serde_json::from_slice::<Value>(&output.stdout)
        .map_err(|e| ToolkitError::cli(format!("Failed to parse API response: {}", e)))
}

/// Run `databricks api get <path>` and return parsed JSON output.
fn run_databricks_api_get(config: &ConnConfig, path: &str) -> Result<Value> {
    let mut cmd = Command::new("databricks");

    cmd.args(["api", "get", path]);

    // Inject credentials via env vars; prevent CLI from reading any default config
    cmd.envs(&config.env);
    cmd.env("DATABRICKS_CONFIG_FILE", "/dev/null");
    if let Some(token) = get_effective_token(config)? {
        cmd.env("DATABRICKS_TOKEN", token);
    }

    let output = cmd
        .output()
        .map_err(|e| ToolkitError::cli(format!("Failed to run databricks CLI: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let raw_msg = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            stdout.trim().to_string()
        };
        return Err(sanitize_cli_error(&raw_msg));
    }

    serde_json::from_slice::<Value>(&output.stdout)
        .map_err(|e| ToolkitError::cli(format!("Failed to parse API response: {}", e)))
}

/// Strip credentials and reduce noisy CLI error messages to a single actionable line.
fn sanitize_cli_error(msg: &str) -> ToolkitError {
    let lower = msg.to_lowercase();

    if lower.contains("401") || lower.contains("unauthorized") {
        return ToolkitError::auth("authentication error: check your token");
    }
    if lower.contains("403") || lower.contains("forbidden") || lower.contains("permission denied") {
        return ToolkitError::permission("permission denied");
    }
    if lower.contains("404") || lower.contains("does not exist") || lower.contains("not found") {
        return ToolkitError::not_found("resource not found");
    }
    if lower.contains("token") && (lower.contains("invalid") || lower.contains("expired")) {
        return ToolkitError::auth("authentication error: invalid or expired token");
    }

    // Return only the first non-empty line to avoid dumping multi-line stack traces
    let first_line = msg
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("CLI error")
        .trim()
        .to_string();
    ToolkitError::cli(first_line)
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

pub fn jobs_list(config: &ConnConfig, limit: u32) -> Result<Value> {
    let limit_str = limit.to_string();
    let raw = run_databricks(config, &["jobs", "list", "--limit", &limit_str])?;

    let jobs = raw
        .get("jobs")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|j| {
                    json!({
                        "id": j["job_id"],
                        "name": j["settings"]["name"],
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let count = jobs.len();
    Ok(json!({"jobs": jobs, "count": count}))
}

pub fn jobs_get(config: &ConnConfig, job_id: i64) -> Result<Value> {
    let id_str = job_id.to_string();
    let raw = run_databricks(config, &["jobs", "get", "--job-id", &id_str])?;

    let tasks = raw["settings"]["tasks"].as_array().map(|tasks| {
        tasks
            .iter()
            .map(|t| {
                json!({
                    "key": t["task_key"],
                    "type": task_type(t),
                })
            })
            .collect::<Vec<_>>()
    });

    let schedule = raw["settings"]
        .get("schedule")
        .and_then(|s| s.get("quartz_cron_expression"))
        .cloned();

    Ok(json!({
        "id": raw["job_id"],
        "name": raw["settings"]["name"],
        "created_by": raw["creator_user_name"],
        "schedule": schedule,
        "tasks": tasks,
    }))
}

fn task_type(task: &Value) -> &str {
    if task.get("notebook_task").is_some() {
        "notebook"
    } else if task.get("spark_jar_task").is_some() {
        "spark_jar"
    } else if task.get("spark_python_task").is_some() {
        "spark_python"
    } else if task.get("python_wheel_task").is_some() {
        "python_wheel"
    } else if task.get("pipeline_task").is_some() {
        "pipeline"
    } else if task.get("sql_task").is_some() {
        "sql"
    } else if task.get("dbt_task").is_some() {
        "dbt"
    } else {
        "unknown"
    }
}

pub fn jobs_trigger(config: &ConnConfig, job_id: i64) -> Result<Value> {
    if !config.can_trigger_runs() {
        return Err(ToolkitError::permission(
            "triggering job runs is not permitted for this connection \
             (set allow_job_runs = true in config)",
        ));
    }
    let id_str = job_id.to_string();
    let raw = run_databricks(config, &["jobs", "run-now", "--job-id", &id_str])?;
    Ok(json!({"run_id": raw["run_id"], "ok": true}))
}

// ---------------------------------------------------------------------------
// Runs
// ---------------------------------------------------------------------------

pub fn runs_list(config: &ConnConfig, job_id: i64, limit: u32) -> Result<Value> {
    let id_str = job_id.to_string();
    let limit_str = limit.to_string();
    let raw = run_databricks(
        config,
        &["runs", "list", "--job-id", &id_str, "--limit", &limit_str],
    )?;

    let runs = raw
        .get("runs")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(compact_run).collect::<Vec<_>>())
        .unwrap_or_default();

    let count = runs.len();
    Ok(json!({"runs": runs, "count": count}))
}

pub fn runs_get(config: &ConnConfig, run_id: i64) -> Result<Value> {
    let id_str = run_id.to_string();
    let raw = run_databricks(config, &["runs", "get", "--run-id", &id_str])?;
    Ok(compact_run(&raw))
}

/// Compact run representation: drop all scheduling/cluster/task detail, keep status + timing.
fn compact_run(r: &Value) -> Value {
    let state = r.get("state").unwrap_or(&Value::Null);

    let message = state
        .get("state_message")
        .and_then(|m| m.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    json!({
        "run_id": r["run_id"],
        "job_id": r["job_id"],
        "state": state["life_cycle_state"],
        "result": state.get("result_state"),
        "message": message,
        "start_ms": r.get("start_time"),
        "end_ms": r.get("end_time"),
    })
}

pub fn runs_output(config: &ConnConfig, run_id: i64) -> Result<Value> {
    let id_str = run_id.to_string();
    let raw = run_databricks(config, &["runs", "get-output", "--run-id", &id_str])?;

    let state = raw["metadata"].get("state").unwrap_or(&Value::Null);

    let error_trace = raw
        .get("error_trace")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| {
            // Truncate long stack traces to avoid flooding agent context
            let truncated: String = s.chars().take(500).collect();
            if truncated.chars().count() < s.chars().count() {
                format!("{}…", truncated)
            } else {
                truncated
            }
        });

    let notebook_result = raw
        .get("notebook_output")
        .and_then(|n| n.get("result"))
        .cloned();

    let error_msg = raw
        .get("error")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    Ok(json!({
        "run_id": raw["metadata"]["run_id"],
        "state": state["life_cycle_state"],
        "result": state.get("result_state"),
        "notebook_output": notebook_result,
        "error": error_msg,
        "error_trace": error_trace,
    }))
}

// ---------------------------------------------------------------------------
// Clusters
// ---------------------------------------------------------------------------

pub fn clusters_list(config: &ConnConfig) -> Result<Value> {
    let raw = run_databricks(config, &["clusters", "list"])?;

    // CLI may return a top-level array or an object with a "clusters" key
    let clusters = raw
        .as_array()
        .or_else(|| raw.get("clusters").and_then(Value::as_array))
        .map(|arr| arr.iter().map(compact_cluster).collect::<Vec<_>>())
        .unwrap_or_default();

    let count = clusters.len();
    Ok(json!({"clusters": clusters, "count": count}))
}

pub fn clusters_get(config: &ConnConfig, cluster_id: &str) -> Result<Value> {
    let raw = run_databricks(config, &["clusters", "get", "--cluster-id", cluster_id])?;
    Ok(compact_cluster(&raw))
}

fn compact_cluster(c: &Value) -> Value {
    let autoscale = c.get("autoscale").map(|a| {
        json!({
            "min": a["min_workers"],
            "max": a["max_workers"],
        })
    });

    json!({
        "id": c["cluster_id"],
        "name": c["cluster_name"],
        "state": c["state"],
        "spark_version": c.get("spark_version"),
        "node_type": c.get("node_type_id"),
        "num_workers": c.get("num_workers"),
        "autoscale": autoscale,
    })
}

// ---------------------------------------------------------------------------
// Warehouses
// ---------------------------------------------------------------------------

pub fn warehouses_list(config: &ConnConfig) -> Result<Value> {
    let raw = run_databricks(config, &["warehouses", "list"])?;

    // CLI may return a top-level array or an object with a "warehouses" key
    let warehouses = raw
        .as_array()
        .or_else(|| raw.get("warehouses").and_then(Value::as_array))
        .map(|arr| arr.iter().map(compact_warehouse).collect::<Vec<_>>())
        .unwrap_or_default();

    let count = warehouses.len();
    Ok(json!({"warehouses": warehouses, "count": count}))
}

pub fn warehouses_get(config: &ConnConfig, warehouse_id: &str) -> Result<Value> {
    let raw = run_databricks(config, &["warehouses", "get", "--id", warehouse_id])?;
    Ok(compact_warehouse(&raw))
}

fn compact_warehouse(w: &Value) -> Value {
    json!({
        "id": w["id"],
        "name": w["name"],
        "state": w["state"],
        "size": w.get("cluster_size"),
        "type": w.get("warehouse_type"),
    })
}

// ---------------------------------------------------------------------------
// Catalogs
// ---------------------------------------------------------------------------

pub fn catalogs_list(config: &ConnConfig, limit: u32) -> Result<Value> {
    let limit_str = limit.to_string();
    let raw = run_databricks(config, &["catalogs", "list", "--max-results", &limit_str])?;

    // API returns a top-level array, not wrapped in an object
    let catalogs = raw
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|c| {
                    json!({
                        "name": c["name"],
                        "owner": c.get("owner"),
                        "created_at": c.get("created_at"),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let count = catalogs.len();
    Ok(json!({"catalogs": catalogs, "count": count}))
}

pub fn catalogs_get(config: &ConnConfig, catalog: &str) -> Result<Value> {
    let raw = run_databricks(config, &["catalogs", "get", catalog])?;

    Ok(json!({
        "name": raw["name"],
        "owner": raw.get("owner"),
        "created_at": raw.get("created_at"),
        "comment": raw.get("comment"),
    }))
}

// ---------------------------------------------------------------------------
// Schemas
// ---------------------------------------------------------------------------

pub fn schemas_list(config: &ConnConfig, catalog: &str, limit: u32) -> Result<Value> {
    let limit_str = limit.to_string();
    let raw = run_databricks(
        config,
        &["schemas", "list", catalog, "--max-results", &limit_str],
    )?;

    // API returns a top-level array, not wrapped in an object
    let schemas = raw
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|s| {
                    json!({
                        "name": s["name"],
                        "catalog": s.get("catalog_name"),
                        "owner": s.get("owner"),
                        "created_at": s.get("created_at"),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let count = schemas.len();
    Ok(json!({"schemas": schemas, "count": count}))
}

pub fn schemas_get(config: &ConnConfig, catalog: &str, schema: &str) -> Result<Value> {
    let full_name = format!("{}.{}", catalog, schema);
    let raw = run_databricks(config, &["schemas", "get", &full_name])?;

    Ok(json!({
        "name": raw["name"],
        "catalog": raw.get("catalog_name"),
        "owner": raw.get("owner"),
        "created_at": raw.get("created_at"),
        "comment": raw.get("comment"),
    }))
}

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

pub fn tables_list(
    config: &ConnConfig,
    catalog: &str,
    schema: &str,
    limit: u32,
    omit_columns: bool,
) -> Result<Value> {
    let limit_str = limit.to_string();
    let mut args = vec![
        "tables",
        "list",
        catalog,
        schema,
        "--max-results",
        &limit_str,
    ];
    if omit_columns {
        args.push("--omit-columns");
    }

    let raw = run_databricks(config, &args)?;

    // API returns a top-level array, not wrapped in an object
    let tables = raw
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|t| {
                    let mut table_obj = json!({
                        "name": t["name"],
                        "type": t.get("table_type"),
                    });

                    if !omit_columns {
                        if let Some(cols) = t.get("columns").and_then(Value::as_array) {
                            table_obj["columns"] = json!(cols
                                .iter()
                                .map(|c| {
                                    json!({
                                        "name": c["name"],
                                        "type": c["type_text"],
                                        "nullable": c.get("nullable").unwrap_or(&json!(true)),
                                        "comment": c.get("comment"),
                                    })
                                })
                                .collect::<Vec<_>>());
                        }
                    }

                    table_obj
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let count = tables.len();
    Ok(json!({"tables": tables, "count": count}))
}

pub fn tables_get(config: &ConnConfig, catalog: &str, schema: &str, table: &str) -> Result<Value> {
    let full_name = format!("{}.{}.{}", catalog, schema, table);
    let raw = run_databricks(config, &["tables", "get", &full_name])?;

    let columns = raw.get("columns").and_then(Value::as_array).map(|cols| {
        cols.iter()
            .map(|c| {
                json!({
                    "name": c["name"],
                    "type": c["type_text"],
                    "nullable": c.get("nullable").unwrap_or(&json!(true)),
                    "comment": c.get("comment"),
                })
            })
            .collect::<Vec<_>>()
    });

    Ok(json!({
        "name": raw["name"],
        "catalog": raw.get("catalog_name"),
        "schema": raw.get("schema_name"),
        "type": raw.get("table_type"),
        "owner": raw.get("owner"),
        "created_at": raw.get("created_at"),
        "comment": raw.get("comment"),
        "columns": columns,
    }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_auth_error() {
        match sanitize_cli_error("Error: 401 Unauthorized") {
            ToolkitError::Auth(m) => assert_eq!(m, "authentication error: check your token"),
            other => panic!("expected Auth, got {:?}", other),
        }
    }

    #[test]
    fn test_sanitize_not_found() {
        match sanitize_cli_error("Error: resource does not exist") {
            ToolkitError::NotFound(m) => assert_eq!(m, "resource not found"),
            other => panic!("expected NotFound, got {:?}", other),
        }
    }

    #[test]
    fn test_sanitize_permission() {
        match sanitize_cli_error("Error: 403 Forbidden") {
            ToolkitError::Permission(m) => assert_eq!(m, "permission denied"),
            other => panic!("expected Permission, got {:?}", other),
        }
    }

    #[test]
    fn test_sanitize_first_line_only() {
        let msg = "Error: something went wrong\n  at line 1\n  at line 2\n  at line 3";
        match sanitize_cli_error(msg) {
            ToolkitError::Cli(m) => assert_eq!(m, "Error: something went wrong"),
            other => panic!("expected Cli, got {:?}", other),
        }
    }

    #[test]
    fn test_task_type_notebook() {
        let task = json!({"task_key": "t1", "notebook_task": {"notebook_path": "/foo"}});
        assert_eq!(task_type(&task), "notebook");
    }

    #[test]
    fn test_task_type_unknown() {
        let task = json!({"task_key": "t1"});
        assert_eq!(task_type(&task), "unknown");
    }

    #[test]
    fn test_compact_run_terminated() {
        let run = json!({
            "run_id": 123,
            "job_id": 456,
            "state": {
                "life_cycle_state": "TERMINATED",
                "result_state": "SUCCESS",
                "state_message": ""
            },
            "start_time": 1700000000000_i64,
            "end_time": 1700000060000_i64,
        });
        let compact = compact_run(&run);
        assert_eq!(compact["run_id"], 123);
        assert_eq!(compact["state"], "TERMINATED");
        assert_eq!(compact["result"], "SUCCESS");
        assert!(compact["message"].is_null()); // empty message filtered
    }

    #[test]
    fn test_compact_run_running_no_result() {
        let run = json!({
            "run_id": 99,
            "job_id": 1,
            "state": {
                "life_cycle_state": "RUNNING",
                "state_message": "In progress"
            },
        });
        let compact = compact_run(&run);
        assert_eq!(compact["state"], "RUNNING");
        assert_eq!(compact["message"], "In progress");
    }

    #[test]
    fn test_compact_cluster() {
        let c = json!({
            "cluster_id": "abc-123",
            "cluster_name": "My Cluster",
            "state": "RUNNING",
            "spark_version": "14.3.x-scala2.12",
            "node_type_id": "i3.xlarge",
            "num_workers": 4,
            "driver": {"node_id": "xxx"},       // should be dropped
            "aws_attributes": {"availability": "ON_DEMAND"}, // should be dropped
        });
        let compact = compact_cluster(&c);
        assert_eq!(compact["id"], "abc-123");
        assert_eq!(compact["state"], "RUNNING");
        assert!(compact.get("driver").is_none());
        assert!(compact.get("aws_attributes").is_none());
    }

    #[test]
    fn test_has_limit_clause() {
        assert!(has_limit_clause("SELECT * FROM t LIMIT 10"));
        assert!(has_limit_clause("select * from t limit 10"));
        assert!(has_limit_clause("SELECT * FROM t WHERE x > 1 LIMIT 50"));
        assert!(!has_limit_clause("SELECT * FROM t"));
        assert!(!has_limit_clause("SELECT limited FROM t"));
    }

    #[test]
    fn test_print_query_result_success() {
        let raw = json!({
            "status": {"state": "SUCCEEDED"},
            "manifest": {
                "schema": {
                    "columns": [
                        {"name": "id", "type_text": "INT"},
                        {"name": "name", "type_text": "STRING"},
                    ]
                },
                "total_row_count": 2,
            },
            "result": {
                "data_array": [
                    ["1", "alice"],
                    ["2", "bob"],
                ],
            },
        });

        // Extract columns like print_query_result does
        let columns: Vec<&str> = raw["manifest"]["schema"]["columns"]
            .as_array()
            .map(|cols| cols.iter().filter_map(|c| c["name"].as_str()).collect())
            .unwrap_or_default();

        assert_eq!(columns, vec!["id", "name"]);

        let rows = raw["result"]["data_array"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], "1");
        assert_eq!(rows[0][1], "alice");
    }
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

/// Maximum number of times to poll for a pending/running statement before giving up.
const QUERY_MAX_POLLS: u32 = 60;

/// Delay between poll attempts.
const QUERY_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Execute a SQL query via the Statement Execution API.
/// Uses `databricks api post /api/2.0/sql/statements` to submit, then polls if needed.
pub fn query(
    config: &ConnConfig,
    sql: &str,
    warehouse_id: Option<&str>,
    limit: u32,
) -> Result<Value> {
    let wh_id = warehouse_id.or(config.warehouse_id()).ok_or_else(|| {
        ToolkitError::config(
            "no warehouse_id: pass --warehouse-id or set DATABRICKS_WAREHOUSE_ID in config env",
        )
    })?;

    // Apply LIMIT to the SQL if the user hasn't already included one
    let statement = if limit > 0 && !has_limit_clause(sql) {
        format!("{} LIMIT {}", sql.trim().trim_end_matches(';'), limit)
    } else {
        sql.trim().trim_end_matches(';').to_string()
    };

    let body = json!({
        "warehouse_id": wh_id,
        "statement": statement,
        "wait_timeout": "50s",
        "disposition": "INLINE",
        "format": "JSON_ARRAY",
    });

    let raw = run_databricks_api_post(config, "/api/2.0/sql/statements", &body)?;

    let result = poll_until_done(config, raw)?;
    build_query_result(&result)
}

/// Check if SQL already contains a LIMIT clause (simple heuristic).
fn has_limit_clause(sql: &str) -> bool {
    // Look for LIMIT as a standalone word (case-insensitive), not inside quotes
    let upper = sql.to_uppercase();
    // Simple check: find LIMIT followed by whitespace and a number
    upper.contains(" LIMIT ") || upper.ends_with(" LIMIT")
}

/// Poll a statement until it reaches a terminal state.
fn poll_until_done(config: &ConnConfig, initial: Value) -> Result<Value> {
    let state = initial["status"]["state"].as_str().unwrap_or("UNKNOWN");

    match state {
        "SUCCEEDED" | "FAILED" | "CANCELED" | "CLOSED" => return Ok(initial),
        _ => {} // PENDING or RUNNING — need to poll
    }

    let statement_id = initial["statement_id"]
        .as_str()
        .ok_or_else(|| ToolkitError::other("no statement_id in response for polling"))?;

    let poll_path = format!("/api/2.0/sql/statements/{}", statement_id);

    for _ in 0..QUERY_MAX_POLLS {
        thread::sleep(QUERY_POLL_INTERVAL);

        let resp = run_databricks_api_get(config, &poll_path)?;
        let state = resp["status"]["state"].as_str().unwrap_or("UNKNOWN");

        match state {
            "SUCCEEDED" | "FAILED" | "CANCELED" | "CLOSED" => {
                return Ok(resp);
            }
            _ => continue,
        }
    }

    Err(ToolkitError::other(format!(
        "query timed out after {}s (statement_id: {})",
        QUERY_MAX_POLLS as u64 * QUERY_POLL_INTERVAL.as_secs(),
        statement_id
    )))
}

/// Build compact query results as a JSON value.
fn build_query_result(raw: &Value) -> Result<Value> {
    let state = raw["status"]["state"].as_str().unwrap_or("UNKNOWN");

    if state != "SUCCEEDED" {
        let error_msg = raw["status"]
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("query failed");
        return Err(ToolkitError::other(format!(
            "{} (state: {})",
            error_msg, state
        )));
    }

    // Extract column names from manifest
    let columns: Vec<&str> = raw["manifest"]["schema"]["columns"]
        .as_array()
        .map(|cols| cols.iter().filter_map(|c| c["name"].as_str()).collect())
        .unwrap_or_default();

    // Extract row data
    let rows = raw["result"]["data_array"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let row_count = raw["manifest"]
        .get("total_row_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(rows.len() as u64);

    // If truncated, note it
    let truncated = raw["result"]
        .get("truncated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut result = json!({
        "columns": columns,
        "rows": rows,
        "count": row_count,
    });

    if truncated {
        result["truncated"] = json!(true);
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Bundles
// ---------------------------------------------------------------------------

pub fn bundle_validate(config: &ConnConfig) -> Result<Value> {
    let target = config.get_bundle_target();
    run_databricks_no_json(config, &["bundle", "validate", "-t", &target])?;
    Ok(json!({"ok": true}))
}

pub fn bundle_deploy(config: &ConnConfig) -> Result<Value> {
    let target = config.get_bundle_target();
    run_databricks_no_json(config, &["bundle", "deploy", "-t", &target])?;
    Ok(json!({"ok": true}))
}

pub fn bundle_run(config: &ConnConfig, name: &str, only: Option<&str>) -> Result<Value> {
    let target = config.get_bundle_target();
    let mut args = vec!["bundle", "run", name, "-t", &target, "--no-wait"];

    if let Some(only_val) = only {
        args.push("--only");
        args.push(only_val);
    }

    let (stdout, stderr) = run_databricks_no_json(config, &args)?;

    // Extract run ID from output like "Run URL: https://...#job/JOB_ID/run/RUN_ID"
    // Check both stdout and stderr as databricks CLI outputs to stderr
    let output = if !stdout.is_empty() { &stdout } else { &stderr };
    let run_id = output
        .lines()
        .find(|line| line.starts_with("Run URL:"))
        .and_then(|line| line.split("/run/").last().map(|id| id.trim().to_string()));

    if let Some(id) = run_id {
        Ok(json!({"ok": true, "run_id": id}))
    } else {
        Ok(json!({"ok": true}))
    }
}
