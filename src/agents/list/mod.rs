//! Inventaire des agents (Claude et Hermes).

mod claude;
mod hermes;
mod info;

pub use info::AgentInfo;
use std::path::Path;

/// Tous les agents, triés par runner puis par nom.
pub fn list(ws: &Path) -> Vec<AgentInfo> {
    let mut out = claude::scan(ws);
    for agent in hermes::scan() {
        // Un nom déjà pris côté Claude garde son entrée (comme l'OverMind TS).
        if !out.iter().any(|a| a.name == agent.name) {
            out.push(agent);
        }
    }
    out.sort_by(|a, b| (a.runner.as_str(), &a.name).cmp(&(b.runner.as_str(), &b.name)));
    out
}
