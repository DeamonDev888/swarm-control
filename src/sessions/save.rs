//! Sauvegarde de la session d'un agent.

use crate::state;
use crate::store::now_ms;
use serde_json::json;
use std::path::Path;

pub fn save(workspace: &Path, runner: &str, agent: &str, session_id: &str) -> std::io::Result<()> {
    state::update(workspace, runner, agent, |doc| {
        doc["session"] = json!({ "id": session_id, "ts": now_ms() });
    })
}
