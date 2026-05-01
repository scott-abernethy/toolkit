use clap::{Parser, Subcommand};
use common::protocol::Request;
use common::{exit_with_error, Result};
use serde_json::json;

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

fn print_json(v: &impl serde::Serialize) {
    println!("{}", serde_json::to_string(v).unwrap());
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    let (op, params) = match &cli.command {
        Commands::Query { sql } => ("query", json!({"sql": sql})),
        Commands::Tables { schema } => ("tables", json!({"schema": schema})),
        Commands::Describe { table } => ("describe", json!({"table": table})),
    };
    let req = Request {
        tool: "psql".to_owned(),
        conn: cli.conn,
        op: op.to_owned(),
        params,
    };
    let result = common::client::send(&req)?;
    print_json(&result);
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        exit_with_error(e);
    }
}
