mod msql;

use clap::{Parser, Subcommand};
use common::{exit_with_error, Result};

#[derive(Parser)]
#[command(
    name = "tkmsql",
    about = "Read-only MS SQL Server query tool for AI agents"
)]
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

fn print_json(v: &impl serde::Serialize) {
    println!("{}", serde_json::to_string(v).unwrap());
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let config = msql::load_config(cli.conn.as_deref())?;

    let result = match cli.command {
        Commands::Query { sql } => msql::run_query(&config, &sql).await?,
        Commands::Tables { schema } => msql::list_tables(&config, &schema).await?,
        Commands::Describe { table } => msql::describe_table(&config, &table).await?,
    };
    print_json(&result);
    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        exit_with_error(e);
    }
}
