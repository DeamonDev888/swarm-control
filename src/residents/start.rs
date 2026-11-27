//! Démarrage de la résidence d'un agent.

use super::ctx::Ctx;
use super::host::AgentHost;
use super::info::HostInfo;
use super::{server, table, worker};
use crate::agents::{self, Runner};
use crate::owners::{self, Busy};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static TOKEN_SEQ: AtomicU64 = AtomicU64::new(0);

/// Rend `agent` résident dans ce process : bail d'hébergement, serveur MCP, file et fil de travail.
pub fn start(ws: &Path, runner: Runner, agent: &str) -> Result<HostInfo, String> {
    agents::validate_name(agent)?;
    if !agents::exists_for(ws, agent, runner) {
        return Err(format!("Agent '{agent}' introuvable pour le runner {}.", runner.as_str()));
    }
    // Un bail d'hébergement pris AVANT de lier le serveur : lier annonce `swarm-<agent>`
    // dans le registre, ce qui écraserait l'annonce d'un hébergeur déjà en place.
    let token = format!("host-{}-{}", std::process::id(), TOKEN_SEQ.fetch_add(1, Ordering::SeqCst));
    let lease = owners::acquire_host(ws, runner.as_str(), agent, &token)
        .map_err(|busy| hosted_error(busy, agent))?;

    let ctx = Arc::new(Ctx::new(agent, runner, ws.to_path_buf(), owners::instance_name()));
    let (bound, handle) = server::bind(&ctx)?;
    let addr = bound.local_addr().map_err(|e| format!("adresse du serveur illisible: {e}"))?;
    let url = format!("http://{addr}/mcp");
    ctx.set_url(url.clone());
    lease.edit(|holder| holder.url = Some(url));

    std::thread::Builder::new()
        .name(format!("swarm-srv-{agent}"))
        .spawn(move || {
            let _ = bound.serve();
        })
        .map_err(|e| format!("thread du serveur impossible: {e}"))?;
    let worker_ctx = Arc::clone(&ctx);
    let worker = std::thread::Builder::new()
        .name(format!("swarm-worker-{agent}"))
        .spawn(move || worker::run(worker_ctx, lease))
        .map_err(|e| {
            handle.shutdown();
            format!("thread de travail impossible: {e}")
        })?;

    let host = Arc::new(AgentHost::new(ctx, handle, worker));
    let info = host.info();
    table::insert(host);
    Ok(info)
}

fn hosted_error(busy: Busy, agent: &str) -> String {
    match busy {
        Busy::Held(h) => format!(
            "AGENT_HOSTED: l'agent '{agent}' est déjà hébergé par {} ({}).",
            h.instance,
            h.url.as_deref().unwrap_or("URL inconnue")
        ),
        Busy::Pending => {
            format!("AGENT_HOSTED: hébergement de '{agent}' en cours ailleurs : réessaie.")
        }
        Busy::Io(e) => format!("LEASE_ERROR: bail d'hébergement de '{agent}' impossible ({e})."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosted_error_names_the_host_and_its_url() {
        let ws = std::env::temp_dir().join(format!("swarm-start-{}", std::process::id()));
        let first = owners::acquire_host(&ws, "claude", "alice", "host-1").unwrap();
        first.edit(|h| h.url = Some("http://127.0.0.1:23082/mcp".into()));
        // Un second hébergement est refusé : le message doit dire QUI héberge, et OÙ.
        let busy = owners::acquire_host(&ws, "claude", "alice", "host-2").unwrap_err();
        let msg = hosted_error(busy, "alice");
        assert!(msg.starts_with("AGENT_HOSTED"), "{msg}");
        assert!(msg.contains(&owners::instance_name()), "{msg}");
        assert!(msg.contains("http://127.0.0.1:23082/mcp"), "{msg}");
        drop(first);
        let _ = std::fs::remove_dir_all(&ws);
    }
}
