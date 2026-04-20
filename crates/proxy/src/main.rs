mod proxy;

use clap::Parser;

#[derive(Parser)]
#[command(name = "tkproxy", about = "CLI firewall proxy for AI agents")]
struct Cli {
    /// App name — matches config section (e.g. kubectl, pup)
    #[arg(long)]
    app: String,

    /// Named connection from config. Required if multiple connections exist.
    #[arg(long)]
    conn: Option<String>,

    /// Arguments to pass to the wrapped CLI (after --)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, last = true)]
    args: Vec<String>,
}

fn main() {
    let cli = Cli::parse();
    let config = proxy::load_config(&cli.app, cli.conn.as_deref());

    let arg_refs: Vec<&str> = cli.args.iter().map(|s| s.as_str()).collect();
    proxy::check_rules(&config, &arg_refs);
    proxy::run(&config, &cli.args);
}
