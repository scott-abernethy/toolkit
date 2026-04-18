use crate::exit_with_error;
use serde::de::DeserializeOwned;
use std::path::PathBuf;

/// Resolve the config file path.
/// Checks `TOOLKIT_CONFIG` env var first, then falls back to
/// `~/.config/toolkit/config.yaml`.
pub fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("TOOLKIT_CONFIG") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| exit_with_error("HOME not set"));
    PathBuf::from(home)
        .join(".config")
        .join("toolkit")
        .join("config.yaml")
}

/// Return true if the parsed YAML contains sops metadata (i.e. the file is encrypted).
pub fn is_encrypted(value: &serde_yaml::Value) -> bool {
    value
        .get("sops")
        .and_then(|s| s.get("version"))
        .is_some()
}

/// Decrypt a sops-encrypted config file using the stored age private key.
/// The key is passed only in the subprocess environment — it never touches disk.
pub fn decrypt_config(path: &std::path::Path) -> String {
    use secrecy::ExposeSecret;

    let key = crate::key::get_private_key().unwrap_or_else(|e| {
        exit_with_error(format!("Failed to retrieve decryption key: {}", e))
    });

    let output = std::process::Command::new("sops")
        .args(["--decrypt", "--output-type", "yaml"])
        .arg(path)
        .env("SOPS_AGE_KEY", key.expose_secret())
        // Clear any ambient SOPS_AGE_KEY_FILE to avoid interference
        .env_remove("SOPS_AGE_KEY_FILE")
        .output()
        .unwrap_or_else(|e| {
            exit_with_error(format!(
                "Failed to run sops (is it installed?): {}",
                e
            ))
        });

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        exit_with_error(format!("sops decryption failed: {}", stderr.trim()));
    }

    String::from_utf8(output.stdout)
        .unwrap_or_else(|_| exit_with_error("sops produced non-UTF-8 output"))
}

/// Load a named section from the shared config file and deserialize it into `T`.
///
/// If the config file is sops-encrypted, it is decrypted transparently at runtime
/// using the age private key from the OS keychain (with fallback to key file).
///
/// Each tool defines its own config struct and calls:
///   `common::load_section::<MyConfig>("mytool")`
pub fn load_section<T: DeserializeOwned>(section: &str) -> T {
    let path = config_path();

    let contents = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        exit_with_error(format!("Failed to read config {}: {}", path.display(), e))
    });

    let probe: serde_yaml::Value = serde_yaml::from_str(&contents)
        .unwrap_or_else(|e| exit_with_error(format!("Invalid config: {}", e)));

    let full: serde_yaml::Value = if is_encrypted(&probe) {
        let decrypted = decrypt_config(&path);
        serde_yaml::from_str(&decrypted)
            .unwrap_or_else(|e| exit_with_error(format!("Invalid decrypted config: {}", e)))
    } else {
        probe
    };

    let section_val = full
        .get(section)
        .unwrap_or_else(|| exit_with_error(format!("Missing [{}] section in config", section)));

    T::deserialize(section_val.clone())
        .unwrap_or_else(|e| exit_with_error(format!("Invalid [{}] config: {}", section, e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_path_env_override() {
        std::env::set_var("TOOLKIT_CONFIG", "/tmp/test-toolkit.yaml");
        assert_eq!(config_path(), PathBuf::from("/tmp/test-toolkit.yaml"));
        std::env::remove_var("TOOLKIT_CONFIG");
    }

    #[test]
    fn test_config_path_default() {
        std::env::remove_var("TOOLKIT_CONFIG");
        let path = config_path();
        assert!(path.ends_with(".config/toolkit/config.yaml"));
    }

    #[test]
    fn test_is_encrypted_plaintext() {
        let val: serde_yaml::Value = serde_yaml::from_str("psql:\n  local:\n    host: localhost").unwrap();
        assert!(!is_encrypted(&val));
    }

    #[test]
    fn test_is_encrypted_sops() {
        let val: serde_yaml::Value =
            serde_yaml::from_str("sops:\n  version: \"3.8.0\"\npsql:\n  local:\n    host: ENC[...]").unwrap();
        assert!(is_encrypted(&val));
    }
}
