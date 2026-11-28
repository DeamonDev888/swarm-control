//! Lecture de la session d'un agent.

use super::entry;
use crate::state;
use crate::store::now_ms;
use std::path::Path;

/// Session à reprendre : celle du fichier d'état.
pub fn get(workspace: &Path, runner: &str, agent: &str) -> Option<String> {
    let doc = state::read(workspace, runner, agent);
    doc.get("session").filter(|s| !entry::is_expired(s, now_ms())).and_then(entry::extract_id)
}
