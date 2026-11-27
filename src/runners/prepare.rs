//! Validation d'une demande et réservation du run.

use super::{RunRequest, RunResult};
use crate::agents::{self, validate_name};
use crate::registry::{self, LiveRun};
use std::sync::Arc;

/// Réserve le run dans le registre, puis rafraîchit les pairs dans la config MCP
/// de l'agent (jamais bloquant). Les erreurs d'agent (nom, existence,
/// occupé) sont rendues ici, avant tout lancement.
pub fn prepare(req: &RunRequest) -> Result<Arc<LiveRun>, RunResult> {
    if req.prompt.trim().is_empty() {
        return Err(RunResult::err("INVALID_PROMPT: prompt vide."));
    }
    if let Some(name) = &req.agent {
        validate_name(name).map_err(|e| RunResult::err(format!("INVALID_AGENT_NAME: {e}")))?;
        check_exists(req, name)?;
    }
    let live = registry::begin(req.runner.as_str(), req.agent.as_deref(), &req.workspace)
        .map_err(RunResult::err)?;
    // Tout tour (worker résident, `ask` local, `run_agent`) prépare son run ici :
    // c'est le point où les pairs vivants entrent dans la config MCP, après la
    // réservation (une fois par tour, pas à chaque essai du bail). Un échec de
    // refresh ne fait jamais échouer le tour (doc de `agents::peers`).
    if let Some(name) = &req.agent {
        if let Err(e) = agents::peers::refresh(&req.workspace, req.runner, name) {
            crate::debug!("run {name} ({}): refresh des pairs ignoré · {e}", req.runner.as_str());
        }
    }
    Ok(live)
}

fn check_exists(req: &RunRequest, name: &str) -> Result<(), RunResult> {
    if agents::exists_for(&req.workspace, name, req.runner) {
        return Ok(());
    }
    let hint = match agents::peek_runner(&req.workspace, name) {
        Some(other) => format!(
            " L'agent existe côté {} : relance avec runner:\"{}\".",
            other.as_str(),
            other.as_str()
        ),
        None => " (create_agent d'abord)".to_string(),
    };
    Err(RunResult::err(format!(
        "INVALID_AGENT: agent '{name}' introuvable pour le runner {}.{hint}",
        req.runner.as_str()
    )))
}
