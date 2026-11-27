//! Découverte des agents résidents annoncés dans le registre LiteMCP.
//!
//! LiteMCP publie `%TEMP%/litemcp-registry/<nom>.json` à chaque `announce()` :
//! `{name, addr, port, pid, instance, exe, args, cwd, ts}`. Les agents de swarm-control
//! s'appellent `swarm-<agent>` ; ce module les rend utilisables par la TUI et, plus tard,
//! par l'injection automatique des pairs dans la configuration MCP.

use crate::process;
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;

/// Un agent résident vu dans le registre.
#[derive(Clone, Debug, Serialize)]
pub struct Peer {
    /// Nom complet annoncé (`swarm-alice`).
    pub name: String,
    /// Nom de l'agent (`alice`).
    pub agent: String,
    /// URL MCP (`http://127.0.0.1:<port>/mcp`).
    pub url: String,
    pub pid: u32,
    /// Le PID annoncé est-il vivant ?
    pub alive: bool,
}

/// Tous les `swarm-*` du registre, triés par nom. Une annonce illisible est ignorée.
pub fn discover() -> Vec<Peer> {
    all().into_iter().filter(|p| !p.is_instance()).collect()
}

/// Les instances d'administration annoncées (`swarm-control-N`).
pub fn instances() -> Vec<Peer> {
    all().into_iter().filter(Peer::is_instance).collect()
}

fn all() -> Vec<Peer> {
    let dir = std::env::temp_dir().join("litemcp-registry");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut peers: Vec<Peer> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| peer(&e.path()))
        .collect();
    peers.sort_by(|a, b| a.name.cmp(&b.name));
    peers
}

/// L'agent résident `agent` (le premier vivant, sinon le premier annoncé).
pub fn find(agent: &str) -> Option<Peer> {
    let mut matches: Vec<Peer> = discover().into_iter().filter(|p| p.agent == agent).collect();
    let alive = matches.iter().position(|p| p.alive);
    match alive {
        Some(i) => Some(matches.swap_remove(i)),
        None => matches.into_iter().next(),
    }
}

fn peer(file: &PathBuf) -> Option<Peer> {
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    let name = doc.get("name")?.as_str()?.to_string();
    let agent = name.strip_prefix("swarm-")?.to_string();
    let port = doc.get("port")?.as_u64()?;
    let pid = doc.get("pid").and_then(Value::as_u64).unwrap_or(0) as u32;
    if port == 0 || port > u16::MAX as u64 {
        return None;
    }
    Some(Peer {
        name,
        agent,
        url: format!("http://127.0.0.1:{port}/mcp"),
        pid,
        alive: process::pid_alive(pid),
    })
}

#[cfg(test)]
mod tests {
    use super::peer;
    use std::path::PathBuf;

    fn write(name: &str, body: &str) -> PathBuf {
        let file =
            std::env::temp_dir().join(format!("swarm-peer-{name}-{}.json", std::process::id()));
        std::fs::write(&file, body).unwrap();
        file
    }

    #[test]
    fn reads_a_litemcp_announcement() {
        let file = write(
            "alice",
            r#"{"name":"swarm-alice","addr":"127.0.0.1:23082","port":23082,"pid":4242}"#,
        );
        let p = peer(&file).expect("annonce lisible");
        assert_eq!((p.agent.as_str(), p.port_url()), ("alice", "http://127.0.0.1:23082/mcp"));
        // Un PID certainement mort (sous Windows, tasklist ne le connaît pas).
        assert!(!p.alive);
        let _ = std::fs::remove_file(file);
    }

    #[test]
    fn ignores_other_servers_and_broken_files() {
        let other = write("other", r#"{"name":"litemcp-test","port":1,"pid":1}"#);
        assert!(peer(&other).is_none());
        let broken = write("broken", "pas du json");
        assert!(peer(&broken).is_none());
        let _ = std::fs::remove_file(other);
        let _ = std::fs::remove_file(broken);
    }
}

impl Peer {
    /// URL complète (raccourci de test).
    pub fn port_url(&self) -> &str {
        &self.url
    }

    /// Annonce d'une instance d'administration (`swarm-control-N`).
    pub fn is_instance(&self) -> bool {
        self.name
            .strip_prefix("swarm-control-")
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    }
}
