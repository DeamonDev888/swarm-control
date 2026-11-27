//! Sondes des serveurs MCP du registre LiteMCP : qui est annoncé, qui répond.
//!
//! Lecture seule : `status` sur les agents résidents (outil annoté `read_only` côté serveur).
//! Une sonde par serveur, attente bornée : elles tournent dans le fil des sondes (`mod.rs`),
//! jamais dans le fil d'interface.

use crate::peers::{self, CallError, Peer, call_tool};
use crate::tui::state::PeerRow;
use serde_json::{Value, json};
use std::sync::mpsc;
use std::time::Duration;

/// Sonde tous les serveurs `swarm-*` annoncés, une sonde par fil (attente bornée chacun).
/// La ligne affichable ([`PeerRow`]) est définie dans `state` : la découverte en fournit les
/// champs d'identité (nom, adresse, vie, soi), la sonde d'ici ajoute l'état et le détail.
pub fn probe_all(self_agent: &str, wait: Duration) -> Vec<PeerRow> {
    let announced: Vec<Peer> = peers::discover();
    let (tx, rx) = mpsc::channel();
    for peer in announced {
        let name = peer.name.clone();
        let url = peer.url.clone();
        let own = peer.agent == self_agent;
        // Clones dédiés au fil : si le fil ne démarre pas, les originaux restent pour la
        // sonde de secours dans ce fil-ci.
        let spawned = {
            let (name, url, tx) = (name.clone(), url.clone(), tx.clone());
            std::thread::Builder::new().name(format!("swarm-tui-probe-{}", peer.agent)).spawn(
                move || {
                    let _ = tx.send(probe_peer(&name, &url, peer.pid, peer.alive, own, wait));
                },
            )
        };
        // Sans fil disponible : la sonde se fait dans ce fil-ci, l'attente reste bornée.
        if spawned.is_err() {
            let view = probe_peer(&name, &url, peer.pid, peer.alive, own, wait);
            let _ = tx.send(view);
        }
    }
    drop(tx);
    let mut out: Vec<PeerRow> = rx.iter().collect();
    sort_peers(&mut out);
    out
}

/// Ordre d'affichage : vivants d'abord, puis par nom.
pub fn sort_peers(peers: &mut [PeerRow]) {
    peers.sort_by(|a, b| {
        let by_life = b.alive.cmp(&a.alive);
        by_life.then_with(|| a.name.cmp(&b.name))
    });
}

/// Sonde un serveur annoncé : vivant au sens du PID, puis réponse MCP réelle.
pub fn probe_peer(
    name: &str,
    url: &str,
    pid: u32,
    alive: bool,
    own: bool,
    wait: Duration,
) -> PeerRow {
    let mut peer = PeerRow {
        name: name.to_string(),
        addr: addr_of(url),
        alive,
        own,
        state: String::new(),
        detail: String::new(),
    };
    if !alive {
        peer.state = "inactif".into();
        peer.detail = format!("PID {pid} mort");
        return peer;
    }
    match call_tool(url, "status", json!({}), wait) {
        Ok(status) => {
            peer.state = "ok".into();
            peer.detail = detail_from_status(&status);
        }
        Err(error) => {
            peer.state = describe(&error);
            peer.detail = short_reason(&error, 60);
        }
    }
    peer
}

/// `http://hôte:port/mcp` → `hôte:port` (l'URL brute si elle n'a pas la forme attendue).
pub fn addr_of(url: &str) -> String {
    url.trim_start_matches("http://").trim_end_matches("/mcp").to_string()
}

/// Ce que la réponse à `status` vaut pour le panneau : `busy · file 2`.
pub fn detail_from_status(status: &Value) -> String {
    let state = status["state"].as_str().unwrap_or("?");
    let file = status["queueLength"].as_u64().unwrap_or(0);
    format!("{state} · file {file}")
}

/// En un mot, la couleur du panneau.
fn describe(error: &CallError) -> String {
    match error {
        CallError::Unreachable(_) => "injoignable".into(),
        CallError::Tool(_) => "erreur".into(),
        CallError::Protocol(_) => "protocole".into(),
    }
}

/// La raison détaillée d'une sonde ratée, bornée par caractères.
fn short_reason(error: &CallError, max: usize) -> String {
    let reason = match error {
        CallError::Unreachable(m) | CallError::Tool(m) | CallError::Protocol(m) => m.as_str(),
    };
    let reason = reason.split('\n').next().unwrap_or_default();
    let mut out: String = reason.chars().take(max).collect();
    if reason.chars().count() > max {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_pid_mort_est_inactif_sans_reseau() {
        let peer = probe_peer(
            "swarm-alice",
            "http://127.0.0.1:23082/mcp",
            4242,
            false,
            false,
            Duration::from_secs(1),
        );
        assert_eq!(peer.state, "inactif");
        assert_eq!(peer.detail, "PID 4242 mort");
        assert_eq!(peer.addr, "127.0.0.1:23082");
    }

    #[test]
    fn un_port_ferme_est_injoignable() {
        // Le PID 1 est vivant (soi-même) mais le port 9 n'écoute pas : sonde rapide et sûre.
        let peer = probe_peer(
            "swarm-alice",
            "http://127.0.0.1:9/mcp",
            std::process::id(),
            true,
            false,
            Duration::from_millis(500),
        );
        assert_eq!(peer.state, "injoignable", "état : {} ({})", peer.state, peer.detail);
    }

    #[test]
    fn les_vivants_passent_avant_les_morts_puis_par_nom() {
        let mut peers = vec![
            PeerRow {
                name: "swarm-zoe".into(),
                addr: String::new(),
                alive: false,
                own: false,
                state: "inactif".into(),
                detail: String::new(),
            },
            PeerRow {
                name: "swarm-alice".into(),
                addr: String::new(),
                alive: true,
                own: false,
                state: "ok".into(),
                detail: String::new(),
            },
            PeerRow {
                name: "swarm-bob".into(),
                addr: String::new(),
                alive: true,
                own: false,
                state: "ok".into(),
                detail: String::new(),
            },
        ];
        sort_peers(&mut peers);
        let names: Vec<&str> = peers.iter().map(|p| p.name.as_str()).collect();
        // Vivants d'abord (alice, bob), puis les morts (zoe), par nom dans chaque groupe.
        assert_eq!(names, ["swarm-alice", "swarm-bob", "swarm-zoe"]);
        assert!(peers[0].alive && peers[1].alive && !peers[2].alive);
    }

    #[test]
    fn le_statut_devient_une_ligne_courte() {
        assert_eq!(
            detail_from_status(&json!({"state": "busy", "queueLength": 2})),
            "busy · file 2"
        );
        assert_eq!(detail_from_status(&json!({})), "? · file 0");
    }

    #[test]
    fn ladresse_sort_de_lurl() {
        assert_eq!(addr_of("http://127.0.0.1:23082/mcp"), "127.0.0.1:23082");
        assert_eq!(addr_of("http://127.0.0.1:23082"), "127.0.0.1:23082");
    }
}
