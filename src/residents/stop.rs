//! Arrêt de la résidence d'un agent.

use super::info::HostInfo;
use super::table;
use crate::agents::Runner;
use crate::owners;
use std::path::Path;

/// Arrête un agent hébergé **par ce process**. `runner` lève l'ambiguïté si le même nom
/// est résident sous Claude et sous Hermes.
pub fn stop(ws: &Path, agent: &str, runner: Option<Runner>) -> Result<HostInfo, String> {
    let mut hosts = table::find(agent, runner);
    match hosts.len() {
        0 => Err(not_here(ws, agent)),
        1 => {
            let host = hosts.remove(0);
            table::remove(host.ctx().runner, agent);
            let info = host.info();
            host.shutdown();
            Ok(info)
        }
        _ => Err(format!("L'agent '{agent}' est résident sous deux runners : précise runner.")),
    }
}

/// Message d'erreur : l'agent n'est pas hébergé ici — peut-être l'est-il ailleurs.
fn not_here(ws: &Path, agent: &str) -> String {
    for runner in ["claude", "hermes"] {
        if let Some(h) = owners::host_of(ws, runner, agent) {
            return format!(
                "L'agent '{agent}' n'est pas hébergé par cette instance : il l'est par {} ({}).",
                h.instance,
                h.url.as_deref().unwrap_or("URL inconnue")
            );
        }
    }
    format!("L'agent '{agent}' n'est pas résident.")
}
