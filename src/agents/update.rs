//! Modification d'un agent : aiguillage vers le runner.

use super::runner::Runner;
use super::{antigravity, claude, hermes, lookup};
use serde_json::{Map, Value};
use std::path::Path;

#[derive(Default)]
pub struct Updates {
    pub model: Option<String>,
    pub mcp_servers: Option<Vec<String>>,
    pub env: Option<Map<String, Value>>,
    pub prompt: Option<String>,
    /// Réécriture complète d'un fichier : (`prompt.md` | `settings.json` |
    /// `.mcp.json` | `skill.md`, contenu).
    pub file: Option<(String, String)>,
}

/// Applique les modifications et renvoie la liste des changements.
pub fn update(
    ws: &Path,
    name: &str,
    wanted: Option<Runner>,
    up: &Updates,
) -> Result<Vec<String>, String> {
    let runner = lookup::resolve(ws, name, wanted)?;
    let mut changes = Vec::new();
    match runner {
        Runner::Claude => claude::update::apply(ws, name, up, &mut changes)?,
        Runner::Hermes => hermes::update::apply(ws, name, up, &mut changes)?,
        Runner::Antigravity => antigravity::update::apply(ws, name, up, &mut changes)?,
    }
    if changes.is_empty() {
        changes.push("Aucune modification demandée.".into());
    }
    Ok(changes)
}
