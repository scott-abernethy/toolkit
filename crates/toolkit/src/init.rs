use clap::ValueEnum;
use common::{Result, ToolkitError};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

pub const CLAUDE_BASH_GUARD_CMD: &str = "~/.config/toolkit/hooks/bash-guard";
pub const CLAUDE_READ_GUARD_CMD: &str = "~/.config/toolkit/hooks/read-guard";
const CLAUDE_DENY_RULE: &str = "Bash(toolkit:*)";

const COPILOT_MARKER_BEGIN: &str = "<!-- toolkit-init:begin -->";
const COPILOT_MARKER_END: &str = "<!-- toolkit-init:end -->";

const OPENCODE_BASH_RULES: &[(&str, &str)] = &[("*", "ask"), ("toolkit *", "deny")];
const OPENCODE_READ_RULES: &[(&str, &str)] = &[
    ("*", "allow"),
    ("*.env", "deny"),
    ("*.env.*", "deny"),
    ("*.env.example", "allow"),
    ("~/.config/toolkit/**", "deny"),
    ("~/.ssh/**", "deny"),
    ("~/.aws/**", "deny"),
    ("~/.gnupg/**", "deny"),
    ("~/.kube/**", "deny"),
    ("~/.azure/**", "deny"),
    ("~/.config/gcloud/**", "deny"),
    ("~/.config/gh/**", "deny"),
    ("~/.databrickscfg", "deny"),
    ("~/.netrc", "deny"),
    ("~/.npmrc", "deny"),
    ("~/.pypirc", "deny"),
    ("~/.git-credentials", "deny"),
    ("~/.docker/config.json", "deny"),
];
const OPENCODE_EXTERNAL_DIR_RULES: &[(&str, &str)] = &[("*", "ask")];

// Keep the scripts embedded in the binary so `toolkit init` works from an
// installed release, not only from a git checkout.
const BASH_GUARD_SCRIPT: &str = include_str!("../../../hooks/claude-code/bash-guard");
const READ_GUARD_SCRIPT: &str = include_str!("../../../hooks/claude-code/read-guard");

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum InitHarness {
    #[value(name = "claude-code")]
    ClaudeCode,
    Opencode,
    #[value(name = "copilot-cli")]
    CopilotCli,
    All,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum InitScope {
    Global,
    Project,
}

pub fn run(harness: InitHarness, scope: InitScope) -> Result<()> {
    let home = home_dir()?;
    let mut changes: Vec<String> = Vec::new();

    if matches!(harness, InitHarness::ClaudeCode | InitHarness::All) {
        let script_changes = install_hook_scripts(&home)?;
        changes.extend(script_changes);

        let path = claude_settings_path(scope, &home);
        if upsert_claude_settings(&path)? {
            changes.push(format!("Updated {}", path.display()));
        }
    }

    if matches!(harness, InitHarness::Opencode | InitHarness::All) {
        let path = opencode_settings_path(scope, &home);
        if upsert_opencode_permissions(&path)? {
            changes.push(format!("Updated {}", path.display()));
        }
    }

    if matches!(harness, InitHarness::CopilotCli | InitHarness::All) {
        let path = copilot_instructions_path(scope, &home);
        if upsert_copilot_instructions(&path)? {
            changes.push(format!("Updated {}", path.display()));
        }
    }

    if changes.is_empty() {
        println!("No changes needed.");
    } else {
        for line in changes {
            println!("{line}");
        }
    }
    Ok(())
}

pub(crate) fn home_dir() -> Result<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .map_err(|_| ToolkitError::config("cannot resolve home directory: set HOME or USERPROFILE"))
}

pub(crate) fn claude_settings_path(scope: InitScope, home: &Path) -> PathBuf {
    match scope {
        InitScope::Global => home.join(".claude").join("settings.json"),
        InitScope::Project => PathBuf::from(".claude").join("settings.json"),
    }
}

pub(crate) fn opencode_settings_path(scope: InitScope, home: &Path) -> PathBuf {
    match scope {
        InitScope::Global => home.join(".config").join("opencode").join("opencode.json"),
        InitScope::Project => PathBuf::from("opencode.json"),
    }
}

pub(crate) fn copilot_instructions_path(scope: InitScope, home: &Path) -> PathBuf {
    match scope {
        InitScope::Global => home.join(".copilot").join("copilot-instructions.md"),
        InitScope::Project => PathBuf::from(".github").join("copilot-instructions.md"),
    }
}

pub(crate) fn hook_scripts_status(home: &Path) -> (bool, bool) {
    let hooks_dir = home.join(".config").join("toolkit").join("hooks");
    let bash_guard = hooks_dir.join("bash-guard");
    let read_guard = hooks_dir.join("read-guard");
    (
        file_exists_and_executable(&bash_guard),
        file_exists_and_executable(&read_guard),
    )
}

pub(crate) fn claude_is_configured(path: &Path) -> bool {
    let Ok(settings) = read_json(path) else {
        return false;
    };
    if !json_array_contains_string(&settings["permissions"]["deny"], CLAUDE_DENY_RULE) {
        return false;
    }
    let Some(entries) = settings["hooks"]["PreToolUse"].as_array() else {
        return false;
    };
    has_claude_hook(entries, "Bash", CLAUDE_BASH_GUARD_CMD)
        && has_claude_hook(entries, "Read", CLAUDE_READ_GUARD_CMD)
}

pub(crate) fn opencode_is_configured(path: &Path) -> bool {
    let Ok(settings) = read_json(path) else {
        return false;
    };
    let Some(permission) = settings.get("permission").and_then(|v| v.as_object()) else {
        return false;
    };
    let Some(bash) = permission.get("bash").and_then(|v| v.as_object()) else {
        return false;
    };
    let Some(read) = permission.get("read").and_then(|v| v.as_object()) else {
        return false;
    };
    let Some(external_directory) = permission
        .get("external_directory")
        .and_then(|v| v.as_object())
    else {
        return false;
    };

    has_required_rules(bash, OPENCODE_BASH_RULES)
        && has_required_rules(read, OPENCODE_READ_RULES)
        && has_required_rules(external_directory, OPENCODE_EXTERNAL_DIR_RULES)
}

pub(crate) fn copilot_guidance_is_configured(path: &Path) -> bool {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };
    contents.contains(COPILOT_MARKER_BEGIN) && contents.contains(COPILOT_MARKER_END)
}

fn install_hook_scripts(home: &Path) -> Result<Vec<String>> {
    let hooks_dir = home.join(".config").join("toolkit").join("hooks");
    std::fs::create_dir_all(&hooks_dir).map_err(|e| {
        ToolkitError::other(format!(
            "failed to create hook directory {}: {e}",
            hooks_dir.display()
        ))
    })?;

    let mut changes = Vec::new();
    let bash_path = hooks_dir.join("bash-guard");
    if write_file_if_changed(&bash_path, BASH_GUARD_SCRIPT)? {
        ensure_executable(&bash_path)?;
        changes.push(format!("Installed {}", bash_path.display()));
    }

    let read_path = hooks_dir.join("read-guard");
    if write_file_if_changed(&read_path, READ_GUARD_SCRIPT)? {
        ensure_executable(&read_path)?;
        changes.push(format!("Installed {}", read_path.display()));
    }
    Ok(changes)
}

fn upsert_claude_settings(path: &Path) -> Result<bool> {
    let mut settings = read_json(path)?;
    if !settings.is_object() {
        settings = json!({});
    }
    let mut changed = false;

    ensure_path_object(&mut settings, &["permissions"]);
    ensure_path_array(&mut settings, &["permissions", "deny"]);
    {
        let deny = settings["permissions"]["deny"]
            .as_array_mut()
            .expect("deny ensured as array");
        if !deny.iter().any(|v| v.as_str() == Some(CLAUDE_DENY_RULE)) {
            deny.push(json!(CLAUDE_DENY_RULE));
            changed = true;
        }
    }

    ensure_path_object(&mut settings, &["hooks"]);
    ensure_path_array(&mut settings, &["hooks", "PreToolUse"]);
    {
        let pre_tool_use = settings["hooks"]["PreToolUse"]
            .as_array_mut()
            .expect("PreToolUse ensured as array");

        if !has_claude_hook(pre_tool_use, "Bash", CLAUDE_BASH_GUARD_CMD) {
            pre_tool_use.push(claude_hook_entry("Bash", CLAUDE_BASH_GUARD_CMD));
            changed = true;
        }
        if !has_claude_hook(pre_tool_use, "Read", CLAUDE_READ_GUARD_CMD) {
            pre_tool_use.push(claude_hook_entry("Read", CLAUDE_READ_GUARD_CMD));
            changed = true;
        }
    }

    if changed {
        write_json(path, &settings)?;
    }
    Ok(changed)
}

fn upsert_opencode_permissions(path: &Path) -> Result<bool> {
    let mut settings = read_json(path)?;
    if !settings.is_object() {
        settings = json!({});
    }
    let mut changed = false;

    ensure_path_object(&mut settings, &["permission"]);
    ensure_path_object(&mut settings, &["permission", "bash"]);
    ensure_path_object(&mut settings, &["permission", "read"]);
    ensure_path_object(&mut settings, &["permission", "external_directory"]);

    {
        let bash = settings["permission"]["bash"]
            .as_object_mut()
            .expect("bash ensured as object");
        changed |= upsert_required_rules(bash, OPENCODE_BASH_RULES);
    }
    {
        let read = settings["permission"]["read"]
            .as_object_mut()
            .expect("read ensured as object");
        changed |= upsert_required_rules(read, OPENCODE_READ_RULES);
    }
    {
        let external = settings["permission"]["external_directory"]
            .as_object_mut()
            .expect("external_directory ensured as object");
        changed |= upsert_required_rules(external, OPENCODE_EXTERNAL_DIR_RULES);
    }

    if changed {
        write_json(path, &settings)?;
    }
    Ok(changed)
}

fn upsert_copilot_instructions(path: &Path) -> Result<bool> {
    let block = copilot_security_block();
    let mut contents = if path.exists() {
        std::fs::read_to_string(path)
            .map_err(|e| ToolkitError::other(format!("failed to read {}: {e}", path.display())))?
    } else {
        String::new()
    };

    let updated = if let (Some(start), Some(end)) = (
        contents.find(COPILOT_MARKER_BEGIN),
        contents.find(COPILOT_MARKER_END),
    ) {
        if start < end {
            let after_end = end + COPILOT_MARKER_END.len();
            format!("{}{}{}", &contents[..start], block, &contents[after_end..])
        } else {
            append_block(&contents, block)
        }
    } else {
        append_block(&contents, block)
    };

    if updated != contents {
        contents = updated;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ToolkitError::other(format!(
                    "failed to create directory {}: {e}",
                    parent.display()
                ))
            })?;
        }
        std::fs::write(path, contents)
            .map_err(|e| ToolkitError::other(format!("failed to write {}: {e}", path.display())))?;
        return Ok(true);
    }
    Ok(false)
}

fn append_block(existing: &str, block: &str) -> String {
    if existing.trim().is_empty() {
        return block.to_string();
    }
    format!("{}\n\n{}", existing.trim_end(), block)
}

fn copilot_security_block() -> &'static str {
    concat!(
        "<!-- toolkit-init:begin -->\n",
        "## Toolkit security constraints\n\n",
        "Do not run `toolkit config`, `toolkit install`, or `toolkit init` unless the user explicitly asks.\n",
        "Do not read `~/.config/toolkit`, `~/.ssh`, `~/.aws`, `~/.gnupg`, or `.env` files unless the user explicitly asks.\n",
        "<!-- toolkit-init:end -->"
    )
}

fn has_required_rules(map: &Map<String, Value>, required: &[(&str, &str)]) -> bool {
    required
        .iter()
        .all(|(k, v)| map.get(*k).and_then(|x| x.as_str()) == Some(*v))
}

fn upsert_required_rules(map: &mut Map<String, Value>, required: &[(&str, &str)]) -> bool {
    let mut changed = false;
    for (key, value) in required {
        if map.get(*key).and_then(|v| v.as_str()) != Some(*value) {
            map.insert((*key).to_string(), json!(value));
            changed = true;
        }
    }
    changed
}

fn ensure_path_object(value: &mut Value, path: &[&str]) {
    let mut current = value;
    for segment in path {
        if !current.is_object() {
            *current = json!({});
        }
        let map = current.as_object_mut().expect("object ensured");
        current = map
            .entry((*segment).to_string())
            .or_insert_with(|| json!({}));
    }
    if !current.is_object() {
        *current = json!({});
    }
}

fn ensure_path_array(value: &mut Value, path: &[&str]) {
    if path.is_empty() {
        return;
    }
    if path.len() == 1 {
        if !value[path[0]].is_array() {
            value[path[0]] = json!([]);
        }
        return;
    }
    ensure_path_object(value, &path[..path.len() - 1]);
    let key = path[path.len() - 1];
    if !value[path[0]].is_object() {
        value[path[0]] = json!({});
    }
    let mut current = value;
    for segment in &path[..path.len() - 1] {
        current = current
            .get_mut(*segment)
            .expect("segment exists after ensure_path_object");
    }
    if !current[key].is_array() {
        current[key] = json!([]);
    }
}

fn claude_hook_entry(matcher: &str, command: &str) -> Value {
    json!({
        "matcher": matcher,
        "hooks": [{ "type": "command", "command": command }],
    })
}

fn has_claude_hook(entries: &[Value], matcher: &str, command: &str) -> bool {
    entries.iter().any(|entry| {
        entry.get("matcher").and_then(|v| v.as_str()) == Some(matcher)
            && entry
                .get("hooks")
                .and_then(|v| v.as_array())
                .map(|hooks| {
                    hooks
                        .iter()
                        .any(|h| h.get("command").and_then(|v| v.as_str()) == Some(command))
                })
                .unwrap_or(false)
    })
}

fn json_array_contains_string(value: &Value, needle: &str) -> bool {
    value
        .as_array()
        .map(|arr| arr.iter().any(|v| v.as_str() == Some(needle)))
        .unwrap_or(false)
}

fn write_file_if_changed(path: &Path, contents: &str) -> Result<bool> {
    let existing = std::fs::read_to_string(path).ok();
    if existing.as_deref() == Some(contents) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            ToolkitError::other(format!(
                "failed to create directory {}: {e}",
                parent.display()
            ))
        })?;
    }
    std::fs::write(path, contents)
        .map_err(|e| ToolkitError::other(format!("failed to write {}: {e}", path.display())))?;
    Ok(true)
}

fn read_json(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let contents = std::fs::read_to_string(path)
        .map_err(|e| ToolkitError::other(format!("failed to read {}: {e}", path.display())))?;
    serde_json::from_str(&contents)
        .map_err(|e| ToolkitError::config(format!("failed to parse {}: {e}", path.display())))
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            ToolkitError::other(format!(
                "failed to create directory {}: {e}",
                parent.display()
            ))
        })?;
    }
    let rendered = serde_json::to_string_pretty(value)
        .map_err(|e| ToolkitError::other(format!("failed to serialize JSON: {e}")))?;
    std::fs::write(path, rendered)
        .map_err(|e| ToolkitError::other(format!("failed to write {}: {e}", path.display())))
}

fn ensure_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(path, perms)
            .map_err(|e| ToolkitError::other(format!("failed to chmod {}: {e}", path.display())))?;
    }
    Ok(())
}

fn file_exists_and_executable(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            return meta.permissions().mode() & 0o111 != 0;
        }
        false
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_upsert_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{}").unwrap();

        let first = upsert_claude_settings(&path).unwrap();
        let second = upsert_claude_settings(&path).unwrap();
        assert!(first);
        assert!(!second);
        assert!(claude_is_configured(&path));
    }

    #[test]
    fn opencode_upsert_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("opencode.json");
        std::fs::write(&path, "{}").unwrap();

        let first = upsert_opencode_permissions(&path).unwrap();
        let second = upsert_opencode_permissions(&path).unwrap();
        assert!(first);
        assert!(!second);
        assert!(opencode_is_configured(&path));
    }

    #[test]
    fn copilot_upsert_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("copilot-instructions.md");
        std::fs::write(&path, "# Existing").unwrap();

        let first = upsert_copilot_instructions(&path).unwrap();
        let second = upsert_copilot_instructions(&path).unwrap();
        assert!(first);
        assert!(!second);
        assert!(copilot_guidance_is_configured(&path));
    }
}
