//! Validation d'une demande et réservation du run.

use super::{RunRequest, RunResult};
use crate::agents::{self, validate_name};
use crate::registry::{self, LiveRun};
use std::sync::Arc;

/// Réserve le run dans le registre. Les erreurs d'agent (nom, existence,
/// occupé) sont rendues ici, avant tout lancement.
pub fn prepare(req: &RunRequest) -> Result<Arc<LiveRun>, RunResult> {
    if req.prompt.trim().is_empty() {
        return Err(RunResult::err("INVALID_PROMPT: prompt vide."));
    }
    if let Some(name) = &req.agent {
        validate_name(name).map_err(|e| RunResult::err(format!("INVALID_AGENT_NAME: {e}")))?;
        check_exists(req, name)?;
    }
    registry::begin(req.runner.as_str(), req.agent.as_deref(), &req.workspace)
        .map_err(RunResult::err)
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
