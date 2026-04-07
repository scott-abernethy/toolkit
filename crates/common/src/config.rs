use crate::exit_with_error;
use serde::de::DeserializeOwned;
use std::path::PathBuf;

/// Resolve the config file path.
/// Checks `TOOLKIT_CONFIG` env var first, then falls back to
/// `~/.config/toolkit/config.toml`.
pub fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("TOOLKIT_CONFIG") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| exit_with_error("HOME not set"));
    PathBuf::from(home)
        .join(".config")
        .join("toolkit")
        .join("config.toml")
}

/// Load a named section from the shared config file and deserialize it into `T`.
///
/// Each tool defines its own config struct and calls:
///   `common::load_section::<MyConfig>("mytool")`
pub fn load_section<T: DeserializeOwned>(section: &str) -> T {
    let path = config_path();

    let contents = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        exit_with_error(format!("Failed to read config {}: {}", path.display(), e))
    });

    let full: toml::Value = toml::from_str(&contents)
        .unwrap_or_else(|e| exit_with_error(format!("Invalid config: {}", e)));

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
        std::env::set_var("TOOLKIT_CONFIG", "/tmp/test-toolkit.toml");
        assert_eq!(config_path(), PathBuf::from("/tmp/test-toolkit.toml"));
        std::env::remove_var("TOOLKIT_CONFIG");
    }

    #[test]
    fn test_config_path_default() {
        std::env::remove_var("TOOLKIT_CONFIG");
        let path = config_path();
        assert!(path.ends_with(".config/toolkit/config.toml"));
    }
}
