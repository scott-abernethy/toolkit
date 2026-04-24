mod psql;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "tkpsql",
    about = "Read-only PostgreSQL query tool for AI agents"
)]
struct Cli {
    /// Named connection from config (e.g. local, prod). Required if multiple connections are configured.
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
        /// Schema to list tables from (default: public)
        #[arg(long, default_value = "public")]
        schema: String,
    },
    /// Describe a table's columns and types
    Describe {
        /// Table name (optionally schema-qualified, e.g. public.users)
        #[arg(long)]
        table: String,
    },
}

fn main() {
    let cli = Cli::parse();
    let config = psql::load_config(cli.conn.as_deref());

    match cli.command {
        Commands::Query { sql } => psql::run_query(&config, &sql),
        Commands::Tables { schema } => psql::list_tables(&config, &schema),
        Commands::Describe { table } => psql::describe_table(&config, &table),
    }
}
