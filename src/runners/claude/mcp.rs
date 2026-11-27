//! Fichier MCP passé à Claude pour un agent nommé.

use super::temp::TempFiles;
use crate::agents;
use crate::store::{read_json, write_json_atomic};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// `(fichier, strict)` : le fichier de l'agent, sinon un extrait filtré du
/// `.mcp.json` global (`enabledMcpjsonServers`), sinon le global. `strict`
/// demande `--strict-mcp-config` : l'agent ne voit que SES serveurs.
pub fn resolve(
    ws: &Path,
    name: &str,
    settings: &Value,
    run_id: &str,
    temps: &mut TempFiles,
) -> (Option<PathBuf>, bool) {
    let own = agents::claude_mcp_path(ws, name);
    if own.is_file() {
        return (Some(own), true);
    }
    if let Some(filtered) = filtered_global(ws, name, settings, run_id, temps) {
        return (Some(filtered), true);
    }
    let global = agents::global_mcp_path(ws);
    (global.is_file().then_some(global), false)
}

fn filtered_global(
    ws: &Path,
    name: &str,
    settings: &Value,
    run_id: &str,
    temps: &mut TempFiles,
) -> Option<PathBuf> {
    if settings.get("enableAllProjectMcpServers") != Some(&Value::Bool(false)) {
        return None;
    }
    let enabled = settings.get("enabledMcpjsonServers")?.as_array()?;
    let all = read_json(&agents::global_mcp_path(ws))
        .and_then(|g| g.get("mcpServers").and_then(Value::as_object).cloned())
        .unwrap_or_default();
    let picked: Map<String, Value> = enabled
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|s| all.get(s).map(|c| (s.to_string(), c.clone())))
        .collect();
    let path = ws.join(".claude").join(format!("mcp_{name}_run_{run_id}.json"));
    write_json_atomic(&path, &json!({ "mcpServers": picked })).ok()?;
    temps.track(path.clone());
    Some(path)
}
