//! Lecture des fichiers d'un agent Antigravity.

use crate::agents::files::read_or_missing;
use crate::agents::paths::{antigravity_prompt_path, antigravity_settings_path};
use crate::agents::secrets::mask_settings;
use std::path::Path;

pub fn read(ws: &Path, name: &str) -> Vec<(String, String)> {
    vec![
        ("prompt.md".into(), read_or_missing(&antigravity_prompt_path(ws, name))),
        (
            "settings.json".into(),
            mask_settings(&read_or_missing(&antigravity_settings_path(ws, name))),
        ),
    ]
}
