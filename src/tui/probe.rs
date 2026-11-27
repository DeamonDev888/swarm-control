//! Sondes des serveurs MCP du registre LiteMCP : qui est annoncé, qui répond.
//!
//! Lecture seule : `status` sur les agents résidents (outil annoté `read_only` côté
//! serveur), `tools/list` sur les instances d'administration. Un fil par sonde, chaque
//! attente bornée — le fil d'interface ne bloque jamais sur le réseau.

use crate::peers::{self, CallError, Peer, call_tool, list_tools};
use crate::tui::view;
use serde_json::{Value, json};
use std::sync::mpsc;
use std::time::Duration;

/// Un serveur MCP annoncé dans le registre, et le résultat de sa dernière sonde.
#[derive(Clone, Debug)]
pub struct PeerView {
    /// Nom complet annoncé (`swarm-alice`, `swarm-control-1`).
    pub name: String,
    /// Nom d'agent (`alice`), vide pour une instance.
    pub agent: String,
    /// URL MCP annoncée.
    pub url: String,
    /// Port TCP extrait de l'URL (0 si illisible).
    pub port: u16,
    /// PID annoncé.
    pub pid: u32,
    /// Le PID annoncé est-il vivant ?
    pub alive: bool,
    /// Instance d'administration (`swarm-control-N`), pas un agent.
    pub is_instance: bool,
    /// Ce serveur est celui de l'agent affiché par la TUI.
    pub is_self: bool,
    /// État de la sonde : « ok », « inactif », ou la raison.
    pub state: String,
    /// Détail court renvoyé par la sonde (état de file, nombre d'outils…).
    pub detail: String,
}

/// Sonde tous les serveurs annoncés (agents + instances), un fil par sonde.
pub fn probe_all(self_name: &str, self_url: Option<&str>, wait: Duration) -> Vec<PeerView> {
    let mut announced: Vec<Peer> = peers::discover();
    announced.extend(peers::instances());
    let (tx, rx) = mpsc::channel();
    for peer in announced {
        let tx = mpsc::Sender::clone(&tx);
        let name = peer.name.clone();
        let agent = peer.agent.clone();
        let url = peer.url.clone();
        let is_instance = peer.is_instance();
        let spawned = std::thread::Builder::new()
            .name(format!("swarm-tui-probe-{}", peer.agent))
            .spawn(move || {
                let _ = tx.send(probe_peer(&name, &agent, &url, peer.pid, peer.alive, is_instance, wait));
            });
        // Sans fil disponible : sonde dans ce fil-ci (l'attente reste bornée).
        if spawned.is_err() {
            let view = probe_peer(&name, &agent, &url, peer.pid, peer.alive, is_instance, wait);
            let _ = tx.send(view);
        }
    }
    drop(tx);
    let mut out: Vec<PeerView> = rx.iter().collect();
    for peer in &mut out {
        peer.is_self = peer.name == self_name || Some(peer.url.as_str()) == self_url;
    }
    out.sort_by(|a, b| b.alive.cmp(&a.alive).then_with(|| a.name.cmp(&b.name)));
    out
}

/// Sonde un serveur annoncé : vivant au sens du PID, puis réponse MCP réelle.
pub fn probe_peer(
    name: &str,
    agent: &str,
    url: &str,
    pid: u32,
    alive: bool,
    is_instance: bool,
    wait: Duration,
) -> PeerView {
    let port = view::port_of(url).unwrap_or(0);
    let mut peer = PeerView {
        name: name.to_string(),
        agent: agent.to_string(),
        url: url.to_string(),
        port,
        pid,
        alive,
        is_instance,
        is_self: false,
        state: String::new(),
        detail: String::new(),
    };
    if !alive {
        peer.state = format!("inactif (PID {pid} mort)");
        return peer;
    }
    let outcome = if is_instance {
        list_tools(url, wait)
            .map(|tools| format!("{} outils", tools["tools"].as_array().map(Vec::len).unwrap_or(0)))
    } else {
        call_tool(url, "status", json!({}), wait).map(|status| detail_from_status(&status))
    };
    match outcome {
        Ok(detail) => {
            peer.state = "ok".into();
            peer.detail = detail;
        }
        Err(error) => {
            peer.state = describe(&error);
            peer.detail = view::short(&reason(&error), 80);
        }
    }
    peer
}

/// Ce que vaut l'état d'un agent pour le panneau : « busy · file 2 ».
pub fn detail_from_status(status: &Value) -> String {
    let state = status["state"].as_str().unwrap_or("?");
    let file = status["queueLength"].as_u64().unwrap_or(0);
    format!("{state} · file {file}")
}

/// Résumé d'une sonde ratée : en un mot, la couleur du panneau.
fn describe(error: &CallError) -> String {
    match error {
        CallError::Unreachable(_) => "injoignable".into(),
        CallError::Tool(_) => "erreur".into(),
        CallError::Protocol(_) => "protocole".into(),
    }
}

/// La raison détaillée d'une sonde ratée.
fn reason(error: &CallError) -> String {
    match error {
        CallError::Unreachable(m) | CallError::Tool(m) | CallError::Protocol(m) => m.clone(),
    }
}
