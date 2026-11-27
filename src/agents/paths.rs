//! Emplacements des fichiers d'un agent.

use crate::config;
use std::path::{Path, PathBuf};

pub fn claude_dir(ws: &Path) -> PathBuf {
    ws.join(".claude")
}

pub fn claude_agents_dir(ws: &Path) -> PathBuf {
    claude_dir(ws).join("agents")
}

pub fn claude_prompt_path(ws: &Path, name: &str) -> PathBuf {
    claude_agents_dir(ws).join(format!("{name}.md"))
}

pub fn claude_skill_path(ws: &Path, name: &str) -> PathBuf {
    claude_agents_dir(ws).join(format!("{name}_skill.md"))
}

pub fn claude_settings_path(ws: &Path, name: &str) -> PathBuf {
    claude_dir(ws).join(format!("settings_{name}.json"))
}

pub fn claude_mcp_path(ws: &Path, name: &str) -> PathBuf {
    claude_dir(ws).join(format!(".mcp.{name}.json"))
}

/// `.mcp.json` du workspace (ou `.mcp.local.json` s'il est seul).
pub fn global_mcp_path(ws: &Path) -> PathBuf {
    let primary = ws.join(".mcp.json");
    if primary.exists() {
        return primary;
    }
    let local = ws.join(".mcp.local.json");
    if local.exists() { local } else { primary }
}

pub fn hermes_profile_dir(name: &str) -> PathBuf {
    config::native_hermes_home().join("profiles").join(name)
}

pub fn antigravity_dir(ws: &Path) -> PathBuf {
    ws.join(".antigravity")
}

pub fn antigravity_agents_dir(ws: &Path) -> PathBuf {
    antigravity_dir(ws).join("agents")
}

pub fn antigravity_prompt_path(ws: &Path, name: &str) -> PathBuf {
    antigravity_agents_dir(ws).join(format!("{name}.md"))
}

pub fn antigravity_settings_path(ws: &Path, name: &str) -> PathBuf {
    antigravity_dir(ws).join(format!("settings_{name}.json"))
}
