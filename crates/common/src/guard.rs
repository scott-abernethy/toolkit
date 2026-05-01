use crate::error::{Result, ToolkitError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize)]
pub struct ConnConfig {
    /// Path or name of the CLI command to invoke (e.g. "kubectl", "/usr/local/bin/pup")
    pub command: String,
    /// Environment variables to inject when running the CLI
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Allow rules — at least one must match (unless empty, which allows all)
    #[serde(default)]
    pub allow: Vec<String>,
    /// Deny rules — if any match, the command is rejected (checked first)
    #[serde(default)]
    pub deny: Vec<String>,
}

/// Load a named connection from the given app section of the shared config.
/// If `conn` is None and exactly one connection is configured, that one is used.
pub fn load_config(app: &str, conn: Option<&str>) -> Result<ConnConfig> {
    crate::load_named_section(app, conn)
}

// ---------------------------------------------------------------------------
// Rule engine
// ---------------------------------------------------------------------------

/// Check whether a rule matches the given args.
///
/// A rule is a space-separated list of token groups. Each group can contain
/// `|`-separated alternatives. The rule matches if **every** group has at
/// least one alternative present as an exact token in the args.
///
/// Examples:
///   "get pod|pods"  matches  ["get", "pods", "-o", "json"]
///   "get pod|pods"  fails    ["get", "deployments"]
///   "--as"          matches  ["get", "pods", "--as", "admin"]
fn rule_matches(rule: &str, args: &[&str]) -> bool {
    rule.split_whitespace()
        .all(|group| group.split('|').any(|alt| args.contains(&alt)))
}

/// Evaluate allow/deny rules against args.
///
/// 1. Deny checked first — any match rejects
/// 2. Allow checked second — at least one must match (unless allow is empty)
pub fn check_rules(config: &ConnConfig, args: &[&str]) -> Result<()> {
    for rule in &config.deny {
        if rule_matches(rule, args) {
            return Err(ToolkitError::permission("command denied"));
        }
    }

    if !config.allow.is_empty() && !config.allow.iter().any(|rule| rule_matches(rule, args)) {
        return Err(ToolkitError::permission("command denied"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

/// Run the wrapped CLI with credential injection and raw passthrough.
/// Returns the wrapped CLI's exit code on success.
pub fn run(config: &ConnConfig, args: &[String]) -> Result<i32> {
    let mut cmd = std::process::Command::new(&config.command);

    for (k, v) in &config.env {
        cmd.env(k, v);
    }

    cmd.args(args);

    let status = cmd.status().map_err(|e| {
        let msg = e.to_string().to_lowercase();
        if msg.contains("not found") || msg.contains("no such file") {
            ToolkitError::not_found(format!("command not found: {}", config.command))
        } else if msg.contains("permission denied") {
            ToolkitError::permission(format!("permission denied: {}", config.command))
        } else {
            ToolkitError::cli(format!("failed to run {}: {}", config.command, e))
        }
    })?;

    Ok(status.code().unwrap_or(1))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- rule_matches ---------------------------------------------------------

    #[test]
    fn test_rule_matches_simple_and() {
        assert!(rule_matches("get pods", &["get", "pods", "-o", "json"]));
    }

    #[test]
    fn test_rule_matches_simple_and_fails() {
        assert!(!rule_matches("get pods", &["get", "deployments"]));
    }

    #[test]
    fn test_rule_matches_or_alternatives() {
        assert!(rule_matches("get pod|pods", &["get", "pod"]));
        assert!(rule_matches("get pod|pods", &["get", "pods"]));
    }

    #[test]
    fn test_rule_matches_or_alternatives_fail() {
        assert!(!rule_matches("get pod|pods", &["get", "services"]));
    }

    #[test]
    fn test_rule_matches_single_token() {
        assert!(rule_matches("logs", &["logs", "my-pod"]));
    }

    #[test]
    fn test_rule_matches_single_token_in_deny() {
        assert!(rule_matches("secret|secrets", &["get", "secrets"]));
    }

    #[test]
    fn test_rule_matches_flag_deny() {
        assert!(rule_matches("--as", &["get", "pods", "--as", "admin"]));
    }

    #[test]
    fn test_rule_matches_no_partial_match() {
        assert!(!rule_matches("secrets", &["get", "secrets-manager"]));
    }

    #[test]
    fn test_rule_matches_empty_rule() {
        assert!(rule_matches("", &["get", "pods"]));
    }

    #[test]
    fn test_rule_matches_multi_alternative_groups() {
        assert!(rule_matches(
            "get|list deploy|deployment|deployments",
            &["list", "deployments"]
        ));
        assert!(!rule_matches(
            "get|list deploy|deployment|deployments",
            &["delete", "deployments"]
        ));
    }

    // -- check_rules ----------------------------------------------------------

    #[test]
    fn test_check_rules_allowed() {
        let config = ConnConfig {
            command: "test".into(),
            env: HashMap::new(),
            allow: vec!["get pods".into()],
            deny: vec![],
        };
        assert!(check_rules(&config, &["get", "pods"]).is_ok());
    }

    #[test]
    fn test_check_rules_empty_allow_permits_all() {
        let config = ConnConfig {
            command: "test".into(),
            env: HashMap::new(),
            allow: vec![],
            deny: vec![],
        };
        assert!(check_rules(&config, &["anything", "goes"]).is_ok());
    }

    #[test]
    fn test_check_rules_denied_by_deny() {
        let config = ConnConfig {
            command: "test".into(),
            env: HashMap::new(),
            allow: vec![],
            deny: vec!["secret|secrets".into()],
        };
        match check_rules(&config, &["get", "secrets"]) {
            Err(ToolkitError::Permission(_)) => {}
            other => panic!("expected Permission, got {:?}", other),
        }
    }

    #[test]
    fn test_check_rules_denied_by_no_allow_match() {
        let config = ConnConfig {
            command: "test".into(),
            env: HashMap::new(),
            allow: vec!["get pods".into()],
            deny: vec![],
        };
        match check_rules(&config, &["delete", "pods"]) {
            Err(ToolkitError::Permission(_)) => {}
            other => panic!("expected Permission, got {:?}", other),
        }
    }

    // -- load_config ----------------------------------------------------------

    #[test]
    fn test_load_config_single_connection() {
        let file = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
        std::fs::write(
            file.path(),
            "myapp:\n  only:\n    command: echo\n    env: {}\n    allow: []\n    deny: []\n",
        )
        .unwrap();

        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("TOOLKIT_CONFIG", file.path());
        let config = load_config("myapp", None).unwrap();
        std::env::remove_var("TOOLKIT_CONFIG");

        assert_eq!(config.command, "echo");
    }

    #[test]
    fn test_load_config_named_connection() {
        let file = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
        std::fs::write(
            file.path(),
            "myapp:\n  a:\n    command: alpha\n    env: {}\n    allow: []\n    deny: []\n  b:\n    command: beta\n    env: {}\n    allow: []\n    deny: []\n",
        )
        .unwrap();

        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("TOOLKIT_CONFIG", file.path());
        let config = load_config("myapp", Some("b")).unwrap();
        std::env::remove_var("TOOLKIT_CONFIG");

        assert_eq!(config.command, "beta");
    }

    /// Mutex to serialise tests that read/write process-global env vars.
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
}
