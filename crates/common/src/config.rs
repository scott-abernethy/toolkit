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
    let key = crate::key::get_private_key().unwrap_or_else(|e| {
        exit_with_error(format!("Failed to retrieve decryption key: {}", e))
    });
    decrypt_config_with_key(path, &key)
}

/// Decrypt a sops-encrypted config file using the provided age private key.
pub fn decrypt_config_with_key(path: &std::path::Path, key: &secrecy::SecretString) -> String {
    use secrecy::ExposeSecret;

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
    use serde::Deserialize;

    #[test]
    fn test_config_path_env_override() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("TOOLKIT_CONFIG", "/tmp/test-toolkit.yaml");
        let result = config_path();
        std::env::remove_var("TOOLKIT_CONFIG");
        assert_eq!(result, PathBuf::from("/tmp/test-toolkit.yaml"));
    }

    #[test]
    fn test_config_path_default() {
        let _guard = ENV_MUTEX.lock().unwrap();
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

    const ENCRYPTED_REGEX: &str = "^(host|database|user|password|token)$";

    /// Mutex to serialise tests that read/write process-global env vars.
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn sops_encrypt(path: &std::path::Path, public_key: &str) {
        let status = std::process::Command::new("sops")
            .args(["--encrypt", "--encrypted-regex", ENCRYPTED_REGEX, "-i"])
            .arg(path)
            .env("SOPS_AGE_RECIPIENTS", public_key)
            .status()
            .expect("failed to run sops");
        assert!(status.success(), "sops encrypt failed");
    }

    #[test]
    fn test_decrypt_config_with_key() {
        let (private_key, public_key) = crate::key::generate_keypair();
        let file = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
        std::fs::write(file.path(), "psql:\n  local:\n    host: db.example.com\n    port: 5432\n    password: secret\n").unwrap();

        sops_encrypt(file.path(), &public_key);

        let contents = std::fs::read_to_string(file.path()).unwrap();
        let probe: serde_yaml::Value = serde_yaml::from_str(&contents).unwrap();
        assert!(is_encrypted(&probe), "file should be encrypted");
        // port is not in encrypted_regex — it should be plaintext in the file
        assert_eq!(probe["psql"]["local"]["port"], serde_yaml::Value::Number(5432.into()));

        let decrypted = decrypt_config_with_key(file.path(), &private_key);
        let val: serde_yaml::Value = serde_yaml::from_str(&decrypted).unwrap();
        assert_eq!(val["psql"]["local"]["host"].as_str(), Some("db.example.com"));
        assert_eq!(val["psql"]["local"]["port"].as_i64(), Some(5432));
        assert_eq!(val["psql"]["local"]["password"].as_str(), Some("secret"));
    }

    #[test]
    fn test_load_section_plaintext() {
        #[derive(Deserialize)]
        struct TestConn {
            host: String,
            port: u16,
        }

        let _guard = ENV_MUTEX.lock().unwrap();
        let file = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
        std::fs::write(file.path(), "psql:\n  local:\n    host: localhost\n    port: 5432\n").unwrap();
        std::env::set_var("TOOLKIT_CONFIG", file.path());

        let configs = load_section::<std::collections::HashMap<String, TestConn>>("psql");
        std::env::remove_var("TOOLKIT_CONFIG");

        let conn = configs.get("local").expect("local connection not found");
        assert_eq!(conn.host, "localhost");
        assert_eq!(conn.port, 5432);
    }

    /// Tests the full encrypted config path: encrypt with sops, decrypt with
    /// decrypt_config_with_key, then deserialize — mirrors what load_section does
    /// at runtime, but with a throwaway key instead of the keychain.
    #[test]
    fn test_decrypt_and_deserialize_encrypted_config() {
        #[derive(Deserialize)]
        struct TestConn {
            host: String,
            port: u16,
            password: String,
        }

        let (private_key, public_key) = crate::key::generate_keypair();
        let file = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
        std::fs::write(
            file.path(),
            "psql:\n  local:\n    host: db.example.com\n    port: 5432\n    password: secret\n",
        )
        .unwrap();

        sops_encrypt(file.path(), &public_key);

        let decrypted = decrypt_config_with_key(file.path(), &private_key);
        let full: serde_yaml::Value = serde_yaml::from_str(&decrypted).unwrap();
        let section = full.get("psql").expect("missing psql section");
        let configs: std::collections::HashMap<String, TestConn> =
            serde_yaml::from_value(section.clone()).unwrap();

        let conn = configs.get("local").unwrap();
        assert_eq!(conn.host, "db.example.com");
        assert_eq!(conn.port, 5432);
        assert_eq!(conn.password, "secret");
    }
}
