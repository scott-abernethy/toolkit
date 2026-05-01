use common::error::Result as ToolkitResult;
use common::protocol::{Request, Response, PROTOCOL_VERSION};
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

/// Hard ceiling for a single MS SQL operation. The Tiberius driver doesn't
/// expose a server-side `statement_timeout` equivalent, so we cap the whole
/// future at the daemon edge — a stuck query can't pin a daemon task forever.
const MSQL_OP_TIMEOUT: Duration = Duration::from_secs(60);

/// Dispatch a request to the appropriate library function and return a Response.
///
/// All psql/dbr functions are synchronous; they must be called inside
/// `tokio::task::block_in_place` to avoid blocking the async runtime thread.
/// msql functions are async and can be awaited directly.
pub async fn dispatch(req: Request) -> Response {
    if req.version > PROTOCOL_VERSION {
        return Response::err_class(
            format!(
                "unsupported wire protocol version {} (this daemon supports up to {}); upgrade the daemon",
                req.version, PROTOCOL_VERSION
            ),
            "version_unsupported",
        );
    }
    match req.tool.as_str() {
        "psql" => dispatch_psql(req).await,
        "msql" => dispatch_msql(req).await,
        "dbr" => dispatch_dbr(req).await,
        "guard" => dispatch_guard(req).await,
        other => Response::err_class(format!("unknown tool: {other}"), "unknown_tool"),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Decode a typed Op enum from `(op, params)` by injecting the op tag into
/// params and letting serde validate against the enum's `#[serde(tag = "op")]`.
fn parse_op<T: serde::de::DeserializeOwned>(
    tool: &str,
    op: &str,
    params: &Value,
) -> Result<T, String> {
    let mut payload = match params {
        Value::Object(_) => params.clone(),
        Value::Null => serde_json::json!({}),
        _ => return Err(format!("{tool}: params must be an object")),
    };
    if let Value::Object(ref mut m) = payload {
        m.insert("op".into(), Value::String(op.to_string()));
    }
    serde_json::from_value(payload).map_err(|e| format!("{tool}: {e}"))
}

fn to_value_result(r: ToolkitResult<impl serde::Serialize>) -> Response {
    match r {
        Ok(v) => match serde_json::to_value(v) {
            Ok(jv) => Response::ok(jv),
            Err(e) => Response::err_class(format!("serialisation error: {e}"), "internal"),
        },
        Err(e) => Response::err_class(e.message(), e.class()),
    }
}

fn default_psql_schema() -> String {
    "public".into()
}

fn default_msql_schema() -> String {
    "dbo".into()
}

fn default_runs_limit() -> u32 {
    10
}

fn default_jobs_limit() -> u32 {
    25
}

fn default_unity_limit() -> u32 {
    100
}

// ---------------------------------------------------------------------------
// psql dispatch
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
enum PsqlOp {
    Query {
        sql: String,
    },
    Tables {
        #[serde(default = "default_psql_schema")]
        schema: String,
    },
    Describe {
        table: String,
    },
}

async fn dispatch_psql(req: Request) -> Response {
    let op: PsqlOp = match parse_op("psql", &req.op, &req.params) {
        Ok(o) => o,
        Err(e) => return Response::err_class(e, "invalid_request"),
    };

    let config = match tkpsql::load_config(req.conn.as_deref()) {
        Ok(c) => c,
        Err(e) => return Response::err_class(e.message(), e.class()),
    };

    tokio::task::block_in_place(|| match op {
        PsqlOp::Query { sql } => to_value_result(tkpsql::run_query(&config, &sql)),
        PsqlOp::Tables { schema } => to_value_result(tkpsql::list_tables(&config, &schema)),
        PsqlOp::Describe { table } => to_value_result(tkpsql::describe_table(&config, &table)),
    })
}

// ---------------------------------------------------------------------------
// msql dispatch
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
enum MsqlOp {
    Query {
        sql: String,
    },
    Tables {
        #[serde(default = "default_msql_schema")]
        schema: String,
    },
    Describe {
        table: String,
    },
}

async fn dispatch_msql(req: Request) -> Response {
    let op: MsqlOp = match parse_op("msql", &req.op, &req.params) {
        Ok(o) => o,
        Err(e) => return Response::err_class(e, "invalid_request"),
    };

    let config = match tkmsql::load_config(req.conn.as_deref()) {
        Ok(c) => c,
        Err(e) => return Response::err_class(e.message(), e.class()),
    };

    match op {
        MsqlOp::Query { sql } => {
            with_msql_timeout(tkmsql::run_query(&config, &sql), "query").await
        }
        MsqlOp::Tables { schema } => {
            with_msql_timeout(tkmsql::list_tables(&config, &schema), "tables").await
        }
        MsqlOp::Describe { table } => {
            with_msql_timeout(tkmsql::describe_table(&config, &table), "describe").await
        }
    }
}

/// Wrap an msql future with a daemon-side timeout. On expiry the future is
/// dropped, which closes the underlying TLS/TCP socket and unblocks the caller.
async fn with_msql_timeout<T: serde::Serialize>(
    fut: impl std::future::Future<Output = ToolkitResult<T>>,
    op: &str,
) -> Response {
    match tokio::time::timeout(MSQL_OP_TIMEOUT, fut).await {
        Ok(result) => to_value_result(result),
        Err(_) => Response::err_class(
            format!("msql {op} timed out after {}s", MSQL_OP_TIMEOUT.as_secs()),
            "timeout",
        ),
    }
}

// ---------------------------------------------------------------------------
// dbr dispatch
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(tag = "op")]
enum DbrOp {
    #[serde(rename = "jobs/list")]
    JobsList {
        #[serde(default = "default_jobs_limit")]
        limit: u32,
    },
    #[serde(rename = "jobs/get")]
    JobsGet { job_id: i64 },
    #[serde(rename = "jobs/trigger")]
    JobsTrigger { job_id: i64 },
    #[serde(rename = "runs/list")]
    RunsList {
        job_id: i64,
        #[serde(default = "default_runs_limit")]
        limit: u32,
    },
    #[serde(rename = "runs/get")]
    RunsGet { run_id: i64 },
    #[serde(rename = "runs/output")]
    RunsOutput { run_id: i64 },
    #[serde(rename = "clusters/list")]
    ClustersList,
    #[serde(rename = "clusters/get")]
    ClustersGet { cluster_id: String },
    #[serde(rename = "warehouses/list")]
    WarehousesList,
    #[serde(rename = "warehouses/get")]
    WarehousesGet { warehouse_id: String },
    #[serde(rename = "catalogs/list")]
    CatalogsList {
        #[serde(default = "default_unity_limit")]
        limit: u32,
    },
    #[serde(rename = "catalogs/get")]
    CatalogsGet { catalog: String },
    #[serde(rename = "schemas/list")]
    SchemasList {
        catalog: String,
        #[serde(default = "default_unity_limit")]
        limit: u32,
    },
    #[serde(rename = "schemas/get")]
    SchemasGet { catalog: String, schema: String },
    #[serde(rename = "tables/list")]
    TablesList {
        catalog: String,
        schema: String,
        #[serde(default = "default_unity_limit")]
        limit: u32,
        #[serde(default)]
        omit_columns: bool,
    },
    #[serde(rename = "tables/get")]
    TablesGet {
        catalog: String,
        schema: String,
        table: String,
    },
    #[serde(rename = "query")]
    Query {
        sql: String,
        #[serde(default)]
        warehouse_id: Option<String>,
        #[serde(default = "default_unity_limit")]
        limit: u32,
    },
    #[serde(rename = "bundle/validate")]
    BundleValidate,
    #[serde(rename = "bundle/deploy")]
    BundleDeploy,
    #[serde(rename = "bundle/run")]
    BundleRun {
        name: String,
        #[serde(default)]
        only: Option<String>,
    },
    #[serde(rename = "auth/store_tokens")]
    AuthStoreTokens(tkdbr::oauth::TokenPair),
}

async fn dispatch_dbr(req: Request) -> Response {
    let op: DbrOp = match parse_op("dbr", &req.op, &req.params) {
        Ok(o) => o,
        Err(e) => return Response::err_class(e, "invalid_request"),
    };

    let config = match tkdbr::load_config(req.conn.as_deref()) {
        Ok(c) => c,
        Err(e) => return Response::err_class(e.message(), e.class()),
    };

    tokio::task::block_in_place(|| dispatch_dbr_sync(&config, op))
}

fn dispatch_dbr_sync(config: &tkdbr::ConnConfig, op: DbrOp) -> Response {
    match op {
        DbrOp::JobsList { limit } => to_value_result(tkdbr::jobs_list(config, limit)),
        DbrOp::JobsGet { job_id } => to_value_result(tkdbr::jobs_get(config, job_id)),
        DbrOp::JobsTrigger { job_id } => to_value_result(tkdbr::jobs_trigger(config, job_id)),
        DbrOp::RunsList { job_id, limit } => {
            to_value_result(tkdbr::runs_list(config, job_id, limit))
        }
        DbrOp::RunsGet { run_id } => to_value_result(tkdbr::runs_get(config, run_id)),
        DbrOp::RunsOutput { run_id } => to_value_result(tkdbr::runs_output(config, run_id)),
        DbrOp::ClustersList => to_value_result(tkdbr::clusters_list(config)),
        DbrOp::ClustersGet { cluster_id } => {
            to_value_result(tkdbr::clusters_get(config, &cluster_id))
        }
        DbrOp::WarehousesList => to_value_result(tkdbr::warehouses_list(config)),
        DbrOp::WarehousesGet { warehouse_id } => {
            to_value_result(tkdbr::warehouses_get(config, &warehouse_id))
        }
        DbrOp::CatalogsList { limit } => to_value_result(tkdbr::catalogs_list(config, limit)),
        DbrOp::CatalogsGet { catalog } => to_value_result(tkdbr::catalogs_get(config, &catalog)),
        DbrOp::SchemasList { catalog, limit } => {
            to_value_result(tkdbr::schemas_list(config, &catalog, limit))
        }
        DbrOp::SchemasGet { catalog, schema } => {
            to_value_result(tkdbr::schemas_get(config, &catalog, &schema))
        }
        DbrOp::TablesList {
            catalog,
            schema,
            limit,
            omit_columns,
        } => to_value_result(tkdbr::tables_list(
            config,
            &catalog,
            &schema,
            limit,
            omit_columns,
        )),
        DbrOp::TablesGet {
            catalog,
            schema,
            table,
        } => to_value_result(tkdbr::tables_get(config, &catalog, &schema, &table)),
        DbrOp::Query {
            sql,
            warehouse_id,
            limit,
        } => to_value_result(tkdbr::query(config, &sql, warehouse_id.as_deref(), limit)),
        DbrOp::BundleValidate => to_value_result(tkdbr::bundle_validate(config)),
        DbrOp::BundleDeploy => to_value_result(tkdbr::bundle_deploy(config)),
        DbrOp::BundleRun { name, only } => {
            to_value_result(tkdbr::bundle_run(config, &name, only.as_deref()))
        }
        DbrOp::AuthStoreTokens(tokens) => {
            to_value_result(tkdbr::store_oauth_tokens(&config.conn_name, &tokens))
        }
    }
}

// ---------------------------------------------------------------------------
// guard dispatch
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
enum GuardOp {
    Config { app: String },
    List,
}

async fn dispatch_guard(req: Request) -> Response {
    let op: GuardOp = match parse_op("guard", &req.op, &req.params) {
        Ok(o) => o,
        Err(e) => return Response::err_class(e, "invalid_request"),
    };

    match op {
        GuardOp::Config { app } => {
            let conn = req.conn.as_deref();
            tokio::task::block_in_place(|| to_value_result(common::guard::load_config(&app, conn)))
        }
        GuardOp::List => tokio::task::block_in_place(list_guard_apps),
    }
}

/// List all guard-configured apps by scanning config for sections
/// whose connections have a "command" field.
fn list_guard_apps() -> Response {
    let path = match common::config::config_path() {
        Ok(p) => p,
        Err(e) => return Response::err_class(e.message(), e.class()),
    };
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => return Response::err_class(format!("failed to read config: {e}"), "config"),
    };
    let full: serde_yaml::Value = match serde_yaml::from_str(&contents) {
        Ok(v) => v,
        Err(e) => return Response::err_class(format!("invalid config: {e}"), "config"),
    };

    let mapping = match full.as_mapping() {
        Some(m) => m,
        None => return Response::err_class("config is not a YAML mapping", "config"),
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

    let install_path = full
        .get("install_path")
        .and_then(|v| v.as_str())
        .unwrap_or("$HOME/.local/bin");

    Response::ok(serde_json::json!({
        "apps": apps,
        "install_path": install_path,
    }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn psql_query_op_parses() {
        let op: PsqlOp = parse_op("psql", "query", &json!({"sql": "SELECT 1"})).unwrap();
        match op {
            PsqlOp::Query { sql } => assert_eq!(sql, "SELECT 1"),
            _ => panic!("expected Query"),
        }
    }

    #[test]
    fn psql_tables_op_defaults_schema() {
        let op: PsqlOp = parse_op("psql", "tables", &json!({})).unwrap();
        match op {
            PsqlOp::Tables { schema } => assert_eq!(schema, "public"),
            _ => panic!("expected Tables"),
        }
    }

    #[test]
    fn psql_describe_op_requires_table() {
        let err = parse_op::<PsqlOp>("psql", "describe", &json!({})).unwrap_err();
        assert!(err.contains("table"), "missing table not flagged: {err}");
    }

    #[test]
    fn psql_unknown_op_rejected() {
        let err = parse_op::<PsqlOp>("psql", "drop_table", &json!({})).unwrap_err();
        assert!(err.contains("psql"), "tool prefix missing: {err}");
    }

    #[test]
    fn dbr_slash_op_parses() {
        let op: DbrOp = parse_op("dbr", "jobs/list", &json!({"limit": 5})).unwrap();
        match op {
            DbrOp::JobsList { limit } => assert_eq!(limit, 5),
            _ => panic!("expected JobsList"),
        }
    }

    #[test]
    fn dbr_unit_variant_op_parses() {
        let op: DbrOp = parse_op("dbr", "clusters/list", &json!({})).unwrap();
        assert!(matches!(op, DbrOp::ClustersList));
    }

    #[test]
    fn dbr_auth_store_tokens_flattens() {
        let op: DbrOp = parse_op(
            "dbr",
            "auth/store_tokens",
            &json!({
                "access_token": "abc",
                "refresh_token": "def",
                "expires_at": 1700000000_u64,
            }),
        )
        .unwrap();
        match op {
            DbrOp::AuthStoreTokens(tp) => {
                assert_eq!(tp.access_token, "abc");
                assert_eq!(tp.refresh_token.as_deref(), Some("def"));
            }
            _ => panic!("expected AuthStoreTokens"),
        }
    }

    #[test]
    fn guard_list_unit_variant_parses() {
        let op: GuardOp = parse_op("guard", "list", &json!({})).unwrap();
        assert!(matches!(op, GuardOp::List));
    }

    #[test]
    fn guard_config_requires_app() {
        let err = parse_op::<GuardOp>("guard", "config", &json!({})).unwrap_err();
        assert!(err.contains("app"));
    }

    #[test]
    fn null_params_treated_as_empty_object() {
        let op: PsqlOp = parse_op("psql", "tables", &json!(null)).unwrap();
        match op {
            PsqlOp::Tables { schema } => assert_eq!(schema, "public"),
            _ => panic!("expected Tables"),
        }
    }

    #[test]
    fn non_object_params_rejected() {
        let err = parse_op::<PsqlOp>("psql", "tables", &json!("oops")).unwrap_err();
        assert!(err.contains("must be an object"));
    }
}
