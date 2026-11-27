//! Configuration MCP d'un agent : ses serveurs persistants + les pairs vivants.
//!
//! Réécrite au démarrage de la résidence et au début de chaque tour : un pair
//! démarré plus tard entre dans la config au tour suivant, un pair arrêté en
//! sort. Chaque runner garde son format :
//!
//! - Claude : `.mcp.<agent>.json` (`mcpServers`), à partir de
//!   `enabledMcpjsonServers` (à défaut, tous les serveurs du `.mcp.json`) ;
//! - Hermes : section `mcp_servers:` du `config.yaml` du profil ;
//! - Antigravity : `.agents/mcp_config.json` du workspace.
//!
//! Un fichier de trace par emplacement retient les serveurs injectés au passage
//! précédent : un serveur de l'utilisateur n'est jamais écrasé, et un pair
//! arrêté n'est pas pris pour un serveur persistant.

use super::Runner;
use super::files::io;
use super::hermes::mcp::{render_body, write_blocks};
use super::hermes::yaml::mcp_blocks;
use super::mcp::global_mcp_servers;
use super::paths::{claude_mcp_path, claude_settings_path, hermes_profile_dir};
use crate::peers::pairs;
use crate::store::{read_json, write_json_atomic};
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::path::Path;

/// Trace des serveurs injectés au passage précédent.
const INJECTED: &str = ".swarm-peers.json";

/// Réécrit la config MCP de `agent` avec ses serveurs persistants + les pairs
/// vivants. Un agent inexistant est laissé tranquille.
pub fn refresh(ws: &Path, runner: Runner, agent: &str) -> Result<(), String> {
    if !super::exists_for(ws, agent, runner) {
        return Ok(());
    }
    match runner {
        Runner::Claude => claude(ws, agent),
        Runner::Hermes => hermes(ws, agent),
        Runner::Antigravity => antigravity(ws, agent),
    }
}

fn claude(ws: &Path, agent: &str) -> Result<(), String> {
    let settings = read_json(&claude_settings_path(ws, agent)).unwrap_or(Value::Null);
    let global = global_mcp_servers(ws);
    let persistent: Map<String, Value> =
        match settings.get("enabledMcpjsonServers").and_then(Value::as_array) {
            Some(names) => names
                .iter()
                .filter_map(Value::as_str)
                .filter_map(|name| global.get(name).map(|cfg| (name.to_string(), cfg.clone())))
                .collect(),
            None => global,
        };
    let servers = pairs::merged(&persistent, agent);
    write_json_atomic(&claude_mcp_path(ws, agent), &json!({ "mcpServers": servers })).map_err(io)
}

fn hermes(_ws: &Path, agent: &str) -> Result<(), String> {
    let dir = hermes_profile_dir(agent);
    let current = std::fs::read_to_string(dir.join("config.yaml")).unwrap_or_default();
    let injected = injected(&dir);
    let mut blocks: Vec<(String, Vec<String>)> =
        mcp_blocks(&current).into_iter().filter(|(name, _)| !injected.contains(name)).collect();
    let mut added = Vec::new();
    for (name, cfg) in pairs::servers(agent) {
        if blocks.iter().any(|(known, _)| known == &name) {
            continue;
        }
        blocks.push((name.clone(), render_body(&cfg)));
        added.push(name);
    }
    remember(&dir, &added)?;
    write_blocks(&dir, &blocks)
}

fn antigravity(ws: &Path, _agent: &str) -> Result<(), String> {
    let dir = ws.join(".agents");
    let path = dir.join("mcp_config.json");
    let injected = injected(&dir);
    let mut servers: Map<String, Value> = read_json(&path)
        .and_then(|doc| doc.get("mcpServers").and_then(Value::as_object).cloned())
        .unwrap_or_default();
    servers.retain(|name, _| !injected.contains(name));
    let mut added = Vec::new();
    // Fichier partagé par les agents du workspace : on n'exclut personne, sinon le
    // dernier rafraîchissement retirerait son propre serveur aux autres.
    for (name, cfg) in pairs::servers("") {
        if servers.contains_key(&name) {
            continue;
        }
        servers.insert(name.clone(), cfg);
        added.push(name);
    }
    remember(&dir, &added)?;
    write_json_atomic(&path, &json!({ "mcpServers": servers })).map_err(io)
}

/// Noms injectés au passage précédent dans ce dossier.
fn injected(dir: &Path) -> HashSet<String> {
    read_json(&dir.join(INJECTED))
        .and_then(|doc| doc.as_array().cloned())
        .map(|names| names.iter().filter_map(Value::as_str).map(String::from).collect())
        .unwrap_or_default()
}

fn remember(dir: &Path, names: &[String]) -> Result<(), String> {
    write_json_atomic(&dir.join(INJECTED), &json!(names)).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::{Runner, refresh};

    #[test]
    fn an_unknown_agent_is_left_alone() {
        let ws = std::env::temp_dir().join(format!("swarm-refresh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ws);
        assert!(refresh(&ws, Runner::Claude, "fantome").is_ok());
        assert!(refresh(&ws, Runner::Hermes, "fantome").is_ok());
        assert!(refresh(&ws, Runner::Antigravity, "fantome").is_ok());
        assert!(!ws.exists(), "un agent inexistant ne crée aucun fichier");
    }
}
