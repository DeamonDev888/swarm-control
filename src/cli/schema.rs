//! `schema <outil>` : le schéma d'entrée (`inputSchema`) d'un outil, lu sur un serveur qui tourne.
//!
//! Cible : `--port N` (`http://127.0.0.1:N/mcp`), sinon le serveur de l'agent (`--agent`, ou
//! l'exécutable renommé d'après l'agent), sinon une instance d'administration vivante. Rien de
//! vivant → erreur claire invitant à démarrer un serveur ou à passer `--port`.

use super::args::Options;
use super::output;
use super::target;
use crate::peers::{self, CallError};
use serde_json::Value;
use std::time::Duration;

/// `tools/list` est une requête locale et instantanée : inutile d'attendre plus.
const WAIT: Duration = Duration::from_secs(10);

pub fn run(options: &Options, agent: Option<&str>, tool: &str) -> Result<(), String> {
    let url = target_url(options, agent)?;
    let list = peers::list_tools(&url, WAIT).map_err(|e| match e {
        CallError::Unreachable(why) => format!("serveur {url} injoignable ({why})."),
        CallError::Tool(message) | CallError::Protocol(message) => format!("{url}: {message}"),
    })?;
    let schema = schema_of(&list, tool).map_err(|e| format!("{url}: {e}"))?;
    output::json(&schema);
    Ok(())
}

fn target_url(options: &Options, agent: Option<&str>) -> Result<String, String> {
    if let Some(port) = options.port {
        return Ok(format!("http://127.0.0.1:{port}/mcp"));
    }
    if let Some(agent) = agent {
        let peer = target::peer(agent).ok_or_else(|| {
            format!(
                "agent '{agent}' non annoncé : démarre son serveur (`{agent} --headless`), \
                 ou passe --port N."
            )
        })?;
        return Ok(peer.url);
    }
    match peers::instances().into_iter().find(|instance| instance.alive) {
        Some(instance) => Ok(instance.url),
        None => Err("aucune instance vivante : démarre `swarm-control`, ou passe --port N.".into()),
    }
}

/// L'`inputSchema` de `tool` dans une réponse `tools/list`.
fn schema_of(list: &Value, tool: &str) -> Result<Value, String> {
    let tools = list
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| "réponse tools/list sans liste d'outils.".to_string())?;
    match tools.iter().find(|announced| announced["name"].as_str() == Some(tool)) {
        Some(found) => Ok(found.get("inputSchema").cloned().unwrap_or(Value::Null)),
        None => {
            let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
            let names = if names.is_empty() {
                "aucun outil annoncé".to_string()
            } else {
                names.join(", ")
            };
            Err(format!("outil '{tool}' inconnu (outils : {names})."))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::schema_of;
    use serde_json::json;

    fn list() -> serde_json::Value {
        json!({"tools": [
            {"name": "run_agent", "inputSchema": {"type": "object", "properties": {"prompt": {}}}},
            {"name": "ask", "inputSchema": {"type": "object"}},
            {"name": "sans_schema"}
        ]})
    }

    #[test]
    fn finds_the_tool_schema() {
        assert_eq!(schema_of(&list(), "ask").unwrap(), json!({"type": "object"}));
        assert_eq!(schema_of(&list(), "run_agent").unwrap()["properties"]["prompt"], json!({}));
        // Un outil sans schéma rend `null` plutôt qu'une erreur.
        assert_eq!(schema_of(&list(), "sans_schema").unwrap(), json!(null));
    }

    #[test]
    fn unknown_tools_list_what_exists() {
        let message = schema_of(&list(), "stop").unwrap_err();
        assert!(message.contains("outil 'stop' inconnu"), "{message}");
        assert!(message.contains("run_agent, ask, sans_schema"), "{message}");
        assert!(schema_of(&json!({}), "ask").is_err());
        let empty = schema_of(&json!({"tools": []}), "ask").unwrap_err();
        assert!(empty.contains("aucun outil annoncé"), "{empty}");
    }
}
