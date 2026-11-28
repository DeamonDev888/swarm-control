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
//!
//! S'y ajoute le serveur **`memory`** (PLAN §4.6) : le serveur MCP du résident lui-même
//! (`swarm-<agent>`, trouvé par [`crate::peers::discover`]), qui sert `memory_store` /
//! `memory_search` / `memory_forget`. Injecté seulement quand l'agent est vivant dans le
//! registre ; un serveur `memory` déjà déclaré par l'utilisateur garde sa configuration.

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

/// Nom du serveur mémoire injecté dans la config de l'agent.
const MEMORY_SERVER: &str = "memory";

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
    let mut servers = pairs::merged(&persistent, agent);
    if let Some((name, cfg)) = memory_server(agent) {
        servers.entry(name).or_insert(cfg);
    }
    write_json_atomic(&claude_mcp_path(ws, agent), &json!({ "mcpServers": servers })).map_err(io)
}

fn hermes(_ws: &Path, agent: &str) -> Result<(), String> {
    let dir = hermes_profile_dir(agent);
    let current = std::fs::read_to_string(dir.join("config.yaml")).unwrap_or_default();
    let injected = injected(&dir);
    let mut blocks: Vec<(String, Vec<String>)> =
        mcp_blocks(&current).into_iter().filter(|(name, _)| !injected.contains(name)).collect();
    let mut added = Vec::new();
    for (name, cfg) in pairs::servers(agent).into_iter().chain(memory_server(agent)) {
        if blocks.iter().any(|(known, _)| known == &name) {
            continue;
        }
        blocks.push((name.clone(), render_body(&cfg)));
        added.push(name);
    }
    remember(&dir, &added)?;
    write_blocks(&dir, &blocks)
}

fn antigravity(ws: &Path, agent: &str) -> Result<(), String> {
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
    for (name, cfg) in pairs::servers("").into_iter().chain(memory_server(agent)) {
        if servers.contains_key(&name) {
            continue;
        }
        servers.insert(name.clone(), cfg);
        added.push(name);
    }
    remember(&dir, &added)?;
    write_json_atomic(&path, &json!({ "mcpServers": servers })).map_err(io)
}

/// Le serveur `memory` de l'agent : son propre serveur MCP résident, annoncé sous
/// `swarm-<agent>` et vivant. `None` sinon (agent non résident, annonce morte).
fn memory_server(agent: &str) -> Option<(String, Value)> {
    let peer = crate::peers::find(agent).filter(|peer| peer.alive)?;
    Some((MEMORY_SERVER.to_string(), json!({ "url": peer.url })))
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
    use crate::agents::paths::{claude_mcp_path, claude_settings_path};
    use crate::store::read_json;
    use serde_json::json;

    /// Un agent Claude déclaré dans un workspace temporaire, et une annonce `swarm-<agent>`
    /// dans le registre LiteMCP (dossier temporaire partagé, fichier retiré à la fin).
    fn announced(tag: &str, pid: u32) -> (std::path::PathBuf, String, std::path::PathBuf) {
        let ws =
            std::env::temp_dir().join(format!("swarm-refresh-memory-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ws);
        let agent = format!("memtest-{tag}-{}", std::process::id());
        let settings = claude_settings_path(&ws, &agent);
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(&settings, "{}").unwrap();
        let dir = std::env::temp_dir().join("litemcp-registry");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(format!("swarm-{agent}.json"));
        // Dossier partagé par tous les tests : un autre test peut le vider entre nos deux
        // appels. On réessaie une fois après l'avoir recréé.
        let body = format!(
            r#"{{"name":"swarm-{agent}","addr":"127.0.0.1:23099","port":23099,"pid":{pid}}}"#
        );
        if std::fs::write(&file, &body).is_err() {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&file, &body).unwrap();
        }
        (ws, agent, file)
    }

    #[test]
    fn an_unknown_agent_is_left_alone() {
        let ws = std::env::temp_dir().join(format!("swarm-refresh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ws);
        assert!(refresh(&ws, Runner::Claude, "fantome").is_ok());
        assert!(refresh(&ws, Runner::Hermes, "fantome").is_ok());
        assert!(refresh(&ws, Runner::Antigravity, "fantome").is_ok());
        assert!(!ws.exists(), "un agent inexistant ne crée aucun fichier");
    }

    #[test]
    fn a_live_resident_gets_its_own_memory_server_in_its_mcp_config() {
        // Notre propre PID : vivant par définition, sans lire le système.
        let (ws, agent, file) = announced("vivant", std::process::id());
        refresh(&ws, Runner::Claude, &agent).unwrap();
        let doc = read_json(&claude_mcp_path(&ws, &agent)).unwrap();
        assert_eq!(
            doc["mcpServers"]["memory"]["url"],
            json!("http://127.0.0.1:23099/mcp"),
            "le serveur mémoire pointe sur le résident lui-même"
        );
        // L'annonce devient morte (PID impair, jamais allouable sous Windows) : le serveur
        // mémoire sort de la config au passage suivant.
        std::fs::write(
            &file,
            format!(r#"{{"name":"swarm-{agent}","addr":"127.0.0.1:23099","port":23099,"pid":3000000201}}"#),
        )
        .unwrap();
        refresh(&ws, Runner::Claude, &agent).unwrap();
        let doc = read_json(&claude_mcp_path(&ws, &agent)).unwrap();
        assert!(doc["mcpServers"].get("memory").is_none(), "annonce morte : rien d'injecté");
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn a_user_declared_memory_server_is_never_overwritten() {
        let (ws, agent, file) = announced("utilisateur", std::process::id());
        // Un `memory` déclaré par l'utilisateur dans le `.mcp.json`, activé pour l'agent.
        std::fs::write(
            ws.join(".mcp.json"),
            r#"{"mcpServers":{"memory":{"url":"http://127.0.0.1:4000/mcp"}}}"#,
        )
        .unwrap();
        let settings = claude_settings_path(&ws, &agent);
        std::fs::write(&settings, r#"{"enabledMcpjsonServers":["memory"]}"#).unwrap();
        refresh(&ws, Runner::Claude, &agent).unwrap();
        let doc = read_json(&claude_mcp_path(&ws, &agent)).unwrap();
        assert_eq!(doc["mcpServers"]["memory"]["url"], json!("http://127.0.0.1:4000/mcp"));
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir_all(&ws);
    }
}
