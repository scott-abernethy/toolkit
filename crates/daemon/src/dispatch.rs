use common::protocol::{Request, Response};
use serde_json::Value;

/// Dispatch a request to the appropriate library function and return a Response.
///
/// All psql/dbr functions are synchronous; they must be called inside
/// `tokio::task::block_in_place` to avoid blocking the async runtime thread.
/// msql functions are async and can be awaited directly.
pub async fn dispatch(req: Request) -> Response {
    match req.tool.as_str() {
        "psql" => dispatch_psql(req).await,
        "msql" => dispatch_msql(req).await,
        "dbr" => dispatch_dbr(req).await,
        "guard" => dispatch_guard(req).await,
        other => Response::err(format!("unknown tool: {other}")),
    }
}

// ---------------------------------------------------------------------------
// Parameter extraction helpers
// ---------------------------------------------------------------------------

fn str_param<'a>(params: &'a Value, key: &str) -> Result<&'a str, String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing required param: {key}"))
}

fn u32_param(params: &Value, key: &str, default: u32) -> u32 {
    params
        .get(key)
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .unwrap_or(default)
}

fn i64_param(params: &Value, key: &str) -> Result<i64, String> {
    params
        .get(key)
        .and_then(|v| v.as_i64())
        .ok_or_else(|| format!("missing required param: {key}"))
}

fn bool_param(params: &Value, key: &str, default: bool) -> bool {
    params.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

fn opt_str<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params.get(key).and_then(|v| v.as_str())
}

fn to_value_result(r: common::error::Result<impl serde::Serialize>) -> Response {
    match r {
        Ok(v) => match serde_json::to_value(v) {
            Ok(jv) => Response::ok(jv),
            Err(e) => Response::err(format!("serialisation error: {e}")),
        },
        Err(e) => Response::err(e.message().to_string()),
    }
}

// ---------------------------------------------------------------------------
// psql dispatch
// ---------------------------------------------------------------------------

async fn dispatch_psql(req: Request) -> Response {
    let conn = req.conn.as_deref();
    let params = &req.params;

    let config = match tkpsql::load_config(conn) {
        Ok(c) => c,
        Err(e) => return Response::err(e.message().to_string()),
    };

    tokio::task::block_in_place(|| match req.op.as_str() {
        "query" => {
            let sql = match str_param(params, "sql") {
                Ok(s) => s,
                Err(e) => return Response::err(e),
            };
            to_value_result(tkpsql::run_query(&config, sql))
        }
        "tables" => {
            let schema = opt_str(params, "schema").unwrap_or("public");
            to_value_result(tkpsql::list_tables(&config, schema))
        }
        "describe" => {
            let table = match str_param(params, "table") {
                Ok(s) => s,
                Err(e) => return Response::err(e),
            };
            to_value_result(tkpsql::describe_table(&config, table))
        }
        other => Response::err(format!("psql: unknown op: {other}")),
    })
}

// ---------------------------------------------------------------------------
// msql dispatch
// ---------------------------------------------------------------------------

async fn dispatch_msql(req: Request) -> Response {
    let conn = req.conn.as_deref();
    let params = &req.params;

    let config = match tkmsql::load_config(conn) {
        Ok(c) => c,
        Err(e) => return Response::err(e.message().to_string()),
    };

    match req.op.as_str() {
        "query" => {
            let sql = match str_param(params, "sql") {
                Ok(s) => s.to_owned(),
                Err(e) => return Response::err(e),
            };
            to_value_result(tkmsql::run_query(&config, &sql).await)
        }
        "tables" => {
            let schema = opt_str(params, "schema").unwrap_or("dbo").to_owned();
            to_value_result(tkmsql::list_tables(&config, &schema).await)
        }
        "describe" => {
            let table = match str_param(params, "table") {
                Ok(s) => s.to_owned(),
                Err(e) => return Response::err(e),
            };
            to_value_result(tkmsql::describe_table(&config, &table).await)
        }
        other => Response::err(format!("msql: unknown op: {other}")),
    }
}

// ---------------------------------------------------------------------------
// dbr dispatch
// ---------------------------------------------------------------------------

async fn dispatch_dbr(req: Request) -> Response {
    let conn = req.conn.as_deref();
    let params = req.params.clone();

    let config = match tkdbr::load_config(conn) {
        Ok(c) => c,
        Err(e) => return Response::err(e.message().to_string()),
    };

    tokio::task::block_in_place(|| dispatch_dbr_sync(&config, &req.op, &params))
}

fn dispatch_dbr_sync(config: &tkdbr::ConnConfig, op: &str, params: &Value) -> Response {
    match op {
        "jobs/list" => {
            let limit = u32_param(params, "limit", 25);
            to_value_result(tkdbr::jobs_list(config, limit))
        }
        "jobs/get" => {
            let job_id = match i64_param(params, "job_id") {
                Ok(v) => v,
                Err(e) => return Response::err(e),
            };
            to_value_result(tkdbr::jobs_get(config, job_id))
        }
        "jobs/trigger" => {
            let job_id = match i64_param(params, "job_id") {
                Ok(v) => v,
                Err(e) => return Response::err(e),
            };
            to_value_result(tkdbr::jobs_trigger(config, job_id))
        }
        "runs/list" => {
            let job_id = match i64_param(params, "job_id") {
                Ok(v) => v,
                Err(e) => return Response::err(e),
            };
            let limit = u32_param(params, "limit", 10);
            to_value_result(tkdbr::runs_list(config, job_id, limit))
        }
        "runs/get" => {
            let run_id = match i64_param(params, "run_id") {
                Ok(v) => v,
                Err(e) => return Response::err(e),
            };
            to_value_result(tkdbr::runs_get(config, run_id))
        }
        "runs/output" => {
            let run_id = match i64_param(params, "run_id") {
                Ok(v) => v,
                Err(e) => return Response::err(e),
            };
            to_value_result(tkdbr::runs_output(config, run_id))
        }
        "clusters/list" => to_value_result(tkdbr::clusters_list(config)),
        "clusters/get" => {
            let cluster_id = match str_param(params, "cluster_id") {
                Ok(s) => s,
                Err(e) => return Response::err(e),
            };
            to_value_result(tkdbr::clusters_get(config, cluster_id))
        }
        "warehouses/list" => to_value_result(tkdbr::warehouses_list(config)),
        "warehouses/get" => {
            let warehouse_id = match str_param(params, "warehouse_id") {
                Ok(s) => s,
                Err(e) => return Response::err(e),
            };
            to_value_result(tkdbr::warehouses_get(config, warehouse_id))
        }
        "catalogs/list" => {
            let limit = u32_param(params, "limit", 100);
            to_value_result(tkdbr::catalogs_list(config, limit))
        }
        "catalogs/get" => {
            let catalog = match str_param(params, "catalog") {
                Ok(s) => s,
                Err(e) => return Response::err(e),
            };
            to_value_result(tkdbr::catalogs_get(config, catalog))
        }
        "schemas/list" => {
            let catalog = match str_param(params, "catalog") {
                Ok(s) => s,
                Err(e) => return Response::err(e),
            };
            let limit = u32_param(params, "limit", 100);
            to_value_result(tkdbr::schemas_list(config, catalog, limit))
        }
        "schemas/get" => {
            let catalog = match str_param(params, "catalog") {
                Ok(s) => s,
                Err(e) => return Response::err(e),
            };
            let schema = match str_param(params, "schema") {
                Ok(s) => s,
                Err(e) => return Response::err(e),
            };
            to_value_result(tkdbr::schemas_get(config, catalog, schema))
        }
        "tables/list" => {
            let catalog = match str_param(params, "catalog") {
                Ok(s) => s,
                Err(e) => return Response::err(e),
            };
            let schema = match str_param(params, "schema") {
                Ok(s) => s,
                Err(e) => return Response::err(e),
            };
            let limit = u32_param(params, "limit", 100);
            let omit_columns = bool_param(params, "omit_columns", false);
            to_value_result(tkdbr::tables_list(
                config,
                catalog,
                schema,
                limit,
                omit_columns,
            ))
        }
        "tables/get" => {
            let catalog = match str_param(params, "catalog") {
                Ok(s) => s,
                Err(e) => return Response::err(e),
            };
            let schema = match str_param(params, "schema") {
                Ok(s) => s,
                Err(e) => return Response::err(e),
            };
            let table = match str_param(params, "table") {
                Ok(s) => s,
                Err(e) => return Response::err(e),
            };
            to_value_result(tkdbr::tables_get(config, catalog, schema, table))
        }
        "query" => {
            let sql = match str_param(params, "sql") {
                Ok(s) => s,
                Err(e) => return Response::err(e),
            };
            let warehouse_id = opt_str(params, "warehouse_id");
            let limit = u32_param(params, "limit", 100);
            to_value_result(tkdbr::query(config, sql, warehouse_id, limit))
        }
        "bundle/validate" => to_value_result(tkdbr::bundle_validate(config)),
        "bundle/deploy" => to_value_result(tkdbr::bundle_deploy(config)),
        "bundle/run" => {
            let name = match str_param(params, "name") {
                Ok(s) => s,
                Err(e) => return Response::err(e),
            };
            let only = opt_str(params, "only");
            to_value_result(tkdbr::bundle_run(config, name, only))
        }
        "auth/store_tokens" => {
            let tokens: tkdbr::oauth::TokenPair = match serde_json::from_value(params.clone()) {
                Ok(t) => t,
                Err(e) => return Response::err(format!("invalid token params: {e}")),
            };
            to_value_result(tkdbr::store_oauth_tokens(&config.conn_name, &tokens))
        }
        other => Response::err(format!("dbr: unknown op: {other}")),
    }
}

// ---------------------------------------------------------------------------
// guard dispatch
// ---------------------------------------------------------------------------

async fn dispatch_guard(req: Request) -> Response {
    let params = &req.params;

    match req.op.as_str() {
        "config" => {
            let app = match str_param(params, "app") {
                Ok(s) => s,
                Err(e) => return Response::err(e),
            };
            let conn = req.conn.as_deref();
            tokio::task::block_in_place(|| to_value_result(common::guard::load_config(app, conn)))
        }
        "list" => tokio::task::block_in_place(list_guard_apps),
        other => Response::err(format!("guard: unknown op: {other}")),
    }
}

/// List all guard-configured apps by scanning config for sections
/// whose connections have a "command" field.
fn list_guard_apps() -> Response {
    let path = match common::config::config_path() {
        Ok(p) => p,
        Err(e) => return Response::err(e.message().to_string()),
    };
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => return Response::err(format!("failed to read config: {e}")),
    };
    let full: serde_yaml::Value = match serde_yaml::from_str(&contents) {
        Ok(v) => v,
        Err(e) => return Response::err(format!("invalid config: {e}")),
    };

    let mapping = match full.as_mapping() {
        Some(m) => m,
        None => return Response::err("config is not a YAML mapping"),
    };

    let mut apps: Vec<Value> = Vec::new();
    for (section_key, section_val) in mapping {
        let app = match section_key.as_str() {
            Some(s) => s,
            None => continue,
        };
        let conns = match section_val.as_mapping() {
            Some(m) => m,
            None => continue,
        };
        for (conn_key, conn_val) in conns {
            let conn = match conn_key.as_str() {
                Some(s) => s,
                None => continue,
            };
            if conn_val.get("command").and_then(|v| v.as_str()).is_some() {
                apps.push(serde_json::json!({"app": app, "conn": conn}));
            }
        }
    }

    // Also include install_path if present
    let install_path = full
        .get("install_path")
        .and_then(|v| v.as_str())
        .unwrap_or("$HOME/.local/bin");

    Response::ok(serde_json::json!({
        "apps": apps,
        "install_path": install_path,
    }))
}
