//! Inventaire des agents Claude : `.claude/agents/*.md`.

use super::info::AgentInfo;
use crate::agents::Runner;
use crate::agents::name::validate_name;
use crate::agents::paths::{claude_agents_dir, claude_settings_path};
use crate::config;
use crate::store::read_json;
use serde_json::Value;
use std::path::Path;

pub fn scan(ws: &Path) -> Vec<AgentInfo> {
    let Ok(entries) = std::fs::read_dir(claude_agents_dir(ws)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let file = entry.file_name().to_string_lossy().into_owned();
            let name = file.strip_suffix(".md")?.to_string();
            if name.ends_with("_skill") || validate_name(&name).is_err() {
                return None;
            }
            let prompt_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            Some(describe(ws, name, prompt_bytes))
        })
        .collect()
}

fn describe(ws: &Path, name: String, prompt_bytes: u64) -> AgentInfo {
    let settings = read_json(&claude_settings_path(ws, &name));
    let model = settings
        .as_ref()
        .map(config::interpolate_json)
        .and_then(|s| {
            s.pointer("/env/ANTHROPIC_MODEL")
                .or_else(|| s.get("model"))
                .and_then(Value::as_str)
                .map(String::from)
        })
        .unwrap_or_else(|| "settings-default".into());
    let mcp_servers = settings
        .as_ref()
        .and_then(|s| s.get("enabledMcpjsonServers"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    AgentInfo {
        name,
        runner: Runner::Claude,
        model,
        mcp_servers,
        prompt_bytes,
        missing_config: settings.is_none(),
    }
}
