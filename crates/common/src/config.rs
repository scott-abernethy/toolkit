use crate::error::{Result, ToolkitError};
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::path::PathBuf;

/// Resolve the config file path.
/// Checks `TOOLKIT_CONFIG` env var first, then falls back to
/// `~/.config/toolkit/config.yaml`.
pub fn config_path() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("TOOLKIT_CONFIG") {
        return Ok(PathBuf::from(p));
    }
    let home = std::env::var("HOME").map_err(|_| ToolkitError::config("HOME not set"))?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("toolkit")
        .join("config.yaml"))
}

/// Load a named section from the shared config file and deserialize it into `T`.
///
/// Each tool defines its own config struct and calls:
///   `common::load_section::<MyConfig>("mytool")`
pub fn load_section<T: DeserializeOwned>(section: &str) -> Result<T> {
    let path = config_path()?;

    let contents = std::fs::read_to_string(&path)
        .map_err(|e| ToolkitError::config(format!("config not found or unreadable: {}", e)))?;

    let full: serde_yaml::Value = serde_yaml::from_str(&contents)
        .map_err(|e| ToolkitError::config(format!("Invalid config: {}", e)))?;

    let section_val = full
        .get(section)
        .ok_or_else(|| ToolkitError::config(format!("Missing [{}] section in config", section)))?;

    T::deserialize(section_val.clone())
        .map_err(|e| ToolkitError::config(format!("Invalid [{}] config: {}", section, e)))
}

/// Load a named connection from a config section.
///
/// If `conn` is None and exactly one connection is configured, that one is used.
/// If `conn` is None and multiple connections exist, returns an error listing
/// the available names.
pub fn load_named_section<T: DeserializeOwned>(section: &str, conn: Option<&str>) -> Result<T> {
    load_named_section_with_name(section, conn).map(|(_, v)| v)
}

/// Like `load_named_section`, but also returns the connection name. Used by
/// tools that need to thread the name through to a CLI flag (e.g. `--profile`).
pub fn load_named_section_with_name<T: DeserializeOwned>(
    section: &str,
    conn: Option<&str>,
) -> Result<(String, T)> {
    let mut configs = load_section::<HashMap<String, T>>(section)?;

    match conn {
        Some(name) => {
            let value = configs.remove(name).ok_or_else(|| {
                ToolkitError::not_found(format!(
                    "Unknown connection '{}'. Available: {}",
                    name,
                    sorted_keys(&configs).join(", ")
                ))
            })?;
            Ok((name.to_string(), value))
        }
        None => {
            if configs.len() == 1 {
                Ok(configs.into_iter().next().unwrap())
            } else {
                Err(ToolkitError::config(format!(
                    "Multiple connections configured, specify --conn. Available: {}",
                    sorted_keys(&configs).join(", ")
                )))
            }
        }
    }
}

fn sorted_keys<T>(map: &HashMap<String, T>) -> Vec<String> {
    let mut keys: Vec<String> = map.keys().cloned().collect();
    keys.sort();
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    /// Mutex to serialise tests that read/write process-global env vars.
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_config_path_env_override() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("TOOLKIT_CONFIG", "/tmp/test-toolkit.yaml");
        let result = config_path().unwrap();
        std::env::remove_var("TOOLKIT_CONFIG");
        assert_eq!(result, PathBuf::from("/tmp/test-toolkit.yaml"));
    }

    #[test]
    fn test_config_path_default() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::remove_var("TOOLKIT_CONFIG");
        let path = config_path().unwrap();
        assert!(path.ends_with(".config/toolkit/config.yaml"));
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
        std::fs::write(
            file.path(),
            "psql:\n  local:\n    host: localhost\n    port: 5432\n",
        )
        .unwrap();
        std::env::set_var("TOOLKIT_CONFIG", file.path());

        let configs = load_section::<std::collections::HashMap<String, TestConn>>("psql").unwrap();
        std::env::remove_var("TOOLKIT_CONFIG");

        let conn = configs.get("local").expect("local connection not found");
        assert_eq!(conn.host, "localhost");
        assert_eq!(conn.port, 5432);
    }

    #[test]
    fn test_load_section_missing_returns_config_error() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("TOOLKIT_CONFIG", "/nonexistent/path/toolkit.yaml");
        let result = load_section::<std::collections::HashMap<String, String>>("psql");
        std::env::remove_var("TOOLKIT_CONFIG");
        match result {
            Err(ToolkitError::Config(_)) => {}
            other => panic!("expected Config error, got {:?}", other),
        }
    }
}
