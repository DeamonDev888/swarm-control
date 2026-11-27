//! Lecture des fichiers d'un agent Claude.

use crate::agents::files::read_or_missing;
use crate::agents::paths::{
    claude_mcp_path, claude_prompt_path, claude_settings_path, claude_skill_path,
};
use crate::agents::secrets::mask_settings;
use std::path::Path;

pub fn read(ws: &Path, name: &str) -> Vec<(String, String)> {
    let mut out = vec![
        ("prompt.md".into(), read_or_missing(&claude_prompt_path(ws, name))),
        ("settings.json".into(), mask_settings(&read_or_missing(&claude_settings_path(ws, name)))),
        (".mcp.json".into(), read_or_missing(&claude_mcp_path(ws, name))),
    ];
    let skill = claude_skill_path(ws, name);
    if skill.is_file() {
        out.push(("skill.md".into(), read_or_missing(&skill)));
    }
    out
}
