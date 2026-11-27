//! Suppression d'un agent : aiguillage vers le runner, puis oubli de la session et de l'état.

use super::runner::Runner;
use super::{antigravity, claude, executable, hermes, lookup};
use crate::{owners, registry, sessions, state};
use std::io::ErrorKind;
use std::path::Path;

pub struct Deleted {
    pub files: Vec<String>,
    pub errors: Vec<String>,
}

pub fn delete(ws: &Path, name: &str, wanted: Option<Runner>) -> Result<Deleted, String> {
    let runner = lookup::resolve(ws, name, wanted)?;
    refuse_if_busy(ws, name, runner)?;
    let mut deleted = match runner {
        Runner::Claude => claude::delete::delete(ws, name),
        Runner::Hermes => hermes::delete::delete(name),
        Runner::Antigravity => antigravity::delete::delete(ws, name),
    };
    remove_local(ws, name, &mut deleted);
    sessions::delete(ws, runner.as_str(), name);
    state::remove(ws, runner.as_str(), name);
    Ok(deleted)
}

/// Retire l'exécutable dédié et l'unit systemd, quel que soit le runner.
/// L'absence de l'un ou de l'autre n'est pas une erreur.
fn remove_local(ws: &Path, name: &str, deleted: &mut Deleted) {
    match executable::remove(ws, name) {
        Some(path) => deleted.files.push(path.display().to_string()),
        None => {
            let path = executable::path(ws, name);
            if path.exists() {
                deleted.errors.push(format!("{}: suppression impossible", path.display()));
            }
        }
    }
    let unit = executable::service_path(ws, name);
    match std::fs::remove_file(&unit) {
        Ok(()) => deleted.files.push(unit.display().to_string()),
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => deleted.errors.push(format!("{}: {e}", unit.display())),
    }
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
