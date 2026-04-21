use common::exit_with_error;
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Command;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ConnConfig {
    /// Path to the CLI binary (e.g. "kubectl", "/usr/local/bin/pup")
    pub binary: String,
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
pub fn load_config(app: &str, conn: Option<&str>) -> ConnConfig {
    let mut configs = common::load_section::<HashMap<String, ConnConfig>>(app);

    match conn {
        Some(name) => configs.remove(name).unwrap_or_else(|| {
            let available = sorted_keys(&configs);
            exit_with_error(format!(
                "Unknown connection '{}'. Available: {}",
                name,
                available.join(", ")
            ))
        }),
        None => {
            if configs.len() == 1 {
                configs.into_values().next().unwrap()
            } else {
                let available = sorted_keys(&configs);
                exit_with_error(format!(
                    "Multiple connections configured, specify --conn. Available: {}",
                    available.join(", ")
                ))
            }
        }
    }
}

fn sorted_keys(map: &HashMap<String, ConnConfig>) -> Vec<String> {
    let mut keys: Vec<String> = map.keys().cloned().collect();
    keys.sort();
    keys
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

/// Evaluate allow/deny rules against args. Exits with error if denied.
///
/// 1. Deny checked first — any match rejects
/// 2. Allow checked second — at least one must match (unless allow is empty)
pub fn check_rules(config: &ConnConfig, args: &[&str]) {
    for rule in &config.deny {
        if rule_matches(rule, args) {
            exit_with_error("command denied");
        }
    }

    if !config.allow.is_empty() && !config.allow.iter().any(|rule| rule_matches(rule, args)) {
        exit_with_error("command denied");
    }
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

/// Run the wrapped CLI with credential injection and raw passthrough.
pub fn run(config: &ConnConfig, args: &[String]) -> ! {
    let mut cmd = Command::new(&config.binary);

    for (k, v) in &config.env {
        cmd.env(k, v);
    }

    cmd.args(args);

    let output = cmd.output().unwrap_or_else(|e| {
        let msg = e.to_string().to_lowercase();
        if msg.contains("not found") || msg.contains("no such file") {
            exit_with_error(format!("binary not found: {}", config.binary))
        } else if msg.contains("permission denied") {
            exit_with_error(format!("permission denied: {}", config.binary))
        } else {
            exit_with_error(format!("failed to run {}: {}", config.binary, e))
        }
    });

    // Raw passthrough: forward stdout and stderr as-is
    use std::io::Write;
    let _ = std::io::stdout().write_all(&output.stdout);
    let _ = std::io::stderr().write_all(&output.stderr);

    let code = output.status.code().unwrap_or(1);
    std::process::exit(code)
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
        // "secrets" should not match "secrets-manager"
        assert!(!rule_matches("secrets", &["get", "secrets-manager"]));
    }

    #[test]
    fn test_rule_matches_empty_rule() {
        // An empty rule has no groups, so all() over empty is true
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
            binary: "test".into(),
            env: HashMap::new(),
            allow: vec!["get pods".into()],
            deny: vec![],
        };
        // Should not exit
        check_rules(&config, &["get", "pods"]);
    }

    #[test]
    fn test_check_rules_empty_allow_permits_all() {
        let config = ConnConfig {
            binary: "test".into(),
            env: HashMap::new(),
            allow: vec![],
            deny: vec![],
        };
        // No allow rules and no deny rules — everything passes
        check_rules(&config, &["anything", "goes"]);
    }

    // -- load_config ----------------------------------------------------------

    #[test]
    fn test_load_config_single_connection() {
        let file = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
        std::fs::write(
            file.path(),
            "myapp:\n  only:\n    binary: echo\n    env: {}\n    allow: []\n    deny: []\n",
        )
        .unwrap();

        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("TOOLKIT_CONFIG", file.path());
        let config = load_config("myapp", None);
        std::env::remove_var("TOOLKIT_CONFIG");

        assert_eq!(config.binary, "echo");
    }

    #[test]
    fn test_load_config_named_connection() {
        let file = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
        std::fs::write(
            file.path(),
            "myapp:\n  a:\n    binary: alpha\n    env: {}\n    allow: []\n    deny: []\n  b:\n    binary: beta\n    env: {}\n    allow: []\n    deny: []\n",
        )
        .unwrap();

        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("TOOLKIT_CONFIG", file.path());
        let config = load_config("myapp", Some("b"));
        std::env::remove_var("TOOLKIT_CONFIG");

        assert_eq!(config.binary, "beta");
    }

    /// Mutex to serialise tests that read/write process-global env vars.
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
}
