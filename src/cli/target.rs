//! Cible d'une commande : nom d'agent, runner, agent hébergé ici ou pair annoncé.

use crate::agents::{self, Runner};
use crate::peers::{self, Peer};
use crate::residents::{self, Ctx};
use std::path::Path;
use std::sync::Arc;

/// Les runners connus, dans l'ordre de résolution (profil Hermes d'abord, comme `lookup`).
pub const RUNNERS: [Runner; 3] = [Runner::Hermes, Runner::Claude, Runner::Antigravity];

/// Valide le nom d'agent (`[a-zA-Z0-9_-]`, anti-traversée).
pub fn validate(agent: &str) -> Result<(), String> {
    agents::validate_name(agent).map_err(|e| format!("agent: {e}"))
}

/// Agent hébergé **par ce process** : runner demandé, sinon le premier qui l'héberge.
pub fn hosted(agent: &str, runner: Option<Runner>) -> Option<Arc<Ctx>> {
    match runner {
        Some(runner) => residents::ctx_of(agent, runner),
        None => RUNNERS.iter().find_map(|runner| residents::ctx_of(agent, *runner)),
    }
}

/// Pair **vivant** annoncé sous `swarm-<agent>` (une annonce morte ne compte pas).
pub fn peer(agent: &str) -> Option<Peer> {
    peers::find(agent).filter(|peer| peer.alive)
}

/// Runner de l'agent : celui de son hôte local, sinon celui de ses fichiers.
///
/// Lève l'ambiguïté d'un agent présent sous plusieurs runners quand `runner` est fourni.
pub fn runner(ws: &Path, agent: &str, runner: Option<Runner>) -> Result<Runner, String> {
    if let Some(ctx) = hosted(agent, runner) {
        return Ok(ctx.runner);
    }
    agents::resolve(ws, agent, runner).map_err(|e| format!("agent {agent}: {e}"))
}

/// Runner probable sans exiger que l'agent existe : hôte local, fichiers, ou runner demandé.
pub fn known_runner(ws: &Path, agent: &str, runner: Option<Runner>) -> Option<Runner> {
    hosted(agent, runner)
        .map(|ctx| ctx.runner)
        .or_else(|| agents::resolve(ws, agent, runner).ok())
        .or(runner)
}

#[cfg(test)]
mod tests {
    use super::{RUNNERS, validate};
    use crate::agents::Runner;

    #[test]
    fn agent_names_are_checked_before_any_io() {
        assert!(validate("discord_2").is_ok());
        assert!(validate("../x").is_err());
        assert!(validate("").is_err());
    }

    #[test]
    fn every_runner_is_known_once() {
        let names: Vec<&str> = RUNNERS.iter().map(|r| r.as_str()).collect();
        assert_eq!(names, ["hermes", "claude", "antigravity"]);
        for name in names {
            assert_eq!(Runner::parse(name).map(Runner::as_str), Some(name));
        }
    }
}
