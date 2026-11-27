//! Inventaire des agents Antigravity : `.antigravity/agents/*.md`.

use super::info::AgentInfo;
use crate::agents::Runner;
use crate::agents::name::validate_name;
use crate::agents::paths::{
    antigravity_agents_dir, antigravity_profile_prompt_path, antigravity_settings_path,
    profiles_root,
};
use crate::config;
use crate::store::read_json;
use serde_json::Value;
use std::path::Path;

pub fn scan(ws: &Path) -> Vec<AgentInfo> {
    let mut out = scan_legacy(ws);
    out.extend(scan_profiles(ws));
    out
}

/// Agents sur l'emplacement historique partagé : `.antigravity/agents/*.md`.
fn scan_legacy(ws: &Path) -> Vec<AgentInfo> {
    let Ok(entries) = std::fs::read_dir(antigravity_agents_dir(ws)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let file = entry.file_name().to_string_lossy().into_owned();
            let name = file.strip_suffix(".md")?.to_string();
            if validate_name(&name).is_err() {
                return None;
            }
            let prompt_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            Some(describe(ws, name, prompt_bytes))
        })
        .collect()
}

/// Agents en profil dédié : `agents/<nom>/.antigravity/agents/<nom>.md`.
/// Le nom vient du dossier du profil ; sans prompt dédié, ce n'est pas un
/// agent Antigravity (restes de suppression, profil d'un autre runner).
fn scan_profiles(ws: &Path) -> Vec<AgentInfo> {
    let Ok(entries) = std::fs::read_dir(profiles_root(ws)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            if !entry.file_type().ok()?.is_dir() {
                return None;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if validate_name(&name).is_err() {
                return None;
            }
            let prompt = antigravity_profile_prompt_path(ws, &name);
            let prompt_bytes = std::fs::metadata(&prompt).ok()?.len();
            Some(describe(ws, name, prompt_bytes))
        })
        .collect()
}

fn describe(ws: &Path, name: String, prompt_bytes: u64) -> AgentInfo {
    let settings = read_json(&antigravity_settings_path(ws, &name));
    let model = settings
        .as_ref()
        .map(config::interpolate_json)
        .and_then(|s| {
            s.get("model")
                .or_else(|| s.pointer("/env/AGY_MODEL"))
                .or_else(|| s.pointer("/env/ANTIGRAVITY_MODEL"))
                .and_then(Value::as_str)
                .filter(|m| !m.is_empty())
                .map(String::from)
        })
        .unwrap_or_else(|| "default".into());
    AgentInfo {
        name,
        runner: Runner::Antigravity,
        model,
        mcp_servers: Vec::new(),
        prompt_bytes,
        missing_config: settings.is_none(),
    }
}
