//! Inventaire des agents Hermes : `profiles/*/config.yaml`.

use super::info::AgentInfo;
use crate::agents::Runner;
use crate::agents::hermes::{yaml_mcp_servers, yaml_model};
use crate::agents::name::validate_name;
use crate::config;

pub fn scan() -> Vec<AgentInfo> {
    let Ok(entries) = std::fs::read_dir(config::native_hermes_home().join("profiles")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let dir = entry.path();
            if !dir.join("config.yaml").is_file() || validate_name(&name).is_err() {
                return None;
            }
            let yaml = std::fs::read_to_string(dir.join("config.yaml")).unwrap_or_default();
            let soul = std::fs::metadata(dir.join("SOUL.md"));
            Some(AgentInfo {
                runner: Runner::Hermes,
                model: yaml_model(&yaml).unwrap_or_else(|| "unknown".into()),
                mcp_servers: yaml_mcp_servers(&yaml),
                prompt_bytes: soul.as_ref().map(|m| m.len()).unwrap_or(0),
                missing_config: soul.is_err(),
                name,
            })
        })
        .collect()
}
