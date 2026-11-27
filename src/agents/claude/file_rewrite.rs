//! Réécriture complète d'un fichier d'agent Claude.

use crate::agents::files::io;
use crate::agents::paths::{
    claude_mcp_path, claude_prompt_path, claude_settings_path, claude_skill_path,
};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn apply(
    ws: &Path,
    name: &str,
    file: &str,
    content: &str,
    changes: &mut Vec<String>,
) -> Result<(), String> {
    let target = target_path(ws, name, file)?;
    if file == "settings.json" || file == ".mcp.json" {
        serde_json::from_str::<Value>(content)
            .map_err(|e| format!("{file} n'est pas du JSON valide: {e}"))?;
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    std::fs::write(&target, content).map_err(io)?;
    changes.push(format!("{file} réécrit → {}", target.display()));
    Ok(())
}

fn target_path(ws: &Path, name: &str, file: &str) -> Result<PathBuf, String> {
    match file {
        "prompt.md" => Ok(claude_prompt_path(ws, name)),
        "settings.json" => Ok(claude_settings_path(ws, name)),
        ".mcp.json" => Ok(claude_mcp_path(ws, name)),
        "skill.md" => Ok(claude_skill_path(ws, name)),
        other => Err(format!("Fichier non supporté: {other}")),
    }
}
