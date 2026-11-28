//! Oubli de la session d'un agent.

use crate::state;
use std::path::Path;

/// Oublie la session de l'agent. `true` si une session existait.
pub fn delete(workspace: &Path, runner: &str, agent: &str) -> bool {
    let had = state::read(workspace, runner, agent).get("session").is_some();
    if had {
        let _ = state::update(workspace, runner, agent, |doc| {
            if let Some(object) = doc.as_object_mut() {
                object.remove("session");
            }
        });
    }
    had
}
