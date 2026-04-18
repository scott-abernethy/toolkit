use clap::{Parser, Subcommand};
use common::{config, key};
use secrecy::ExposeSecret;
use std::process;

/// Environment variables set by known AI agent harnesses.
/// If any are present, toolkit refuses to run — agents must not be able to
/// invoke key/config management commands (e.g. `toolkit config show` would
/// defeat the entire encryption scheme).
const AGENT_ENV_VARS: &[&str] = &[
    "CLAUDECODE", // Claude Code (claude.ai/code)
    "OPENCODE",   // opencode (sst/opencode)
];

fn reject_if_agent() {
    for var in AGENT_ENV_VARS {
        if std::env::var(var).is_ok() {
            eprintln!(
                "error: toolkit cannot be run inside an AI agent harness (detected: {}={})",
                var,
                std::env::var(var).unwrap()
            );
            eprintln!("       Run toolkit commands directly in your terminal.");
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
    /// Encrypt a plaintext config.toml in-place (no-op if already encrypted)
    Migrate,
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
}

fn main() {
    reject_if_agent();
    let cli = Cli::parse();
    match cli.command {
        Commands::Init => cmd_init(),
        Commands::Config { cmd } => match cmd {
            ConfigCmd::Edit => cmd_config_edit(),
            ConfigCmd::Encrypt => cmd_config_encrypt(),
            ConfigCmd::Decrypt => cmd_config_decrypt(),
            ConfigCmd::Show => cmd_config_show(),
        },
        Commands::Migrate => cmd_migrate(),
    }
}

fn cmd_init() {
    // Generate keypair
    let (private_key, public_key) = key::generate_keypair();

    // Store in OS keychain
    match key::store_private_key(&private_key) {
        Ok(()) => println!("Stored age private key in OS keychain (service=toolkit, account=age-identity)"),
        Err(e) => {
            eprintln!("Warning: could not store key in keychain: {}", e);
            eprintln!("The key will only be written to the key file.");
        }
    }

    // Write key file for sops CLI / VS Code interop
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
    println!("  toolkit migrate                  encrypt an existing plaintext config.toml");
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

fn cmd_config_edit() {
    let path = config::config_path();
    if !path.exists() {
        eprintln!("Config not found: {}", path.display());
        process::exit(1);
    }

    let key = key::get_private_key().unwrap_or_else(|e| {
        eprintln!("Error retrieving key: {}", e);
        process::exit(1);
    });

    let status = process::Command::new("sops")
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

    let probe: toml::Value = toml::from_str(&contents).unwrap_or_else(|e| {
        eprintln!("Invalid TOML: {}", e);
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
        .args(["--encrypt", "--age", &public_key, "-i"])
        .arg(&path)
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

    let probe: toml::Value = toml::from_str(&contents).unwrap_or_else(|e| {
        eprintln!("Invalid TOML: {}", e);
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

    let probe: toml::Value = toml::from_str(&contents).unwrap_or_else(|e| {
        eprintln!("Invalid TOML: {}", e);
        process::exit(1);
    });

    if config::is_encrypted(&probe) {
        let decrypted = config::decrypt_config(&path);
        print!("{}", decrypted);
    } else {
        print!("{}", contents);
    }
}

fn cmd_migrate() {
    let path = config::config_path();
    if !path.exists() {
        eprintln!("Config not found: {}", path.display());
        process::exit(1);
    }

    let contents = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("Failed to read config: {}", e);
        process::exit(1);
    });

    let probe: toml::Value = toml::from_str(&contents).unwrap_or_else(|e| {
        eprintln!("Invalid TOML: {}", e);
        process::exit(1);
    });

    if config::is_encrypted(&probe) {
        println!("Config is already encrypted — nothing to do.");
        return;
    }

    // Warn about what will be encrypted
    println!("Encrypting {} in-place.", path.display());
    println!("All values will be encrypted. Connection names and structure remain readable.");

    let private_key = key::get_private_key().unwrap_or_else(|e| {
        eprintln!("Error retrieving key: {}", e);
        eprintln!("Run `toolkit init` first to generate and store an age key.");
        process::exit(1);
    });

    let public_key = key::public_key_from_private(&private_key).unwrap_or_else(|e| {
        eprintln!("Error deriving public key: {}", e);
        process::exit(1);
    });

    let status = process::Command::new("sops")
        .args(["--encrypt", "--age", &public_key, "-i"])
        .arg(&path)
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
