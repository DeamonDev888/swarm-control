//! Réécriture complète d'un fichier de profil Hermes.

use crate::agents::files::io;
use std::path::Path;

pub fn apply(
    profile_dir: &Path,
    file: &str,
    content: &str,
    changes: &mut Vec<String>,
) -> Result<(), String> {
    let target = match file {
        "prompt.md" => profile_dir.join("SOUL.md"),
        "settings.json" => profile_dir.join("config.yaml"),
        ".mcp.json" => profile_dir.join("mcp.json"),
        "skill.md" => profile_dir.join("skills").join("skill.md"),
        other => return Err(format!("Fichier non supporté: {other}")),
    };
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    std::fs::write(&target, content).map_err(io)?;
    changes.push(format!("{file} réécrit → {}", target.display()));
    Ok(())
}
