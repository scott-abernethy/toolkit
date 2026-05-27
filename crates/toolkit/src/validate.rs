use crate::init::{
    self, claude_is_configured, claude_settings_path, copilot_guidance_is_configured,
    copilot_instructions_path, hook_scripts_status, opencode_is_configured, opencode_settings_path,
    InitScope,
};
use common::protocol::{Request, PROTOCOL_VERSION};
use common::{client, Result};
use serde_json::json;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct Check {
    name: &'static str,
    level: &'static str,
    ok: bool,
    details: String,
}

impl Check {
    fn error(name: &'static str, ok: bool, details: impl Into<String>) -> Self {
        Self {
            name,
            level: "error",
            ok,
            details: details.into(),
        }
    }

    fn warning(name: &'static str, ok: bool, details: impl Into<String>) -> Self {
        Self {
            name,
            level: "warning",
            ok,
            details: details.into(),
        }
    }
}

pub fn run() -> Result<i32> {
    let home = init::home_dir()?;
    let socket_path =
        std::env::var("TOOLKIT_SOCKET").unwrap_or_else(|_| client::DEFAULT_SOCKET.to_owned());

    let mut checks: Vec<Check> = Vec::new();
    let reachable = std::os::unix::net::UnixStream::connect(&socket_path).is_ok();
    checks.push(Check::error(
        "daemon_reachable",
        reachable,
        format!("socket={socket_path}"),
    ));

    let protocol_ok = if reachable {
        match client::send(&Request::new("meta", None, "version", json!({}))) {
            Ok(resp) => {
                let daemon_version = resp
                    .get("protocol_version")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                matches!(daemon_version, Some(v) if v == PROTOCOL_VERSION)
            }
            Err(_) => false,
        }
    } else {
        false
    };
    checks.push(Check::error(
        "daemon_protocol_compatible",
        protocol_ok,
        format!("cli={PROTOCOL_VERSION}"),
    ));

    let (bash_script_ok, read_script_ok) = hook_scripts_status(&home);
    checks.push(Check::warning(
        "hook_script_bash_guard",
        bash_script_ok,
        "~/.config/toolkit/hooks/bash-guard",
    ));
    checks.push(Check::warning(
        "hook_script_read_guard",
        read_script_ok,
        "~/.config/toolkit/hooks/read-guard",
    ));

    let claude_global = claude_settings_path(InitScope::Global, &home);
    let claude_project = claude_settings_path(InitScope::Project, &home);
    checks.push(Check::warning(
        "claude_global_hook_configured",
        claude_is_configured(&claude_global),
        claude_global.display().to_string(),
    ));
    checks.push(Check::warning(
        "claude_project_hook_configured",
        claude_is_configured(&claude_project),
        claude_project.display().to_string(),
    ));

    let opencode_global = opencode_settings_path(InitScope::Global, &home);
    let opencode_project = opencode_settings_path(InitScope::Project, &home);
    checks.push(Check::warning(
        "opencode_global_policy_configured",
        opencode_is_configured(&opencode_global),
        opencode_global.display().to_string(),
    ));
    checks.push(Check::warning(
        "opencode_project_policy_configured",
        opencode_is_configured(&opencode_project),
        opencode_project.display().to_string(),
    ));

    let copilot_global = copilot_instructions_path(InitScope::Global, &home);
    let copilot_project = copilot_instructions_path(InitScope::Project, &home);
    checks.push(Check::warning(
        "copilot_global_guidance_configured",
        copilot_guidance_is_configured(&copilot_global),
        copilot_global.display().to_string(),
    ));
    checks.push(Check::warning(
        "copilot_project_guidance_configured",
        copilot_guidance_is_configured(&copilot_project),
        copilot_project.display().to_string(),
    ));

    match wrapper_script_status(&home, reachable) {
        Ok((ok, details)) => checks.push(Check::warning("guard_wrapper_scripts", ok, details)),
        Err(e) => checks.push(Check::warning(
            "guard_wrapper_scripts",
            false,
            format!("wrapper check failed: {}", e.message()),
        )),
    }

    let ok = checks.iter().all(|c| c.level != "error" || c.ok);
    let checks_json: Vec<_> = checks
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "level": c.level,
                "ok": c.ok,
                "details": c.details,
            })
        })
        .collect();

    let out = json!({
        "ok": ok,
        "checks": checks_json,
    });
    println!("{out}");
    Ok(if ok { 0 } else { 1 })
}

fn wrapper_script_status(home: &Path, daemon_reachable: bool) -> Result<(bool, String)> {
    if !daemon_reachable {
        return Ok((
            false,
            "daemon unavailable; wrapper inspection skipped".to_string(),
        ));
    }

    let value = client::send(&Request::new("guard", None, "list", json!({})))?;
    let apps = value
        .get("apps")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if apps.is_empty() {
        return Ok((true, "no guard wrappers configured".to_string()));
    }

    let install_path = value
        .get("install_path")
        .and_then(|v| v.as_str())
        .unwrap_or("$HOME/.local/bin");
    let bin_dir = PathBuf::from(install_path.replace("$HOME", &home.to_string_lossy()));

    let mut missing = Vec::new();
    for entry in apps {
        let app = entry.get("app").and_then(|v| v.as_str()).unwrap_or("");
        let conn = entry.get("conn").and_then(|v| v.as_str()).unwrap_or("");
        if app.is_empty() || conn.is_empty() {
            continue;
        }
        let script = bin_dir.join(format!("tk{}-{}", app, conn));
        if !script.exists() || !is_executable(&script) {
            missing.push(script.display().to_string());
        }
    }

    if missing.is_empty() {
        Ok((
            true,
            format!("all wrapper scripts present in {}", bin_dir.display()),
        ))
    } else {
        Ok((
            false,
            format!(
                "{} wrapper script(s) missing or not executable",
                missing.len()
            ),
        ))
    }
}

fn is_executable(path: &Path) -> bool {
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
        path.exists()
    }
}
