//! Réglages d'un run Claude : settings interpolés, variables, délai, MCP, persona.

use super::temp::TempFiles;
use super::{mcp, persona};
use crate::agents;
use crate::config;
use crate::store::{read_json, write_json_atomic};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Default)]
pub struct AgentSetup {
    pub env: Map<String, Value>,
    pub timeout: Option<Duration>,
    pub settings_file: Option<PathBuf>,
    pub mcp_file: Option<PathBuf>,
    pub strict_mcp: bool,
    pub persona: Option<String>,
}

/// Réglages d'un agent nommé (`settings_<nom>.json`, `.mcp.<nom>.json`, persona).
pub fn for_agent(
    ws: &Path,
    name: &str,
    run_id: &str,
    temps: &mut TempFiles,
) -> Result<AgentSetup, String> {
    let raw = read_json(&agents::claude_settings_path(ws, name))
        .ok_or_else(|| format!("INVALID_AGENT: settings_{name}.json absent ou invalide."))?;
    let settings = config::interpolate_json(&raw);
    let env = settings.get("env").and_then(Value::as_object).cloned().unwrap_or_default();

    let (mcp_file, strict_mcp) = mcp::resolve(ws, name, &settings, run_id, temps);
    Ok(AgentSetup {
        timeout: timeout_from_env(&env),
        settings_file: write_cli_settings(ws, name, run_id, &settings, temps),
        env,
        mcp_file,
        strict_mcp,
        persona: persona::load(ws, name),
    })
}

/// Sans agent : `.claude/settings.json` et `.mcp.json` du workspace s'ils existent.
pub fn for_anonymous(ws: &Path) -> AgentSetup {
    let settings = ws.join(".claude").join("settings.json");
    let global_mcp = agents::global_mcp_path(ws);
    AgentSetup {
        settings_file: settings.is_file().then_some(settings),
        mcp_file: global_mcp.is_file().then_some(global_mcp),
        ..Default::default()
    }
}

/// `AGENT_TIMEOUT_MS` (ou `API_TIMEOUT_MS`) de l'agent.
fn timeout_from_env(env: &Map<String, Value>) -> Option<Duration> {
    ["AGENT_TIMEOUT_MS", "API_TIMEOUT_MS"]
        .iter()
        .find_map(|k| env.get(*k).and_then(Value::as_str))
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|ms| *ms > 0)
        .map(Duration::from_millis)
}

/// Settings passés au CLI : SANS `env` — variables et jetons voyagent par
/// l'environnement du process, jamais par un fichier temporaire sur disque.
fn write_cli_settings(
    ws: &Path,
    name: &str,
    run_id: &str,
    settings: &Value,
    temps: &mut TempFiles,
) -> Option<PathBuf> {
    let mut for_cli = settings.clone();
    for_cli.as_object_mut()?.remove("env");
    let path = ws.join(".claude").join(format!("settings_{name}_run_{run_id}.json"));
    write_json_atomic(&path, &for_cli).ok()?;
    temps.track(path.clone());
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::timeout_from_env;
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn reads_timeout_from_agent_env() {
        let env = json!({"AGENT_TIMEOUT_MS": "1500"}).as_object().unwrap().clone();
        assert_eq!(timeout_from_env(&env), Some(Duration::from_millis(1500)));
        let none = json!({"AGENT_TIMEOUT_MS": "0"}).as_object().unwrap().clone();
        assert_eq!(timeout_from_env(&none), None);
    }
}
