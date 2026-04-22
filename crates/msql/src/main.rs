mod msql;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "tkmsql", about = "Read-only MS SQL Server query tool for AI agents")]
struct Cli {
    /// Named connection from config (e.g. onprem, prod). Required if multiple connections are configured.
    #[arg(long, global = true)]
    conn: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Execute a read-only SQL query and return results as JSON
    Query {
        /// SQL query to execute
        #[arg(long)]
        sql: String,
    },
    /// List all tables in the database (or a specific schema)
    Tables {
        /// Schema to list tables from (default: dbo)
        #[arg(long, default_value = "dbo")]
        schema: String,
    },
    /// Describe a table's columns and types
    Describe {
        /// Table name (optionally schema-qualified, e.g. dbo.users)
        #[arg(long)]
        table: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let config = msql::load_config(cli.conn.as_deref());

    match cli.command {
        Commands::Query { sql } => msql::run_query(&config, &sql).await,
        Commands::Tables { schema } => msql::list_tables(&config, &schema).await,
        Commands::Describe { table } => msql::describe_table(&config, &table).await,
    }
}
