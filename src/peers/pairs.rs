//! Pairs automatiques : les autres agents résidents **vivants** du registre.
//!
//! Chaque pair devient un serveur MCP nommé comme son agent : le modèle peut
//! appeler `bob.ask` sans configuration manuelle. Ce module ne lit ni n'écrit
//! aucun fichier ; il rend la section à fusionner avec la configuration de
//! l'agent, la priorité restant aux serveurs déjà déclarés par l'utilisateur.

use super::Peer;
use super::discover;
use super::url;
use serde_json::{Map, Value, json};

/// Serveurs des pairs vivants, triés par nom d'agent, sans `exclude` (l'agent
/// lui-même) ni les annonces mortes ou dont l'URL est illisible.
pub fn servers(exclude: &str) -> Map<String, Value> {
    from_peers(discover(), exclude)
}

/// Fusionne les pairs avec les serveurs persistants : un nom déjà pris garde sa
/// configuration (celle de l'utilisateur), on ne l'écrase jamais.
pub fn merged(persistent: &Map<String, Value>, exclude: &str) -> Map<String, Value> {
    merge(persistent, servers(exclude))
}

fn merge(persistent: &Map<String, Value>, pairs: Map<String, Value>) -> Map<String, Value> {
    let mut out = persistent.clone();
    for (name, cfg) in pairs {
        out.entry(name).or_insert(cfg);
    }
    out
}

fn from_peers(peers: Vec<Peer>, exclude: &str) -> Map<String, Value> {
    let mut peers = peers;
    peers.sort_by(|a, b| a.agent.cmp(&b.agent));
    let mut out = Map::new();
    for peer in peers {
        if !peer.alive || peer.agent.is_empty() || peer.agent == exclude {
            continue;
        }
        if url::parse(&peer.url).is_none() {
            continue;
        }
        out.insert(peer.agent, json!({ "url": peer.url }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{from_peers, merge};
    use crate::peers::Peer;
    use serde_json::{Map, json};

    fn peer(agent: &str, url: &str, alive: bool) -> Peer {
        Peer { name: format!("swarm-{agent}"), agent: agent.into(), url: url.into(), pid: 1, alive }
    }

    fn names(servers: &Map<String, serde_json::Value>) -> Vec<&str> {
        servers.keys().map(String::as_str).collect()
    }

    #[test]
    fn excludes_the_agent_itself() {
        let peers = vec![
            peer("alice", "http://127.0.0.1:23080/mcp", true),
            peer("bob", "http://127.0.0.1:23081/mcp", true),
        ];
        let servers = from_peers(peers, "alice");
        assert_eq!(names(&servers), ["bob"]);
    }

    #[test]
    fn ignores_a_dead_peer() {
        let peers = vec![
            peer("bob", "http://127.0.0.1:23081/mcp", false),
            peer("carl", "http://127.0.0.1:23082/mcp", true),
        ];
        let servers = from_peers(peers, "alice");
        assert_eq!(names(&servers), ["carl"]);
    }

    #[test]
    fn keeps_the_global_server_on_a_name_clash() {
        let mut persistent = Map::new();
        persistent.insert("bob".into(), json!({"url": "http://127.0.0.1:4000/mcp"}));
        let pairs = from_peers(vec![peer("bob", "http://127.0.0.1:23081/mcp", true)], "alice");
        let servers = merge(&persistent, pairs);
        assert_eq!(servers["bob"]["url"], "http://127.0.0.1:4000/mcp");
    }

    #[test]
    fn sorts_by_agent_name() {
        let peers = vec![
            peer("carl", "http://127.0.0.1:23082/mcp", true),
            peer("bob", "http://127.0.0.1:23081/mcp", true),
            peer("alice", "http://127.0.0.1:23080/mcp", true),
        ];
        let servers = from_peers(peers, "");
        assert_eq!(names(&servers), ["alice", "bob", "carl"]);
    }

    #[test]
    fn ignores_a_malformed_url() {
        let peers = vec![
            peer("bob", "pas-une-url", true),
            peer("carl", "http://127.0.0.1:23082/mcp", true),
        ];
        let servers = from_peers(peers, "alice");
        assert_eq!(names(&servers), ["carl"]);
    }
}
