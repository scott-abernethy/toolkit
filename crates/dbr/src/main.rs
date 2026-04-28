mod dbr;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "tkdbr", about = "Databricks CLI wrapper for AI agents")]
struct Cli {
    /// Named connection from config (e.g. dev, prod). Required if multiple connections are configured.
    #[arg(long, global = true)]
    conn: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List and inspect jobs
    Jobs {
        #[command(subcommand)]
        cmd: JobsCmd,
    },
    /// List and inspect job runs
    Runs {
        #[command(subcommand)]
        cmd: RunsCmd,
    },
    /// List and inspect clusters
    Clusters {
        #[command(subcommand)]
        cmd: ClustersCmd,
    },
    /// List and inspect SQL warehouses
    Warehouses {
        #[command(subcommand)]
        cmd: WarehousesCmd,
    },
    /// Explore Unity Catalog metadata
    Catalogs {
        #[command(subcommand)]
        cmd: CatalogsCmd,
    },
    /// Explore schemas in a catalog
    Schemas {
        #[command(subcommand)]
        cmd: SchemasCmd,
    },
    /// Explore tables in a schema
    Tables {
        #[command(subcommand)]
        cmd: TablesCmd,
    },
    /// Manage Databricks bundles (validate, deploy, run)
    Bundle {
        #[command(subcommand)]
        cmd: BundleCmd,
    },
    /// Print a valid auth token for the configured Databricks host.
    /// Runs `databricks auth token --host <host>` with inherited stdio.
    Auth,
    /// Execute a SQL query against a warehouse
    Query {
        /// SQL statement to execute
        #[arg(long)]
        sql: String,
        /// SQL warehouse ID (overrides config warehouse_id)
        #[arg(long)]
        warehouse_id: Option<String>,
        /// Maximum number of rows to return
        #[arg(long, default_value = "100")]
        limit: u32,
    },
}

#[derive(Subcommand)]
enum JobsCmd {
    /// List jobs (compact: id, name)
    List {
        /// Maximum number of jobs to return
        #[arg(long, default_value = "25")]
        limit: u32,
    },
    /// Get job details (compact: id, name, tasks, schedule)
    Get {
        #[arg(long)]
        job_id: i64,
    },
    /// Trigger a job run (requires allow_job_runs = true in config)
    Trigger {
        #[arg(long)]
        job_id: i64,
    },
}

#[derive(Subcommand)]
enum RunsCmd {
    /// List recent runs for a job
    List {
        #[arg(long)]
        job_id: i64,
        /// Maximum number of runs to return
        #[arg(long, default_value = "10")]
        limit: u32,
    },
    /// Get run status (compact: run_id, state, result, timing)
    Get {
        #[arg(long)]
        run_id: i64,
    },
    /// Get run output (notebook result or error)
    Output {
        #[arg(long)]
        run_id: i64,
    },
}

#[derive(Subcommand)]
enum ClustersCmd {
    /// List clusters (compact: id, name, state)
    List,
    /// Get cluster details
    Get {
        #[arg(long)]
        cluster_id: String,
    },
}

#[derive(Subcommand)]
enum WarehousesCmd {
    /// List SQL warehouses (compact: id, name, state)
    List,
    /// Get warehouse details
    Get {
        #[arg(long)]
        warehouse_id: String,
    },
}

#[derive(Subcommand)]
enum CatalogsCmd {
    /// List catalogs accessible to current user
    List {
        /// Maximum number of catalogs to return
        #[arg(long, default_value = "100")]
        limit: u32,
    },
    /// Get details about a specific catalog
    Get {
        #[arg(long)]
        catalog: String,
    },
}

#[derive(Subcommand)]
enum SchemasCmd {
    /// List schemas in a catalog
    List {
        #[arg(long)]
        catalog: String,
        /// Maximum number of schemas to return
        #[arg(long, default_value = "100")]
        limit: u32,
    },
    /// Get details about a specific schema
    Get {
        #[arg(long)]
        catalog: String,
        #[arg(long)]
        schema: String,
    },
}

#[derive(Subcommand)]
enum TablesCmd {
    /// List tables in a schema
    List {
        #[arg(long)]
        catalog: String,
        #[arg(long)]
        schema: String,
        /// Maximum number of tables to return
        #[arg(long, default_value = "100")]
        limit: u32,
        /// Omit column definitions (lighter response)
        #[arg(long)]
        omit_columns: bool,
    },
    /// Get table schema and metadata
    Get {
        #[arg(long)]
        catalog: String,
        #[arg(long)]
        schema: String,
        #[arg(long)]
        table: String,
    },
}

#[derive(Subcommand)]
enum BundleCmd {
    /// Validate the bundle (runs `databricks bundle validate -t <target>`)
    Validate,
    /// Deploy the bundle (runs `databricks bundle deploy -t <target>`)
    Deploy,
    /// Run a bundle resource (runs `databricks bundle run <name> -t <target>`)
    Run {
        /// Name of the bundle resource to run
        #[arg(value_name = "NAME")]
        name: String,
        /// Comma-separated list of task keys to run (for jobs)
        #[arg(long)]
        only: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();
    let config = dbr::load_config(cli.conn.as_deref());

    match cli.command {
        Commands::Jobs { cmd } => match cmd {
            JobsCmd::List { limit } => dbr::jobs_list(&config, limit),
            JobsCmd::Get { job_id } => dbr::jobs_get(&config, job_id),
            JobsCmd::Trigger { job_id } => dbr::jobs_trigger(&config, job_id),
        },
        Commands::Runs { cmd } => match cmd {
            RunsCmd::List { job_id, limit } => dbr::runs_list(&config, job_id, limit),
            RunsCmd::Get { run_id } => dbr::runs_get(&config, run_id),
            RunsCmd::Output { run_id } => dbr::runs_output(&config, run_id),
        },
        Commands::Clusters { cmd } => match cmd {
            ClustersCmd::List => dbr::clusters_list(&config),
            ClustersCmd::Get { cluster_id } => dbr::clusters_get(&config, &cluster_id),
        },
        Commands::Warehouses { cmd } => match cmd {
            WarehousesCmd::List => dbr::warehouses_list(&config),
            WarehousesCmd::Get { warehouse_id } => dbr::warehouses_get(&config, &warehouse_id),
        },
        Commands::Catalogs { cmd } => match cmd {
            CatalogsCmd::List { limit } => dbr::catalogs_list(&config, limit),
            CatalogsCmd::Get { catalog } => dbr::catalogs_get(&config, &catalog),
        },
        Commands::Schemas { cmd } => match cmd {
            SchemasCmd::List { catalog, limit } => dbr::schemas_list(&config, &catalog, limit),
            SchemasCmd::Get { catalog, schema } => dbr::schemas_get(&config, &catalog, &schema),
        },
        Commands::Tables { cmd } => match cmd {
            TablesCmd::List {
                catalog,
                schema,
                limit,
                omit_columns,
            } => dbr::tables_list(&config, &catalog, &schema, limit, omit_columns),
            TablesCmd::Get {
                catalog,
                schema,
                table,
            } => dbr::tables_get(&config, &catalog, &schema, &table),
        },
        Commands::Bundle { cmd } => match cmd {
            BundleCmd::Validate => dbr::bundle_validate(&config),
            BundleCmd::Deploy => dbr::bundle_deploy(&config),
            BundleCmd::Run { name, only } => dbr::bundle_run(&config, &name, only.as_deref()),
        },
        Commands::Auth => dbr::auth_token(&config),
        Commands::Query {
            sql,
            warehouse_id,
            limit,
        } => dbr::query(&config, &sql, warehouse_id.as_deref(), limit),
    }
}
