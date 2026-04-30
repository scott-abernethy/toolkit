mod guard;

use clap::{Parser, Subcommand};
use common::{client, config, exit_with_error, key, Result, ToolkitError};
use secrecy::ExposeSecret;
use std::os::unix::net::UnixStream;
use std::process;

const DAEMON_USER: &str = "_toolkit";
const DAEMON_CONFIG_PATH: &str = "/var/lib/toolkit/.config/toolkit/config.yaml";

/// Default environment variables set by known AI agent harnesses.
/// If any are present, toolkit refuses to run — agents must not be able to
/// invoke key/config management commands (e.g. `toolkit config show` would
/// defeat the entire encryption scheme).
/// These can be overridden via the `harness_detection.env` config section.
const DEFAULT_AGENT_ENV_VARS: &[&str] = &[
    "CLAUDECODE",      // Claude Code (claude.ai/code)
    "OPENCODE",        // opencode (sst/opencode)
    "COPILOT_CLI",     // GitHub Copilot CLI
    "COPILOT_RUN_APP", // GitHub Copilot CLI (run app context)
];

/// Load harness env var names from config, falling back to compiled defaults.
/// Since these names are not sensitive, they remain plaintext even in encrypted
/// configs — no decryption needed.
fn load_agent_env_vars() -> Vec<String> {
    let path = match config::config_path() {
        Ok(p) => p,
        Err(_) => return defaults(),
    };
    if !path.exists() {
        return defaults();
    }
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return defaults(),
    };
    let value: serde_yaml::Value = match serde_yaml::from_str(&contents) {
        Ok(v) => v,
        Err(_) => return defaults(),
    };
    match value
        .get("harness_detection")
        .and_then(|h| h.get("env"))
        .and_then(|e| e.as_sequence())
    {
        Some(seq) => seq
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        None => defaults(),
    }
}

fn defaults() -> Vec<String> {
    DEFAULT_AGENT_ENV_VARS
        .iter()
        .map(|s| s.to_string())
        .collect()
}

fn not_permitted() -> ! {
    eprintln!("Not permitted");
    process::exit(77);
}

fn reject_if_agent() {
    for var in &load_agent_env_vars() {
        if std::env::var(var).is_ok() {
            not_permitted();
        }
    }
}

#[derive(Parser)]
#[command(name = "toolkit", about = "Toolkit key and config management")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate an age keypair and store it in the OS keychain
    Init,
    /// Manage the encrypted config file
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// Generate guarded wrapper scripts in ~/.config/toolkit/bin
    Install,
    /// Manage the toolkit daemon
    Daemon {
        #[command(subcommand)]
        cmd: DaemonCmd,
    },
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
enum DaemonCmd {
    /// Manage the daemon's config file (requires sudo)
    Config {
        #[command(subcommand)]
        cmd: DaemonConfigCmd,
    },
    /// Show daemon status: socket path and reachability
    Status,
}

#[derive(Subcommand)]
enum DaemonConfigCmd {
    /// Open the daemon config in $EDITOR (runs as _toolkit via sudo)
    Edit,
    /// Print the daemon config with secrets masked (runs as _toolkit via sudo)
    Show,
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Open the config in $EDITOR via sops (handles encrypt/decrypt automatically)
    Edit,
    /// Encrypt config.toml in-place using the stored age key
    Encrypt,
    /// Decrypt config.toml in-place (leaves plaintext on disk — use with caution)
    Decrypt,
    /// Print the decrypted config to stdout
    Show,
    /// Print a config template for a known app (e.g. psql, dbr)
    Template {
        /// App name (psql, dbr)
        app: String,
    },
    /// Re-encrypt config with the current encrypted-regex (run after toolkit upgrade)
    Migrate,
}

fn is_agent() -> bool {
    load_agent_env_vars()
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
    // All other commands (init, config, install) must be blocked for agents.
    if !matches!(cli.command, Commands::Guard { .. }) {
        reject_if_agent();
    }
    match cli.command {
        Commands::Init => {
            cmd_init()?;
            Ok(0)
        }
        Commands::Config { cmd } => {
            match cmd {
                ConfigCmd::Edit => cmd_config_edit()?,
                ConfigCmd::Encrypt => cmd_config_encrypt()?,
                ConfigCmd::Decrypt => cmd_config_decrypt()?,
                ConfigCmd::Show => cmd_config_show()?,
                ConfigCmd::Template { app } => cmd_config_template(&app)?,
                ConfigCmd::Migrate => cmd_config_migrate()?,
            }
            Ok(0)
        }
        Commands::Daemon { cmd } => {
            match cmd {
                DaemonCmd::Config { cmd } => match cmd {
                    DaemonConfigCmd::Edit => cmd_daemon_config_edit()?,
                    DaemonConfigCmd::Show => cmd_daemon_config_show()?,
                },
                DaemonCmd::Status => cmd_daemon_status()?,
            }
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

fn cmd_init() -> Result<()> {
    let (private_key, public_key) = key::generate_keypair();

    let path = key::write_key_file(&private_key)?;
    println!("Wrote private key to {} (mode 0600)", path.display());

    println!("Public key (age recipient): {}", public_key);
    println!();
    println!("Next steps:");
    println!("  toolkit config edit              edit the config via sops + $EDITOR");
    println!();
    println!("Agent harness configuration:");
    println!("  toolkit blocks known agent env vars at runtime.");
    println!("  Defaults: CLAUDECODE, OPENCODE, COPILOT_CLI, COPILOT_RUN_APP");
    println!("  Customize via harness_detection.env in config.yaml.");
    println!("  You may also want to add an explicit deny rule in your harness settings:");
    println!();
    println!("  Claude Code (~/.claude/settings.json):");
    println!("    {{\"permissions\": {{\"deny\": [\"Bash(toolkit:*)\"]}}}}");
    Ok(())
}

fn cmd_install() -> Result<()> {
    let path = config::config_path()?;
    if !path.exists() {
        return Err(ToolkitError::config(format!(
            "Config not found: {}. Run `toolkit init` first, then `toolkit config edit` to add connections.",
            path.display()
        )));
    }

    let contents = std::fs::read_to_string(&path)
        .map_err(|e| ToolkitError::config(format!("Failed to read config: {}", e)))?;

    let probe: serde_yaml::Value = serde_yaml::from_str(&contents)
        .map_err(|e| ToolkitError::config(format!("Invalid YAML: {}", e)))?;

    let full: serde_yaml::Value = if config::is_encrypted(&probe) {
        let decrypted = config::decrypt_config(&path)?;
        serde_yaml::from_str(&decrypted)
            .map_err(|e| ToolkitError::config(format!("Invalid decrypted config: {}", e)))?
    } else {
        probe
    };

    let mapping = full
        .as_mapping()
        .ok_or_else(|| ToolkitError::config("Config is not a YAML mapping"))?;

    // Discover guarded apps: top-level sections where any connection has a "binary" field.
    let mut scripts: Vec<(String, String, String)> = Vec::new(); // (name, app, conn)
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
            // A guarded connection has a "binary" field
            if conn_val.get("binary").and_then(|v| v.as_str()).is_some() {
                let name = format!("tk{}-{}", app, conn);
                scripts.push((name, app.to_string(), conn.to_string()));
            }
        }
    }

    if scripts.is_empty() {
        println!("No guarded apps found in config.");
        println!("Guarded app connections have a 'binary' field. Example:");
        println!();
        println!("  kubectl:");
        println!("    dev:");
        println!("      binary: kubectl");
        println!("      env:");
        println!("        KUBECONFIG: /path/to/kubeconfig");
        println!("      allow:");
        println!("        - \"get pod|pods\"");
        println!("      deny:");
        println!("        - \"secret|secrets\"");
        return Ok(());
    }

    let home = std::env::var("HOME").map_err(|_| ToolkitError::config("HOME not set"))?;

    let install_path = full
        .get("install_path")
        .and_then(|v| v.as_str())
        .unwrap_or("$HOME/.local/bin");
    let bin_dir = std::path::PathBuf::from(install_path.replace("$HOME", &home));

    std::fs::create_dir_all(&bin_dir).map_err(|e| {
        ToolkitError::other(format!("Failed to create {}: {}", bin_dir.display(), e))
    })?;

    let mut installed = 0;
    for (name, app, conn) in &scripts {
        let script_path = bin_dir.join(name);
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
    let path = config::config_path()?;

    // Ensure the config directory exists.
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| {
            ToolkitError::config(format!("Failed to create config directory: {}", e))
        })?;
    }

    let key = key::get_private_key()?;
    let public_key = key::public_key_from_private(&key)?;

    // sops can only edit files it encrypted itself. If an existing plaintext
    // file is present, encrypt it in-place first. New (non-existent) files are
    // seeded with a default template before sops opens the editor.
    if !path.exists() {
        let template = format!(
            "# Toolkit config. Managed by `toolkit config edit`. Sensitive data encrypted.\n\
             install_path: \"$HOME/.local/bin\"\n\
             encrypted_regex: \"{}\"\n\n\
             harness_detection:\n  \
             env:\n    \
             - CLAUDECODE\n    \
             - OPENCODE\n    \
             - COPILOT_CLI\n    \
             - COPILOT_RUN_APP\n",
            config::DEFAULT_ENCRYPTED_REGEX
        );
        std::fs::write(&path, template)
            .map_err(|e| ToolkitError::config(format!("Failed to write default config: {}", e)))?;
    }

    let encrypted_regex = config::load_encrypted_regex();

    if path.exists() {
        let contents = std::fs::read_to_string(&path).unwrap_or_default();
        let probe: serde_yaml::Value =
            serde_yaml::from_str(&contents).unwrap_or(serde_yaml::Value::Null);
        if !config::is_encrypted(&probe) {
            let status = process::Command::new("sops")
                .args(["--encrypt", "--encrypted-regex", &encrypted_regex, "-i"])
                .arg(&path)
                .env("SOPS_AGE_RECIPIENTS", &public_key)
                .status()
                .map_err(|e| ToolkitError::crypto(format!("Failed to run sops: {}", e)))?;
            if !status.success() {
                process::exit(status.code().unwrap_or(1));
            }
        }
    }

    let status = process::Command::new("sops")
        .args(["--encrypted-regex", &encrypted_regex])
        .arg(&path)
        .env("SOPS_AGE_KEY", key.expose_secret())
        .env("SOPS_AGE_RECIPIENTS", &public_key)
        .env_remove("SOPS_AGE_KEY_FILE")
        .status()
        .map_err(|e| ToolkitError::crypto(format!("Failed to run sops: {}", e)))?;

    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn cmd_config_encrypt() -> Result<()> {
    let path = config::config_path()?;
    if !path.exists() {
        return Err(ToolkitError::not_found(format!(
            "Config not found: {}",
            path.display()
        )));
    }

    let contents = std::fs::read_to_string(&path)
        .map_err(|e| ToolkitError::config(format!("Failed to read config: {}", e)))?;

    let probe: serde_yaml::Value = serde_yaml::from_str(&contents)
        .map_err(|e| ToolkitError::config(format!("Invalid YAML: {}", e)))?;

    if config::is_encrypted(&probe) {
        println!("Config is already encrypted.");
        return Ok(());
    }

    let private_key = key::get_private_key()?;
    let public_key = key::public_key_from_private(&private_key)?;

    let encrypted_regex = config::load_encrypted_regex();
    let status = process::Command::new("sops")
        .args(["--encrypt", "--encrypted-regex", &encrypted_regex, "-i"])
        .arg(&path)
        .env("SOPS_AGE_RECIPIENTS", &public_key)
        .status()
        .map_err(|e| ToolkitError::crypto(format!("Failed to run sops: {}", e)))?;

    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }

    println!("Encrypted: {}", path.display());
    Ok(())
}

fn cmd_config_decrypt() -> Result<()> {
    let path = config::config_path()?;
    if !path.exists() {
        return Err(ToolkitError::not_found(format!(
            "Config not found: {}",
            path.display()
        )));
    }

    let contents = std::fs::read_to_string(&path)
        .map_err(|e| ToolkitError::config(format!("Failed to read config: {}", e)))?;

    let probe: serde_yaml::Value = serde_yaml::from_str(&contents)
        .map_err(|e| ToolkitError::config(format!("Invalid YAML: {}", e)))?;

    if !config::is_encrypted(&probe) {
        println!("Config is not encrypted.");
        return Ok(());
    }

    let key = key::get_private_key()?;

    let status = process::Command::new("sops")
        .args(["--decrypt", "-i"])
        .arg(&path)
        .env("SOPS_AGE_KEY", key.expose_secret())
        .env_remove("SOPS_AGE_KEY_FILE")
        .status()
        .map_err(|e| ToolkitError::crypto(format!("Failed to run sops: {}", e)))?;

    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }

    println!("Decrypted: {}", path.display());
    Ok(())
}

fn cmd_config_migrate() -> Result<()> {
    let path = config::config_path()?;
    if !path.exists() {
        return Err(ToolkitError::not_found(format!(
            "Config not found: {}",
            path.display()
        )));
    }

    let contents = std::fs::read_to_string(&path)
        .map_err(|e| ToolkitError::config(format!("Failed to read config: {}", e)))?;

    let probe: serde_yaml::Value = serde_yaml::from_str(&contents)
        .map_err(|e| ToolkitError::config(format!("Invalid YAML: {}", e)))?;

    if !config::is_encrypted(&probe) {
        return Err(ToolkitError::config(
            "Config is not encrypted — nothing to migrate.",
        ));
    }

    let key = key::get_private_key()?;
    let public_key = key::public_key_from_private(&key)?;

    // Decrypt in-place
    let status = process::Command::new("sops")
        .args(["--decrypt", "-i"])
        .arg(&path)
        .env("SOPS_AGE_KEY", key.expose_secret())
        .env_remove("SOPS_AGE_KEY_FILE")
        .status()
        .map_err(|e| ToolkitError::crypto(format!("Failed to run sops: {}", e)))?;
    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }

    // Re-encrypt with the regex now in the (decrypted) config — picks up any
    // edit the user made to `encrypted_regex` since the last encryption.
    let encrypted_regex = config::load_encrypted_regex();
    let status = process::Command::new("sops")
        .args(["--encrypt", "--encrypted-regex", &encrypted_regex, "-i"])
        .arg(&path)
        .env("SOPS_AGE_RECIPIENTS", &public_key)
        .status()
        .map_err(|e| ToolkitError::crypto(format!("Failed to run sops: {}", e)))?;
    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }

    println!("Migrated: {}", path.display());
    println!("Encrypted fields: {}", encrypted_regex);
    Ok(())
}

fn cmd_config_show() -> Result<()> {
    let path = config::config_path()?;
    if !path.exists() {
        return Err(ToolkitError::not_found(format!(
            "Config not found: {}",
            path.display()
        )));
    }

    let contents = std::fs::read_to_string(&path)
        .map_err(|e| ToolkitError::config(format!("Failed to read config: {}", e)))?;

    let probe: serde_yaml::Value = serde_yaml::from_str(&contents)
        .map_err(|e| ToolkitError::config(format!("Invalid YAML: {}", e)))?;

    if config::is_encrypted(&probe) {
        let decrypted = config::decrypt_config(&path)?;
        print!("{}", decrypted);
    } else {
        print!("{}", contents);
    }
    Ok(())
}

fn cmd_daemon_status() -> Result<()> {
    let socket_path = std::env::var("TOOLKIT_SOCKET")
        .unwrap_or_else(|_| client::DEFAULT_SOCKET.to_owned());
    let reachable = UnixStream::connect(&socket_path).is_ok();
    println!(
        "{}",
        serde_json::json!({"socket": socket_path, "reachable": reachable})
    );
    Ok(())
}

fn cmd_daemon_config_edit() -> Result<()> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let status = process::Command::new("sudo")
        .args(["-u", DAEMON_USER, &editor, DAEMON_CONFIG_PATH])
        .status()
        .map_err(|e| ToolkitError::other(format!("failed to run sudo: {e}")))?;
    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn cmd_daemon_config_show() -> Result<()> {
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
    binary: databricks
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

    println!("# Add to your config via `toolkit config edit`:");
    print!("{}", template);
    Ok(())
}
