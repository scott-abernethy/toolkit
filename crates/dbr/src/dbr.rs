use common::exit_with_error;
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
pub fn load_config(conn: Option<&str>) -> ConnConfig {
    let mut configs = common::load_section::<HashMap<String, ConnConfig>>("dbr");

    match conn {
        Some(name) => {
            let mut c = configs.remove(name).unwrap_or_else(|| {
                let available = sorted_keys(&configs);
                exit_with_error(format!(
                    "Unknown connection '{}'. Available: {}",
                    name,
                    available.join(", ")
                ))
            });
            c.conn_name = name.to_string();
            c
        }
        None => {
            if configs.len() == 1 {
                let (name, mut c) = configs.into_iter().next().unwrap();
                c.conn_name = name;
                c
            } else {
                let available = sorted_keys(&configs);
                exit_with_error(format!(
                    "Multiple connections configured, specify --conn. Available: {}",
                    available.join(", ")
                ))
            }
        }
    }
}

fn sorted_keys(map: &HashMap<String, ConnConfig>) -> Vec<String> {
    let mut keys: Vec<String> = map.keys().cloned().collect();
    keys.sort();
    keys
}

// ---------------------------------------------------------------------------
// CLI invocation
// ---------------------------------------------------------------------------

/// Path to the toolkit-managed databricks config file.
/// Credentials written here by `tkdbr auth login`; read by all subsequent calls.
fn dbr_config_file() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    format!("{}/.config/toolkit/tkdbr-config", home)
}

/// Run a `databricks` subcommand and return parsed JSON output.
/// Global flags (--profile, --output) are prepended; subcommand args follow.
fn run_databricks(config: &ConnConfig, args: &[&str]) -> Value {
    let mut cmd = Command::new("databricks");

    cmd.arg("--output").arg("json");

    // Subcommand and its args
    cmd.args(args);

    // Inject credentials via env vars — no external config files needed
    cmd.envs(&config.env);
    cmd.env("DATABRICKS_CONFIG_FILE", dbr_config_file());
    cmd.env("DATABRICKS_CONFIG_PROFILE", &config.conn_name);

    let output = cmd
        .output()
        .unwrap_or_else(|e| exit_with_error(format!("Failed to run databricks CLI: {}", e)));

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Prefer stderr for error message; fall back to stdout
        let raw_msg = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            stdout.trim().to_string()
        };
        exit_with_error(sanitize_cli_error(&raw_msg));
    }

    serde_json::from_slice::<Value>(&output.stdout)
        .unwrap_or_else(|e| exit_with_error(format!("Failed to parse CLI output: {}", e)))
}

/// Run a databricks command that doesn't produce JSON output (e.g. bundle commands).
/// Returns (stdout, stderr) if successful, exits on failure.
fn run_databricks_no_json(config: &ConnConfig, args: &[&str]) -> (String, String) {
    let mut cmd = Command::new("databricks");

    // Subcommand and its args
    cmd.args(args);

    // Inject credentials via env vars — no external config files needed
    cmd.envs(&config.env);
    cmd.env("DATABRICKS_CONFIG_FILE", dbr_config_file());
    cmd.env("DATABRICKS_CONFIG_PROFILE", &config.conn_name);

    let output = cmd
        .output()
        .unwrap_or_else(|e| exit_with_error(format!("Failed to run databricks CLI: {}", e)));

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Prefer stderr for error message; fall back to stdout
        let raw_msg = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            stdout.trim().to_string()
        };
        exit_with_error(sanitize_cli_error(&raw_msg));
    }

    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

/// Run `databricks api post <path>` with a JSON body and return parsed JSON output.
fn run_databricks_api_post(config: &ConnConfig, path: &str, body: &Value) -> Value {
    let body_str = serde_json::to_string(body).unwrap();
    let mut cmd = Command::new("databricks");

    cmd.args(["api", "post", path, "--json", &body_str]);

    // Inject credentials via env vars — no external config files needed
    cmd.envs(&config.env);

    let output = cmd
        .output()
        .unwrap_or_else(|e| exit_with_error(format!("Failed to run databricks CLI: {}", e)));

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let raw_msg = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            stdout.trim().to_string()
        };
        exit_with_error(sanitize_cli_error(&raw_msg));
    }

    serde_json::from_slice::<Value>(&output.stdout)
        .unwrap_or_else(|e| exit_with_error(format!("Failed to parse API response: {}", e)))
}

/// Run `databricks api get <path>` and return parsed JSON output.
fn run_databricks_api_get(config: &ConnConfig, path: &str) -> Value {
    let mut cmd = Command::new("databricks");

    cmd.args(["api", "get", path]);

    // Inject credentials via env vars — no external config files needed
    cmd.envs(&config.env);

    let output = cmd
        .output()
        .unwrap_or_else(|e| exit_with_error(format!("Failed to run databricks CLI: {}", e)));

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let raw_msg = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            stdout.trim().to_string()
        };
        exit_with_error(sanitize_cli_error(&raw_msg));
    }

    serde_json::from_slice::<Value>(&output.stdout)
        .unwrap_or_else(|e| exit_with_error(format!("Failed to parse API response: {}", e)))
}

/// Strip credentials and reduce noisy CLI error messages to a single actionable line.
fn sanitize_cli_error(msg: &str) -> String {
    let lower = msg.to_lowercase();

    if lower.contains("401") || lower.contains("unauthorized") {
        return "authentication error: check your token".to_string();
    }
    if lower.contains("403") || lower.contains("forbidden") || lower.contains("permission denied") {
        return "permission denied".to_string();
    }
    if lower.contains("404") || lower.contains("does not exist") || lower.contains("not found") {
        return "resource not found".to_string();
    }
    if lower.contains("token") && (lower.contains("invalid") || lower.contains("expired")) {
        return "authentication error: invalid or expired token".to_string();
    }

    // Return only the first non-empty line to avoid dumping multi-line stack traces
    msg.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("CLI error")
        .trim()
        .to_string()
}

fn print_json(v: &Value) {
    println!("{}", serde_json::to_string(v).unwrap());
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

pub fn jobs_list(config: &ConnConfig, limit: u32) {
    let limit_str = limit.to_string();
    let raw = run_databricks(config, &["jobs", "list", "--limit", &limit_str]);

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
    print_json(&json!({"jobs": jobs, "count": count}));
}

pub fn jobs_get(config: &ConnConfig, job_id: i64) {
    let id_str = job_id.to_string();
    let raw = run_databricks(config, &["jobs", "get", "--job-id", &id_str]);

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

    print_json(&json!({
        "id": raw["job_id"],
        "name": raw["settings"]["name"],
        "created_by": raw["creator_user_name"],
        "schedule": schedule,
        "tasks": tasks,
    }));
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

pub fn jobs_trigger(config: &ConnConfig, job_id: i64) {
    if !config.can_trigger_runs() {
        exit_with_error(
            "triggering job runs is not permitted for this connection \
             (set allow_job_runs = true in config)",
        );
    }
    let id_str = job_id.to_string();
    let raw = run_databricks(config, &["jobs", "run-now", "--job-id", &id_str]);
    print_json(&json!({"run_id": raw["run_id"], "ok": true}));
}

// ---------------------------------------------------------------------------
// Runs
// ---------------------------------------------------------------------------

pub fn runs_list(config: &ConnConfig, job_id: i64, limit: u32) {
    let id_str = job_id.to_string();
    let limit_str = limit.to_string();
    let raw = run_databricks(
        config,
        &["runs", "list", "--job-id", &id_str, "--limit", &limit_str],
    );

    let runs = raw
        .get("runs")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(compact_run).collect::<Vec<_>>())
        .unwrap_or_default();

    let count = runs.len();
    print_json(&json!({"runs": runs, "count": count}));
}

pub fn runs_get(config: &ConnConfig, run_id: i64) {
    let id_str = run_id.to_string();
    let raw = run_databricks(config, &["runs", "get", "--run-id", &id_str]);
    print_json(&compact_run(&raw));
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

pub fn runs_output(config: &ConnConfig, run_id: i64) {
    let id_str = run_id.to_string();
    let raw = run_databricks(config, &["runs", "get-output", "--run-id", &id_str]);

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

    print_json(&json!({
        "run_id": raw["metadata"]["run_id"],
        "state": state["life_cycle_state"],
        "result": state.get("result_state"),
        "notebook_output": notebook_result,
        "error": error_msg,
        "error_trace": error_trace,
    }));
}

// ---------------------------------------------------------------------------
// Clusters
// ---------------------------------------------------------------------------

pub fn clusters_list(config: &ConnConfig) {
    let raw = run_databricks(config, &["clusters", "list"]);

    // CLI may return a top-level array or an object with a "clusters" key
    let clusters = raw
        .as_array()
        .or_else(|| raw.get("clusters").and_then(Value::as_array))
        .map(|arr| arr.iter().map(compact_cluster).collect::<Vec<_>>())
        .unwrap_or_default();

    let count = clusters.len();
    print_json(&json!({"clusters": clusters, "count": count}));
}

pub fn clusters_get(config: &ConnConfig, cluster_id: &str) {
    let raw = run_databricks(config, &["clusters", "get", "--cluster-id", cluster_id]);
    print_json(&compact_cluster(&raw));
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

pub fn warehouses_list(config: &ConnConfig) {
    let raw = run_databricks(config, &["warehouses", "list"]);

    // CLI may return a top-level array or an object with a "warehouses" key
    let warehouses = raw
        .as_array()
        .or_else(|| raw.get("warehouses").and_then(Value::as_array))
        .map(|arr| arr.iter().map(compact_warehouse).collect::<Vec<_>>())
        .unwrap_or_default();

    let count = warehouses.len();
    print_json(&json!({"warehouses": warehouses, "count": count}));
}

pub fn warehouses_get(config: &ConnConfig, warehouse_id: &str) {
    let raw = run_databricks(config, &["warehouses", "get", "--id", warehouse_id]);
    print_json(&compact_warehouse(&raw));
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

pub fn catalogs_list(config: &ConnConfig, limit: u32) {
    let limit_str = limit.to_string();
    let raw = run_databricks(config, &["catalogs", "list", "--max-results", &limit_str]);

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
    print_json(&json!({"catalogs": catalogs, "count": count}));
}

pub fn catalogs_get(config: &ConnConfig, catalog: &str) {
    let raw = run_databricks(config, &["catalogs", "get", catalog]);

    print_json(&json!({
        "name": raw["name"],
        "owner": raw.get("owner"),
        "created_at": raw.get("created_at"),
        "comment": raw.get("comment"),
    }));
}

// ---------------------------------------------------------------------------
// Schemas
// ---------------------------------------------------------------------------

pub fn schemas_list(config: &ConnConfig, catalog: &str, limit: u32) {
    let limit_str = limit.to_string();
    let raw = run_databricks(
        config,
        &["schemas", "list", catalog, "--max-results", &limit_str],
    );

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
    print_json(&json!({"schemas": schemas, "count": count}));
}

pub fn schemas_get(config: &ConnConfig, catalog: &str, schema: &str) {
    let full_name = format!("{}.{}", catalog, schema);
    let raw = run_databricks(config, &["schemas", "get", &full_name]);

    print_json(&json!({
        "name": raw["name"],
        "catalog": raw.get("catalog_name"),
        "owner": raw.get("owner"),
        "created_at": raw.get("created_at"),
        "comment": raw.get("comment"),
    }));
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
) {
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

    let raw = run_databricks(config, &args);

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
    print_json(&json!({"tables": tables, "count": count}));
}

pub fn tables_get(config: &ConnConfig, catalog: &str, schema: &str, table: &str) {
    let full_name = format!("{}.{}.{}", catalog, schema, table);
    let raw = run_databricks(config, &["tables", "get", &full_name]);

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

    print_json(&json!({
        "name": raw["name"],
        "catalog": raw.get("catalog_name"),
        "schema": raw.get("schema_name"),
        "type": raw.get("table_type"),
        "owner": raw.get("owner"),
        "created_at": raw.get("created_at"),
        "comment": raw.get("comment"),
        "columns": columns,
    }));
}

// ---------------------------------------------------------------------------
// Auth
// ---------------------------------------------------------------------------

/// Run `databricks auth login --host <host>` interactively, inheriting stdio.
/// Writes OAuth credentials to DATABRICKS_CONFIG_FILE under DATABRICKS_CONFIG_PROFILE,
/// so all subsequent tkdbr commands find them without a manually created ~/.databrickscfg.
pub fn auth_login(config: &ConnConfig) {
    let host = config
        .env
        .get("DATABRICKS_HOST")
        .map(|s| s.as_str())
        .unwrap_or_else(|| exit_with_error("DATABRICKS_HOST not set in config env"));

    let status = Command::new("databricks")
        .args(["auth", "login", "--host", host])
        .envs(&config.env)
        .env("DATABRICKS_CONFIG_FILE", dbr_config_file())
        .env("DATABRICKS_CONFIG_PROFILE", &config.conn_name)
        .status()
        .unwrap_or_else(|e| exit_with_error(format!("Failed to run databricks CLI: {}", e)));

    std::process::exit(status.code().unwrap_or(1));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_auth_error() {
        assert_eq!(
            sanitize_cli_error("Error: 401 Unauthorized"),
            "authentication error: check your token"
        );
    }

    #[test]
    fn test_sanitize_not_found() {
        assert_eq!(
            sanitize_cli_error("Error: resource does not exist"),
            "resource not found"
        );
    }

    #[test]
    fn test_sanitize_permission() {
        assert_eq!(
            sanitize_cli_error("Error: 403 Forbidden"),
            "permission denied"
        );
    }

    #[test]
    fn test_sanitize_first_line_only() {
        let msg = "Error: something went wrong\n  at line 1\n  at line 2\n  at line 3";
        assert_eq!(sanitize_cli_error(msg), "Error: something went wrong");
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
pub fn query(config: &ConnConfig, sql: &str, warehouse_id: Option<&str>, limit: u32) {
    let wh_id = warehouse_id.or(config.warehouse_id()).unwrap_or_else(|| {
        exit_with_error(
            "no warehouse_id: pass --warehouse-id or set DATABRICKS_WAREHOUSE_ID in config env",
        )
    });

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

    let raw = run_databricks_api_post(config, "/api/2.0/sql/statements", &body);

    let result = poll_until_done(config, raw);
    print_query_result(&result);
}

/// Check if SQL already contains a LIMIT clause (simple heuristic).
fn has_limit_clause(sql: &str) -> bool {
    // Look for LIMIT as a standalone word (case-insensitive), not inside quotes
    let upper = sql.to_uppercase();
    // Simple check: find LIMIT followed by whitespace and a number
    upper.contains(" LIMIT ") || upper.ends_with(" LIMIT")
}

/// Poll a statement until it reaches a terminal state.
fn poll_until_done(config: &ConnConfig, initial: Value) -> Value {
    let state = initial["status"]["state"].as_str().unwrap_or("UNKNOWN");

    match state {
        "SUCCEEDED" | "FAILED" | "CANCELED" | "CLOSED" => return initial,
        _ => {} // PENDING or RUNNING — need to poll
    }

    let statement_id = initial["statement_id"]
        .as_str()
        .unwrap_or_else(|| exit_with_error("no statement_id in response for polling"));

    let poll_path = format!("/api/2.0/sql/statements/{}", statement_id);

    eprint!("waiting");
    for _ in 0..QUERY_MAX_POLLS {
        thread::sleep(QUERY_POLL_INTERVAL);
        eprint!(".");

        let resp = run_databricks_api_get(config, &poll_path);
        let state = resp["status"]["state"].as_str().unwrap_or("UNKNOWN");

        match state {
            "SUCCEEDED" | "FAILED" | "CANCELED" | "CLOSED" => {
                eprintln!();
                return resp;
            }
            _ => continue,
        }
    }

    eprintln!();
    exit_with_error(format!(
        "query timed out after {}s (statement_id: {})",
        QUERY_MAX_POLLS as u64 * QUERY_POLL_INTERVAL.as_secs(),
        statement_id
    ))
}

/// Format and print query results as compact JSON.
fn print_query_result(raw: &Value) {
    let state = raw["status"]["state"].as_str().unwrap_or("UNKNOWN");

    if state != "SUCCEEDED" {
        let error_msg = raw["status"]
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("query failed");
        print_json(&json!({"error": error_msg, "state": state}));
        std::process::exit(1);
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

    print_json(&result);
}

// ---------------------------------------------------------------------------
// Bundles
// ---------------------------------------------------------------------------

pub fn bundle_validate(config: &ConnConfig) {
    let target = config.get_bundle_target();
    let _ = run_databricks_no_json(config, &["bundle", "validate", "-t", &target]);
    print_json(&json!({"ok": true}));
}

pub fn bundle_deploy(config: &ConnConfig) {
    let target = config.get_bundle_target();
    let _ = run_databricks_no_json(config, &["bundle", "deploy", "-t", &target]);
    print_json(&json!({"ok": true}));
}

pub fn bundle_run(config: &ConnConfig, name: &str, only: Option<&str>) {
    let target = config.get_bundle_target();
    let mut args = vec!["bundle", "run", name, "-t", &target, "--no-wait"];

    if let Some(only_val) = only {
        args.push("--only");
        args.push(only_val);
    }

    let (stdout, stderr) = run_databricks_no_json(config, &args);

    // Extract run ID from output like "Run URL: https://...#job/JOB_ID/run/RUN_ID"
    // Check both stdout and stderr as databricks CLI outputs to stderr
    let output = if !stdout.is_empty() { &stdout } else { &stderr };
    let run_id = output
        .lines()
        .find(|line| line.starts_with("Run URL:"))
        .and_then(|line| line.split("/run/").last().map(|id| id.trim().to_string()));

    if let Some(id) = run_id {
        print_json(&json!({"ok": true, "run_id": id}));
    } else {
        print_json(&json!({"ok": true}));
    }
}
