//! Suppression d'un agent : aiguillage vers le runner, puis oubli de la session et de l'état.

use super::runner::Runner;
use super::{claude, hermes, lookup};
use crate::{owners, registry, sessions, state};
use std::path::Path;

pub struct Deleted {
    pub files: Vec<String>,
    pub errors: Vec<String>,
}

pub fn delete(ws: &Path, name: &str, wanted: Option<Runner>) -> Result<Deleted, String> {
    let runner = lookup::resolve(ws, name, wanted)?;
    refuse_if_busy(ws, name, runner)?;
    let deleted = match runner {
        Runner::Claude => claude::delete::delete(ws, name),
        Runner::Hermes => hermes::delete::delete(name),
    };
    sessions::delete(ws, runner.as_str(), name);
    state::remove(ws, runner.as_str(), name);
    Ok(deleted)
}

/// Refus si l'agent tourne (ici, ou dans une autre instance par son bail) ou s'il est résident.
fn refuse_if_busy(ws: &Path, name: &str, runner: Runner) -> Result<(), String> {
    let running_here = registry::find(name, Some(runner.as_str()))
        .is_some_and(|run| run.snapshot().status == registry::Status::Running);
    if running_here || owners::holder_of(ws, runner.as_str(), name).is_some() {
        return Err(format!("L'agent '{name}' a un run en cours : agent_control(kill) d'abord."));
    }
    if owners::host_of(ws, runner.as_str(), name).is_some() {
        return Err(format!("L'agent '{name}' est résident : agent_stop d'abord."));
    }
    Ok(())
}
