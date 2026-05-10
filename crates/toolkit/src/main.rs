mod guard;

use clap::{Parser, Subcommand};
use common::{client, exit_with_error, Result, ToolkitError};
use std::os::unix::net::UnixStream;
use std::process;

const DAEMON_USER: &str = "_toolkit";
const DAEMON_CONFIG_PATH: &str = "/var/lib/toolkit/.config/toolkit/config.yaml";

/// Default environment variables set by known AI agent harnesses.
/// If any are present, toolkit refuses to run — agents must not be able to
/// invoke config management commands.
const DEFAULT_AGENT_ENV_VARS: &[&str] = &[
    "CLAUDECODE",      // Claude Code (claude.ai/code)
    "OPENCODE",        // opencode (sst/opencode)
    "COPILOT_CLI",     // GitHub Copilot CLI
    "COPILOT_RUN_APP", // GitHub Copilot CLI (run app context)
];

fn not_permitted() -> ! {
    eprintln!("Not permitted");
    process::exit(77);
}

fn reject_if_agent() {
    for var in DEFAULT_AGENT_ENV_VARS {
        if std::env::var(var).is_ok() {
            not_permitted();
        }
    }
}

#[derive(Parser)]
#[command(name = "toolkit", about = "Toolkit daemon management and CLI guard")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage the daemon config file (requires sudo)
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// Show daemon status: socket path and reachability
    Status,
    /// Read the daemon-owned error log (requires sudo)
    Logs {
        /// Number of trailing lines to show
        #[arg(long, default_value_t = 100)]
        tail: u32,

        /// Follow the log as it grows
        #[arg(long)]
        follow: bool,
    },
    /// Generate guarded wrapper scripts
    Install,
    /// Run a CLI through the toolkit guard (used by generated wrapper scripts)
    Guard {
        /// App name — matches config section (e.g. kubectl, pup)
        #[arg(long)]
        app: String,

        /// Named connection from config. Required if multiple connections exist.
        #[arg(long)]
        conn: Option<String>,

        /// Print guard overhead timing to stderr
        #[arg(long)]
        debug: bool,

        /// Arguments to pass to the wrapped CLI (after --)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, last = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Open the daemon config in $EDITOR (runs as _toolkit via sudo)
    Edit,
    /// Print the daemon config with secrets masked (runs as _toolkit via sudo)
    Show,
    /// Print a config template for a known app (e.g. psql, dbr, msql)
    Template {
        /// App name (psql, dbr, msql)
        app: String,
    },
}

fn is_agent() -> bool {
    DEFAULT_AGENT_ENV_VARS
        .iter()
        .any(|var| std::env::var(var).is_ok())
}

fn run() -> Result<i32> {
    let start = std::time::Instant::now();

    // When running under an agent, use try_parse so that missing/invalid
    // subcommands produce "Not permitted" instead of clap help text.
    let cli = if is_agent() {
        match Cli::try_parse() {
            Ok(cli) => cli,
            Err(_) => not_permitted(),
        }
    } else {
        Cli::parse()
    };

    // Guard is invoked by generated wrapper scripts in agent context — allow it.
    // All other commands (config, install, status, logs) must be blocked for agents.
    if !matches!(cli.command, Commands::Guard { .. }) {
        reject_if_agent();
    }
    match cli.command {
        Commands::Config { cmd } => {
            match cmd {
                ConfigCmd::Edit => cmd_config_edit()?,
                ConfigCmd::Show => cmd_config_show()?,
                ConfigCmd::Template { app } => cmd_config_template(&app)?,
            }
            Ok(0)
        }
        Commands::Status => {
            cmd_status()?;
            Ok(0)
        }
        Commands::Logs { tail, follow } => {
            cmd_logs(tail, follow)?;
            Ok(0)
        }
        Commands::Install => {
            cmd_install()?;
            Ok(0)
        }
        Commands::Guard {
            app,
            conn,
            debug,
            args,
        } => {
            let config = guard::load_config(&app, conn.as_deref())?;
            let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            guard::check_rules(&config, &arg_refs)?;
            if debug {
                let elapsed = start.elapsed();
                eprintln!("[guard] overhead: {:.1}ms", elapsed.as_secs_f64() * 1000.0);
            }
            guard::run(&config, &args)
        }
    }
}

fn main() {
    match run() {
        Ok(code) => process::exit(code),
        Err(e) => exit_with_error(e),
    }
}

fn cmd_install() -> Result<()> {
    // Fetch guard apps and install_path from the daemon
    let req = common::protocol::Request::new("guard", None, "list", serde_json::json!({}));
    let value = client::send(&req)?;

    let apps = value
        .get("apps")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ToolkitError::other("unexpected response from daemon"))?;

    if apps.is_empty() {
        println!("No guarded apps found in config.");
        println!("Guarded app connections have a 'command' field. Example:");
        println!();
        println!("  kubectl:");
        println!("    dev:");
        println!("      command: kubectl");
        println!("      env:");
        println!("        KUBECONFIG: /path/to/kubeconfig");
        println!("      allow:");
        println!("        - \"get pod|pods\"");
        println!("      deny:");
        println!("        - \"secret|secrets\"");
        return Ok(());
    }

    let home = std::env::var("HOME").map_err(|_| ToolkitError::config("HOME not set"))?;

    let install_path = value
        .get("install_path")
        .and_then(|v| v.as_str())
        .unwrap_or("$HOME/.local/bin");
    let bin_dir = std::path::PathBuf::from(install_path.replace("$HOME", &home));

    std::fs::create_dir_all(&bin_dir).map_err(|e| {
        ToolkitError::other(format!("Failed to create {}: {}", bin_dir.display(), e))
    })?;

    let mut installed = 0;
    for entry in apps {
        let app = entry.get("app").and_then(|v| v.as_str()).unwrap_or("");
        let conn = entry.get("conn").and_then(|v| v.as_str()).unwrap_or("");
        if app.is_empty() || conn.is_empty() {
            continue;
        }

        let name = format!("tk{}-{}", app, conn);
        let script_path = bin_dir.join(&name);
        let script = format!(
            "#!/bin/sh\nexec toolkit guard --app {} --conn {} -- \"$@\"\n",
            app, conn
        );

        if let Err(e) = std::fs::write(&script_path, &script) {
            eprintln!("Failed to write {}: {}", script_path.display(), e);
            continue;
        }

        // chmod +x
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o755);
            if let Err(e) = std::fs::set_permissions(&script_path, perms) {
                eprintln!("Failed to chmod {}: {}", script_path.display(), e);
                continue;
            }
        }

        println!("  {}", name);
        installed += 1;
    }

    println!();
    println!("Installed {} scripts to {}", installed, bin_dir.display());
    println!();
    println!("Add to your shell profile if not already present:");
    println!("  export PATH=\"{}:$PATH\"", install_path);
    Ok(())
}

fn cmd_config_edit() -> Result<()> {
    // Choose an editor that's likely to be available in the _toolkit user's environment.
    // Don't use EDITOR env var because it may not be set or accessible for _toolkit user.
    let editor = "vim";
    let status = process::Command::new("sudo")
        .args(["-u", DAEMON_USER, editor, DAEMON_CONFIG_PATH])
        .status()
        .map_err(|e| ToolkitError::other(format!("failed to run sudo: {e}")))?;
    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn cmd_config_show() -> Result<()> {
    let output = process::Command::new("sudo")
        .args(["-u", DAEMON_USER, "cat", DAEMON_CONFIG_PATH])
        .output()
        .map_err(|e| ToolkitError::other(format!("failed to run sudo: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ToolkitError::other(format!(
            "failed to read daemon config: {}",
            stderr.trim()
        )));
    }
    let contents = String::from_utf8(output.stdout)
        .map_err(|_| ToolkitError::other("daemon config is not valid UTF-8"))?;
    let mut value: serde_yaml::Value = serde_yaml::from_str(&contents)
        .map_err(|e| ToolkitError::config(format!("invalid daemon config: {e}")))?;
    mask_sensitive(&mut value);
    let masked = serde_yaml::to_string(&value)
        .map_err(|e| ToolkitError::other(format!("failed to serialize config: {e}")))?;
    print!("{}", masked);
    Ok(())
}

fn cmd_status() -> Result<()> {
    let socket_path =
        std::env::var("TOOLKIT_SOCKET").unwrap_or_else(|_| client::DEFAULT_SOCKET.to_owned());
    let reachable = UnixStream::connect(&socket_path).is_ok();

    let cli_version = common::protocol::PROTOCOL_VERSION;
    let daemon_version: Option<u32> = if reachable {
        let req = common::protocol::Request::new("meta", None, "version", serde_json::json!({}));
        client::send(&req).ok().and_then(|v| {
            v.get("protocol_version")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
        })
    } else {
        None
    };

    let mut out = serde_json::json!({
        "socket": socket_path,
        "reachable": reachable,
        "protocol_version": {
            "cli": cli_version,
            "daemon": daemon_version,
        },
    });
    if let Some(d) = daemon_version {
        if d != cli_version {
            out["warning"] = serde_json::json!(format!(
                "protocol version mismatch — cli={cli_version}, daemon={d}"
            ));
        }
    }
    println!("{out}");
    Ok(())
}

fn cmd_logs(tail: u32, follow: bool) -> Result<()> {
    let path = common::errorlog::path();
    let tail_arg = tail.to_string();

    let mut cmd = process::Command::new("sudo");
    cmd.args(["-u", DAEMON_USER, "tail", "-n"]).arg(&tail_arg);
    if follow {
        cmd.arg("-F");
    }
    cmd.arg(&path);

    let status = cmd
        .status()
        .map_err(|e| ToolkitError::other(format!("failed to run sudo: {e}")))?;
    if !status.success() {
        let code = status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "signal".to_string());
        return Err(ToolkitError::other(format!(
            "failed to read daemon logs from {} (exit {code})",
            path.display()
        )));
    }
    Ok(())
}

fn mask_sensitive(value: &mut serde_yaml::Value) {
    match value {
        serde_yaml::Value::Mapping(map) => {
            for (k, v) in map.iter_mut() {
                let key = k.as_str().unwrap_or("").to_lowercase();
                if key.contains("password") || key.contains("token") || key.contains("secret") {
                    if v.is_string() {
                        *v = serde_yaml::Value::String("***".to_string());
                    }
                } else {
                    mask_sensitive(v);
                }
            }
        }
        serde_yaml::Value::Sequence(seq) => {
            for v in seq.iter_mut() {
                mask_sensitive(v);
            }
        }
        _ => {}
    }
}

fn cmd_config_template(app: &str) -> Result<()> {
    let template = match app {
        "psql" => {
            "\
psql:
  conn:
    host: localhost
    port: 5432
    database: mydb
    user: readonly
    password: changeme
    tls: false
    writable_tables: []
"
        }
        "dbr" => {
            "\
dbr:
  dev:
    command: databricks
    env:
      DATABRICKS_HOST: https://dbc-abc123.cloud.databricks.com
      DATABRICKS_AUTH_TYPE: external-browser
      DATABRICKS_ACCOUNT_ID: 00000000-0000-0000-0000-000000000000
      DATABRICKS_WAREHOUSE_ID: abc1234567890abcdef
      # Token-based auth (alternative to external-browser):
      # DATABRICKS_AUTH_TYPE: pat
      # DATABRICKS_TOKEN: dapi...
    allow: []
    deny: []
"
        }
        "msql" => {
            "\
msql:
  conn:
    host: sql-server.internal
    port: 1433
    database: mydb
    user: readonly
    password: changeme
    tls: true
    trust_cert: false
    writable_tables: []
"
        }
        _ => {
            return Err(ToolkitError::not_found(format!(
                "Unknown app: {}. Known apps: psql, dbr, msql",
                app
            )));
        }
    };

    println!("# Add to daemon config via `toolkit config edit`:");
    print!("{}", template);
    Ok(())
}
