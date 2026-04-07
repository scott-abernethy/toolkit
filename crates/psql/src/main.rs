mod psql;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "tkpsql", about = "Read-only PostgreSQL query tool for AI agents")]
struct Cli {
    /// Config file path (default: ~/.config/tkpsql/config.toml)
    #[arg(long, global = true)]
    config: Option<String>,

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
        /// Table name (optionally schema-qualified)
        #[arg(long)]
        table: String,
    },
}

fn main() {
    let cli = Cli::parse();
    let config = psql::load_config(cli.config);

    match cli.command {
        Commands::Query { sql } => psql::run_query(&config, &sql),
        Commands::Tables { schema } => psql::list_tables(&config, &schema),
        Commands::Describe { table } => psql::describe_table(&config, &table),
    }
}
