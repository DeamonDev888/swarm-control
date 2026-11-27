//! Suppression des fichiers d'un agent Claude.

use crate::agents::Deleted;
use crate::agents::paths::{
    claude_mcp_path, claude_prompt_path, claude_settings_path, claude_skill_path,
};
use std::io::ErrorKind;
use std::path::Path;

pub fn delete(ws: &Path, name: &str) -> Deleted {
    let mut out = Deleted { files: Vec::new(), errors: Vec::new() };
    let paths = [
        claude_prompt_path(ws, name),
        claude_settings_path(ws, name),
        claude_mcp_path(ws, name),
        claude_skill_path(ws, name),
    ];
    for path in paths {
        match std::fs::remove_file(&path) {
            Ok(()) => out.files.push(path.display().to_string()),
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => out.errors.push(format!("{}: {e}", path.display())),
        }
    }
    out
}
