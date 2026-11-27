//! Suppression des fichiers d'un agent Antigravity.

use crate::agents::Deleted;
use crate::agents::paths::{antigravity_prompt_path, antigravity_settings_path};
use std::io::ErrorKind;
use std::path::Path;

pub fn delete(ws: &Path, name: &str) -> Deleted {
    let mut out = Deleted { files: Vec::new(), errors: Vec::new() };
    let paths = [antigravity_prompt_path(ws, name), antigravity_settings_path(ws, name)];
    for path in paths {
        match std::fs::remove_file(&path) {
            Ok(()) => out.files.push(path.display().to_string()),
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => out.errors.push(format!("{}: {e}", path.display())),
        }
    }
    out
}
