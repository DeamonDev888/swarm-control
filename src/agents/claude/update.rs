//! Modification d'un agent Claude.

use super::{file_rewrite, settings_edit};
use crate::agents::Updates;
use crate::agents::files::io;
use crate::agents::paths::claude_prompt_path;
use std::path::Path;

pub fn apply(ws: &Path, name: &str, up: &Updates, changes: &mut Vec<String>) -> Result<(), String> {
    if let Some((file, content)) = &up.file {
        return file_rewrite::apply(ws, name, file, content, changes);
    }
    if let Some(prompt) = &up.prompt {
        let path = claude_prompt_path(ws, name);
        std::fs::create_dir_all(path.parent().expect("agents/ a un parent")).map_err(io)?;
        std::fs::write(&path, prompt).map_err(io)?;
        changes.push("Prompt mis à jour".into());
    }
    if up.model.is_some() || up.env.is_some() || up.mcp_servers.is_some() {
        settings_edit::apply(ws, name, up, changes)?;
    }
    Ok(())
}
