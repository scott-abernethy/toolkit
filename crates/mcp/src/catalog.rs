//! MCP tool catalog.
//!
//! A static table mapping each MCP tool name to a toolkit `(tool, op)` pair and
//! its parameters. This is the single source of truth for the MCP surface and
//! deliberately mirrors the CLI tools (`tkpsql`, `tkmsql`, `tkdbr`) one-for-one:
//! enforcement lives in the daemon, so the MCP frontend exposes exactly what the
//! CLIs expose — no more, no less.
//!
//! Internal/transport ops are intentionally omitted: dbr `auth/store_tokens` and
//! `auth/get_host` belong to the user-run OAuth flow, and the `guard` ops back
//! the wrapper-script machinery rather than agent-facing operations.

use serde_json::{json, Map, Value};

/// JSON Schema scalar type for a parameter.
#[derive(Clone, Copy)]
pub enum Ty {
    String,
    Integer,
    Boolean,
}

impl Ty {
    fn as_str(self) -> &'static str {
        match self {
            Ty::String => "string",
            Ty::Integer => "integer",
            Ty::Boolean => "boolean",
        }
    }
}

/// A single tool parameter, forwarded verbatim into the toolkit request params.
pub struct Param {
    pub name: &'static str,
    pub ty: Ty,
    pub required: bool,
    pub description: &'static str,
}

/// One MCP tool, bound to a toolkit `(tool, op)`.
pub struct ToolDef {
    /// MCP tool name (must match `^[a-zA-Z0-9_-]+$`).
    pub name: &'static str,
    /// toolkit tool segment of the wire request (e.g. "psql", "dbr").
    pub tool: &'static str,
    /// toolkit op segment of the wire request (e.g. "query", "jobs/list").
    pub op: &'static str,
    pub description: &'static str,
    pub params: &'static [Param],
}

impl ToolDef {
    /// Build the MCP `inputSchema` (a JSON Schema object) for this tool.
    ///
    /// Every tool gains an optional `conn` argument; the daemon selects the sole
    /// connection automatically when `conn` is omitted and only one is configured.
    pub fn input_schema(&self) -> Value {
        let mut props = Map::new();
        props.insert(
            "conn".to_string(),
            json!({
                "type": "string",
                "description": "Named connection from config; omit if only one is configured.",
            }),
        );

        let mut required: Vec<&str> = Vec::new();
        for p in self.params {
            props.insert(
                p.name.to_string(),
                json!({ "type": p.ty.as_str(), "description": p.description }),
            );
            if p.required {
                required.push(p.name);
            }
        }

        json!({
            "type": "object",
            "properties": Value::Object(props),
            "required": required,
            "additionalProperties": false,
        })
    }

    /// The MCP `tools/list` entry for this tool.
    pub fn descriptor(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": self.input_schema(),
        })
    }
}

/// Look up a tool by its MCP name.
pub fn find(name: &str) -> Option<&'static ToolDef> {
    CATALOG.iter().find(|d| d.name == name)
}

/// The full, static MCP tool catalog.
pub static CATALOG: &[ToolDef] = &[
    // ---- psql -----------------------------------------------------------
    ToolDef {
        name: "psql_query",
        tool: "psql",
        op: "query",
        description: "Run a read-only PostgreSQL query and return rows as JSON.",
        params: &[Param {
            name: "sql",
            ty: Ty::String,
            required: true,
            description: "SQL SELECT statement to execute.",
        }],
    },
    ToolDef {
        name: "psql_tables",
        tool: "psql",
        op: "tables",
        description: "List tables in a PostgreSQL schema (default: public).",
        params: &[Param {
            name: "schema",
            ty: Ty::String,
            required: false,
            description: "Schema to list tables from (default: public).",
        }],
    },
    ToolDef {
        name: "psql_describe",
        tool: "psql",
        op: "describe",
        description: "Describe a PostgreSQL table's columns and types.",
        params: &[Param {
            name: "table",
            ty: Ty::String,
            required: true,
            description: "Table name, optionally schema-qualified (e.g. public.users).",
        }],
    },
    // ---- msql -----------------------------------------------------------
    ToolDef {
        name: "msql_query",
        tool: "msql",
        op: "query",
        description: "Run a read-only MS SQL Server query and return rows as JSON.",
        params: &[Param {
            name: "sql",
            ty: Ty::String,
            required: true,
            description: "SQL SELECT statement to execute.",
        }],
    },
    ToolDef {
        name: "msql_tables",
        tool: "msql",
        op: "tables",
        description: "List tables in an MS SQL Server schema (default: dbo).",
        params: &[Param {
            name: "schema",
            ty: Ty::String,
            required: false,
            description: "Schema to list tables from (default: dbo).",
        }],
    },
    ToolDef {
        name: "msql_describe",
        tool: "msql",
        op: "describe",
        description: "Describe an MS SQL Server table's columns and types.",
        params: &[Param {
            name: "table",
            ty: Ty::String,
            required: true,
            description: "Table name, optionally schema-qualified (e.g. dbo.users).",
        }],
    },
    // ---- dbr: jobs / runs ----------------------------------------------
    ToolDef {
        name: "dbr_jobs_list",
        tool: "dbr",
        op: "jobs/list",
        description: "List Databricks jobs.",
        params: &[Param {
            name: "limit",
            ty: Ty::Integer,
            required: false,
            description: "Maximum number of jobs to return (default: 25).",
        }],
    },
    ToolDef {
        name: "dbr_jobs_get",
        tool: "dbr",
        op: "jobs/get",
        description: "Get a Databricks job by ID.",
        params: &[Param {
            name: "job_id",
            ty: Ty::Integer,
            required: true,
            description: "Job ID.",
        }],
    },
    ToolDef {
        name: "dbr_jobs_trigger",
        tool: "dbr",
        op: "jobs/trigger",
        description: "Trigger a Databricks job run (requires allow_job_runs in config).",
        params: &[Param {
            name: "job_id",
            ty: Ty::Integer,
            required: true,
            description: "Job ID to trigger.",
        }],
    },
    ToolDef {
        name: "dbr_runs_list",
        tool: "dbr",
        op: "runs/list",
        description: "List runs for a Databricks job.",
        params: &[
            Param {
                name: "job_id",
                ty: Ty::Integer,
                required: true,
                description: "Job ID to list runs for.",
            },
            Param {
                name: "limit",
                ty: Ty::Integer,
                required: false,
                description: "Maximum number of runs to return (default: 10).",
            },
        ],
    },
    ToolDef {
        name: "dbr_runs_get",
        tool: "dbr",
        op: "runs/get",
        description: "Get a Databricks job run by ID.",
        params: &[Param {
            name: "run_id",
            ty: Ty::Integer,
            required: true,
            description: "Run ID.",
        }],
    },
    ToolDef {
        name: "dbr_runs_output",
        tool: "dbr",
        op: "runs/output",
        description: "Get the output of a Databricks job run.",
        params: &[Param {
            name: "run_id",
            ty: Ty::Integer,
            required: true,
            description: "Run ID.",
        }],
    },
    // ---- dbr: clusters / warehouses ------------------------------------
    ToolDef {
        name: "dbr_clusters_list",
        tool: "dbr",
        op: "clusters/list",
        description: "List Databricks clusters.",
        params: &[],
    },
    ToolDef {
        name: "dbr_clusters_get",
        tool: "dbr",
        op: "clusters/get",
        description: "Get a Databricks cluster by ID.",
        params: &[Param {
            name: "cluster_id",
            ty: Ty::String,
            required: true,
            description: "Cluster ID.",
        }],
    },
    ToolDef {
        name: "dbr_warehouses_list",
        tool: "dbr",
        op: "warehouses/list",
        description: "List Databricks SQL warehouses.",
        params: &[],
    },
    ToolDef {
        name: "dbr_warehouses_get",
        tool: "dbr",
        op: "warehouses/get",
        description: "Get a Databricks SQL warehouse by ID.",
        params: &[Param {
            name: "warehouse_id",
            ty: Ty::String,
            required: true,
            description: "Warehouse ID.",
        }],
    },
    // ---- dbr: Unity Catalog --------------------------------------------
    ToolDef {
        name: "dbr_catalogs_list",
        tool: "dbr",
        op: "catalogs/list",
        description: "List Unity Catalog catalogs.",
        params: &[Param {
            name: "limit",
            ty: Ty::Integer,
            required: false,
            description: "Maximum number of catalogs to return (default: 100).",
        }],
    },
    ToolDef {
        name: "dbr_catalogs_get",
        tool: "dbr",
        op: "catalogs/get",
        description: "Get a Unity Catalog catalog by name.",
        params: &[Param {
            name: "catalog",
            ty: Ty::String,
            required: true,
            description: "Catalog name.",
        }],
    },
    ToolDef {
        name: "dbr_schemas_list",
        tool: "dbr",
        op: "schemas/list",
        description: "List schemas in a Unity Catalog catalog.",
        params: &[
            Param {
                name: "catalog",
                ty: Ty::String,
                required: true,
                description: "Catalog name.",
            },
            Param {
                name: "limit",
                ty: Ty::Integer,
                required: false,
                description: "Maximum number of schemas to return (default: 100).",
            },
        ],
    },
    ToolDef {
        name: "dbr_schemas_get",
        tool: "dbr",
        op: "schemas/get",
        description: "Get a Unity Catalog schema.",
        params: &[
            Param {
                name: "catalog",
                ty: Ty::String,
                required: true,
                description: "Catalog name.",
            },
            Param {
                name: "schema",
                ty: Ty::String,
                required: true,
                description: "Schema name.",
            },
        ],
    },
    ToolDef {
        name: "dbr_tables_list",
        tool: "dbr",
        op: "tables/list",
        description: "List tables in a Unity Catalog schema.",
        params: &[
            Param {
                name: "catalog",
                ty: Ty::String,
                required: true,
                description: "Catalog name.",
            },
            Param {
                name: "schema",
                ty: Ty::String,
                required: true,
                description: "Schema name.",
            },
            Param {
                name: "limit",
                ty: Ty::Integer,
                required: false,
                description: "Maximum number of tables to return (default: 100).",
            },
            Param {
                name: "omit_columns",
                ty: Ty::Boolean,
                required: false,
                description: "Omit per-column detail to reduce output size.",
            },
        ],
    },
    ToolDef {
        name: "dbr_tables_get",
        tool: "dbr",
        op: "tables/get",
        description: "Get a Unity Catalog table.",
        params: &[
            Param {
                name: "catalog",
                ty: Ty::String,
                required: true,
                description: "Catalog name.",
            },
            Param {
                name: "schema",
                ty: Ty::String,
                required: true,
                description: "Schema name.",
            },
            Param {
                name: "table",
                ty: Ty::String,
                required: true,
                description: "Table name.",
            },
        ],
    },
    ToolDef {
        name: "dbr_query",
        tool: "dbr",
        op: "query",
        description: "Run a SQL query against a Databricks SQL warehouse.",
        params: &[
            Param {
                name: "sql",
                ty: Ty::String,
                required: true,
                description: "SQL statement to execute.",
            },
            Param {
                name: "warehouse_id",
                ty: Ty::String,
                required: false,
                description: "Warehouse ID (defaults to DATABRICKS_WAREHOUSE_ID).",
            },
            Param {
                name: "limit",
                ty: Ty::Integer,
                required: false,
                description: "Maximum number of rows to return (default: 100).",
            },
        ],
    },
    // ---- dbr: bundles ---------------------------------------------------
    ToolDef {
        name: "dbr_bundle_validate",
        tool: "dbr",
        op: "bundle/validate",
        description: "Validate a Databricks Asset Bundle.",
        params: &[Param {
            name: "cwd",
            ty: Ty::String,
            required: false,
            description: "Bundle working directory (defaults to current directory).",
        }],
    },
    ToolDef {
        name: "dbr_bundle_deploy",
        tool: "dbr",
        op: "bundle/deploy",
        description: "Deploy a Databricks Asset Bundle.",
        params: &[Param {
            name: "cwd",
            ty: Ty::String,
            required: false,
            description: "Bundle working directory (defaults to current directory).",
        }],
    },
    ToolDef {
        name: "dbr_bundle_destroy",
        tool: "dbr",
        op: "bundle/destroy",
        description: "Destroy a deployed Databricks Asset Bundle.",
        params: &[Param {
            name: "cwd",
            ty: Ty::String,
            required: false,
            description: "Bundle working directory (defaults to current directory).",
        }],
    },
    ToolDef {
        name: "dbr_bundle_run",
        tool: "dbr",
        op: "bundle/run",
        description: "Run a resource from a Databricks Asset Bundle.",
        params: &[
            Param {
                name: "name",
                ty: Ty::String,
                required: true,
                description: "Resource name to run.",
            },
            Param {
                name: "only",
                ty: Ty::String,
                required: false,
                description: "Restrict the run to a specific sub-resource.",
            },
            Param {
                name: "cwd",
                ty: Ty::String,
                required: false,
                description: "Bundle working directory (defaults to current directory).",
            },
        ],
    },
    ToolDef {
        name: "dbr_bundle_context",
        tool: "dbr",
        op: "bundle/context",
        description: "Show the resolved Databricks Asset Bundle context.",
        params: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for d in CATALOG {
            assert!(seen.insert(d.name), "duplicate MCP tool name: {}", d.name);
        }
    }

    #[test]
    fn tool_names_are_mcp_safe() {
        for d in CATALOG {
            assert!(
                d.name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
                "tool name not MCP-safe: {}",
                d.name
            );
        }
    }

    #[test]
    fn no_internal_ops_exposed() {
        for d in CATALOG {
            assert!(d.op != "auth/store_tokens" && d.op != "auth/get_host");
            assert!(d.tool != "guard");
        }
    }

    #[test]
    fn input_schema_lists_required_and_conn() {
        let def = find("dbr_runs_list").unwrap();
        let schema = def.input_schema();
        // conn is always an optional property.
        assert!(schema["properties"]["conn"].is_object());
        // job_id is required; limit is not.
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "job_id"));
        assert!(!required.iter().any(|v| v == "limit"));
        assert!(!required.iter().any(|v| v == "conn"));
    }

    #[test]
    fn paramless_tool_has_only_conn() {
        let def = find("dbr_clusters_list").unwrap();
        let schema = def.input_schema();
        let props = schema["properties"].as_object().unwrap();
        assert_eq!(props.len(), 1);
        assert!(props.contains_key("conn"));
        assert!(schema["required"].as_array().unwrap().is_empty());
    }
}
