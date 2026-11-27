//! `agents` : les agents du workspace, et ceux qui sont résidents.
//!
//! La liste vient du disque (`agents::list` : Claude, Hermes, Antigravity) ; la résidence des
//! annonces LiteMCP (`peers::find`). Une annonce dont le PID est mort est signalée comme telle
//! plutôt que comme vivante.

use super::args::Options;
use super::output;
use crate::agents::{self, AgentInfo};
use crate::peers::{self, Peer};
use serde_json::{Value, json};
use std::path::Path;

pub fn run(options: &Options, ws: &Path) -> Result<(), String> {
    let list = agents::list(ws);
    if options.json {
        let rows: Vec<Value> =
            list.iter().map(|agent| row(agent, peers::find(&agent.name).as_ref())).collect();
        output::json(&Value::Array(rows));
        return Ok(());
    }
    if list.is_empty() {
        println!("aucun agent dans {}.", ws.display());
        return Ok(());
    }
    let rows: Vec<Vec<String>> =
        list.iter().map(|agent| cells(agent, peers::find(&agent.name).as_ref())).collect();
    print!("{}", output::table(&["AGENT", "RUNNER", "MODÈLE", "MCP", "RÉSIDENT", "URL"], &rows));
    Ok(())
}

fn row(agent: &AgentInfo, peer: Option<&Peer>) -> Value {
    let resident = peer.is_some_and(|peer| peer.alive);
    json!({
        "agent": agent.name,
        "runner": agent.runner.as_str(),
        "model": agent.model,
        "mcpServers": agent.mcp_servers,
        "promptBytes": agent.prompt_bytes,
        "missingConfig": agent.missing_config,
        "resident": resident,
        "url": peer.filter(|peer| peer.alive).map(|peer| peer.url.clone()),
    })
}

fn cells(agent: &AgentInfo, peer: Option<&Peer>) -> Vec<String> {
    let (resident, url) = residence(peer);
    vec![
        agent.name.clone(),
        agent.runner.as_str().to_string(),
        label(&agent.model),
        agent.mcp_servers.len().to_string(),
        resident,
        url,
    ]
}

/// Résidence lisible : vivante, annoncée mais morte, ou absente.
fn residence(peer: Option<&Peer>) -> (String, String) {
    match peer {
        Some(peer) if peer.alive => ("oui".into(), peer.url.clone()),
        Some(peer) => (format!("annoncé (pid {} mort)", peer.pid), "-".into()),
        None => ("non".into(), "-".into()),
    }
}

fn label(value: &str) -> String {
    if value.trim().is_empty() { "-".into() } else { value.to_string() }
}

#[cfg(test)]
mod tests {
    use super::{cells, residence};
    use crate::agents::{AgentInfo, Runner};
    use crate::peers::Peer;

    fn agent() -> AgentInfo {
        AgentInfo {
            name: "sniper".into(),
            runner: Runner::Claude,
            model: String::new(),
            mcp_servers: vec!["memory".into(), "db".into()],
            prompt_bytes: 12,
            missing_config: false,
        }
    }

    fn peer(alive: bool) -> Peer {
        Peer {
            name: "swarm-sniper".into(),
            agent: "sniper".into(),
            url: "http://127.0.0.1:23082/mcp".into(),
            pid: 4242,
            alive,
        }
    }

    #[test]
    fn a_living_peer_is_the_resident() {
        let (resident, url) = residence(Some(&peer(true)));
        assert_eq!((resident.as_str(), url.as_str()), ("oui", "http://127.0.0.1:23082/mcp"));
    }

    #[test]
    fn a_dead_announcement_is_not_a_resident() {
        let (resident, url) = residence(Some(&peer(false)));
        assert!(resident.contains("pid 4242 mort"), "{resident}");
        assert_eq!(url, "-");
        assert_eq!(residence(None), ("non".into(), "-".into()));
    }

    #[test]
    fn the_row_shows_every_column() {
        let row = cells(&agent(), None);
        assert_eq!(row, ["sniper", "claude", "-", "2", "non", "-"]);
    }
}
