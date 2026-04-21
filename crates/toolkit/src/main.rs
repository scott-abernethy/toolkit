mod guard;

use clap::{Parser, Subcommand};
use common::{config, key};
use secrecy::ExposeSecret;
use std::process;

/// Environment variables set by known AI agent harnesses.
/// If any are present, toolkit refuses to run — agents must not be able to
/// invoke key/config management commands (e.g. `toolkit config show` would
/// defeat the entire encryption scheme).
/// Only encrypt fields that contain credentials. Structure and non-sensitive
/// values (port, tls, allow_job_runs, etc.) remain readable in the encrypted file.
const ENCRYPTED_REGEX: &str = "^(host|database|user|password|token|DATABRICKS_HOST|DATABRICKS_TOKEN|DATABRICKS_ACCOUNT_ID)$";

const AGENT_ENV_VARS: &[&str] = &[
    "CLAUDECODE", // Claude Code (claude.ai/code)
    "OPENCODE",   // opencode (sst/opencode)
];

fn reject_if_agent() {
    for var in AGENT_ENV_VARS {
        if std::env::var(var).is_ok() {
            eprintln!("Not Allowed");
            process::exit(1);
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
    AGENT_ENV_VARS.iter().any(|var| std::env::var(var).is_ok())
}

fn main() {
    let start = std::time::Instant::now();

    // When running under an agent, use try_parse so that missing/invalid
    // subcommands produce "Not Allowed" instead of clap help text.
    let cli = if is_agent() {
        match Cli::try_parse() {
            Ok(cli) => cli,
            Err(_) => {
                eprintln!("Not Allowed");
                process::exit(1);
            }
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
        Commands::Init => cmd_init(),
        Commands::Config { cmd } => match cmd {
            ConfigCmd::Edit => cmd_config_edit(),
            ConfigCmd::Encrypt => cmd_config_encrypt(),
            ConfigCmd::Decrypt => cmd_config_decrypt(),
            ConfigCmd::Show => cmd_config_show(),
            ConfigCmd::Template { app } => cmd_config_template(&app),
            ConfigCmd::Migrate => cmd_config_migrate(),
        },
        Commands::Install => cmd_install(),
        Commands::Guard { app, conn, debug, args } => {
            let config = guard::load_config(&app, conn.as_deref());
            let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            guard::check_rules(&config, &arg_refs);
            if debug {
                let elapsed = start.elapsed();
                eprintln!("[guard] overhead: {:.1}ms", elapsed.as_secs_f64() * 1000.0);
            }
            guard::run(&config, &args);
        }
    }
}

fn cmd_init() {
    let (private_key, public_key) = key::generate_keypair();

    match key::write_key_file(&private_key) {
        Ok(path) => println!("Wrote private key to {} (mode 0600)", path.display()),
        Err(e) => {
            eprintln!("Error writing key file: {}", e);
            process::exit(1);
        }
    }

    println!("Public key (age recipient): {}", public_key);
    println!();
    println!("Next steps:");
    println!("  toolkit config edit              edit the config via sops + $EDITOR");
    println!();
    println!("Agent harness configuration:");
    println!("  toolkit blocks known agent env vars (CLAUDECODE, OPENCODE) at runtime.");
    println!("  GitHub Copilot CLI does not set such a variable — add an explicit deny");
    println!("  rule in your harness settings to cover it:");
    println!();
    println!("  Claude Code (~/.claude/settings.json):");
    println!("    {{\"permissions\": {{\"deny\": [\"Bash(toolkit:*)\"]}}}}");
}

fn cmd_install() {
    let path = config::config_path();
    if !path.exists() {
        eprintln!("Config not found: {}", path.display());
        eprintln!("Run `toolkit init` first, then `toolkit config edit` to add connections.");
        process::exit(1);
    }

    let contents = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("Failed to read config: {}", e);
        process::exit(1);
    });

    let probe: serde_yaml::Value = serde_yaml::from_str(&contents).unwrap_or_else(|e| {
        eprintln!("Invalid YAML: {}", e);
        process::exit(1);
    });

    let full: serde_yaml::Value = if config::is_encrypted(&probe) {
        let decrypted = config::decrypt_config(&path);
        serde_yaml::from_str(&decrypted).unwrap_or_else(|e| {
            eprintln!("Invalid decrypted config: {}", e);
            process::exit(1);
        })
    } else {
        probe
    };

    let mapping = match full.as_mapping() {
        Some(m) => m,
        None => {
            eprintln!("Config is not a YAML mapping");
            process::exit(1);
        }
    };

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
        return;
    }

    let home = std::env::var("HOME").unwrap_or_else(|_| {
        eprintln!("HOME not set");
        process::exit(1);
    });

    let install_path = full
        .get("install_path")
        .and_then(|v| v.as_str())
        .unwrap_or("$HOME/.local/bin");
    let bin_dir =
        std::path::PathBuf::from(install_path.replace("$HOME", &home));

    std::fs::create_dir_all(&bin_dir).unwrap_or_else(|e| {
        eprintln!("Failed to create {}: {}", bin_dir.display(), e);
        process::exit(1);
    });

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
}

fn cmd_config_edit() {
    let path = config::config_path();

    // Ensure the config directory exists.
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap_or_else(|e| {
            eprintln!("Failed to create config directory: {}", e);
            process::exit(1);
        });
    }

    let key = key::get_private_key().unwrap_or_else(|e| {
        eprintln!("Error retrieving key: {}", e);
        process::exit(1);
    });

    let public_key = key::public_key_from_private(&key).unwrap_or_else(|e| {
        eprintln!("Error deriving public key: {}", e);
        process::exit(1);
    });

    // sops can only edit files it encrypted itself. If an existing plaintext
    // file is present, encrypt it in-place first. New (non-existent) files are
    // seeded with a default template before sops opens the editor.
    if !path.exists() {
        let template = "# Toolkit config. Managed by `toolkit config edit`. Sensitive data encrypted.\ninstall_path: \"$HOME/.local/bin\"\n";
        std::fs::write(&path, template).unwrap_or_else(|e| {
            eprintln!("Failed to write default config: {}", e);
            process::exit(1);
        });
    }

    if path.exists() {
        let contents = std::fs::read_to_string(&path).unwrap_or_default();
        let probe: serde_yaml::Value = serde_yaml::from_str(&contents).unwrap_or(serde_yaml::Value::Null);
        if !config::is_encrypted(&probe) {
            let status = process::Command::new("sops")
                .args(["--encrypt", "--encrypted-regex", ENCRYPTED_REGEX, "-i"])
                .arg(&path)
                .env("SOPS_AGE_RECIPIENTS", &public_key)
                .status()
                .unwrap_or_else(|e| {
                    eprintln!("Failed to run sops: {}", e);
                    process::exit(1);
                });
            if !status.success() {
                process::exit(status.code().unwrap_or(1));
            }
        }
    }

    let status = process::Command::new("sops")
        .args(["--encrypted-regex", ENCRYPTED_REGEX])
        .arg(&path)
        .env("SOPS_AGE_KEY", key.expose_secret())
        .env("SOPS_AGE_RECIPIENTS", &public_key)
        .env_remove("SOPS_AGE_KEY_FILE")
        .status()
        .unwrap_or_else(|e| {
            eprintln!("Failed to run sops: {}", e);
            process::exit(1);
        });

    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }
}

fn cmd_config_encrypt() {
    let path = config::config_path();
    if !path.exists() {
        eprintln!("Config not found: {}", path.display());
        process::exit(1);
    }

    let contents = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("Failed to read config: {}", e);
        process::exit(1);
    });

    let probe: serde_yaml::Value = serde_yaml::from_str(&contents).unwrap_or_else(|e| {
        eprintln!("Invalid YAML:{}", e);
        process::exit(1);
    });

    if config::is_encrypted(&probe) {
        println!("Config is already encrypted.");
        return;
    }

    let private_key = key::get_private_key().unwrap_or_else(|e| {
        eprintln!("Error retrieving key: {}", e);
        process::exit(1);
    });

    let public_key = key::public_key_from_private(&private_key).unwrap_or_else(|e| {
        eprintln!("Error deriving public key: {}", e);
        process::exit(1);
    });

    let status = process::Command::new("sops")
        .args(["--encrypt", "--encrypted-regex", ENCRYPTED_REGEX, "-i"])
        .arg(&path)
        .env("SOPS_AGE_RECIPIENTS", &public_key)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("Failed to run sops: {}", e);
            process::exit(1);
        });

    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }

    println!("Encrypted: {}", path.display());
}

fn cmd_config_decrypt() {
    let path = config::config_path();
    if !path.exists() {
        eprintln!("Config not found: {}", path.display());
        process::exit(1);
    }

    let contents = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("Failed to read config: {}", e);
        process::exit(1);
    });

    let probe: serde_yaml::Value = serde_yaml::from_str(&contents).unwrap_or_else(|e| {
        eprintln!("Invalid YAML:{}", e);
        process::exit(1);
    });

    if !config::is_encrypted(&probe) {
        println!("Config is not encrypted.");
        return;
    }

    let key = key::get_private_key().unwrap_or_else(|e| {
        eprintln!("Error retrieving key: {}", e);
        process::exit(1);
    });

    let status = process::Command::new("sops")
        .args(["--decrypt", "-i"])
        .arg(&path)
        .env("SOPS_AGE_KEY", key.expose_secret())
        .env_remove("SOPS_AGE_KEY_FILE")
        .status()
        .unwrap_or_else(|e| {
            eprintln!("Failed to run sops: {}", e);
            process::exit(1);
        });

    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }

    println!("Decrypted: {}", path.display());
}

fn cmd_config_migrate() {
    let path = config::config_path();
    if !path.exists() {
        eprintln!("Config not found: {}", path.display());
        process::exit(1);
    }

    let contents = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("Failed to read config: {}", e);
        process::exit(1);
    });

    let probe: serde_yaml::Value = serde_yaml::from_str(&contents).unwrap_or_else(|e| {
        eprintln!("Invalid YAML: {}", e);
        process::exit(1);
    });

    if !config::is_encrypted(&probe) {
        eprintln!("Config is not encrypted — nothing to migrate.");
        process::exit(1);
    }

    let key = key::get_private_key().unwrap_or_else(|e| {
        eprintln!("Error retrieving key: {}", e);
        process::exit(1);
    });

    let public_key = key::public_key_from_private(&key).unwrap_or_else(|e| {
        eprintln!("Error deriving public key: {}", e);
        process::exit(1);
    });

    // Decrypt in-place
    let status = process::Command::new("sops")
        .args(["--decrypt", "-i"])
        .arg(&path)
        .env("SOPS_AGE_KEY", key.expose_secret())
        .env_remove("SOPS_AGE_KEY_FILE")
        .status()
        .unwrap_or_else(|e| {
            eprintln!("Failed to run sops: {}", e);
            process::exit(1);
        });
    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }

    // Re-encrypt with current ENCRYPTED_REGEX
    let status = process::Command::new("sops")
        .args(["--encrypt", "--encrypted-regex", ENCRYPTED_REGEX, "-i"])
        .arg(&path)
        .env("SOPS_AGE_RECIPIENTS", &public_key)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("Failed to run sops: {}", e);
            process::exit(1);
        });
    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }

    println!("Migrated: {}", path.display());
    println!("Encrypted fields: {}", ENCRYPTED_REGEX);
}

fn cmd_config_show() {
    let path = config::config_path();
    if !path.exists() {
        eprintln!("Config not found: {}", path.display());
        process::exit(1);
    }

    let contents = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("Failed to read config: {}", e);
        process::exit(1);
    });

    let probe: serde_yaml::Value = serde_yaml::from_str(&contents).unwrap_or_else(|e| {
        eprintln!("Invalid YAML:{}", e);
        process::exit(1);
    });

    if config::is_encrypted(&probe) {
        let decrypted = config::decrypt_config(&path);
        print!("{}", decrypted);
    } else {
        print!("{}", contents);
    }
}

fn cmd_config_template(app: &str) {
    let template = match app {
        "psql" => "\
psql:
  conn:
    host: localhost
    port: 5432
    database: mydb
    user: readonly
    password: changeme
    tls: false
    writable_tables: []
",
        "dbr" => "\
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
",
        _ => {
            eprintln!("Unknown app: {}", app);
            eprintln!("Known apps: psql, dbr");
            process::exit(1);
        }
    };

    println!("# Add to your config via `toolkit config edit`:");
    print!("{}", template);
}

