//! Persona de l'agent : son fichier prompt `.claude/agents/<nom>.md`.

use crate::agents;
use std::path::Path;

pub fn load(ws: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(agents::claude_prompt_path(ws, name))
        .ok()
        .filter(|p| !p.trim().is_empty())
}
