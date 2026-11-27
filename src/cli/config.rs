//! `config` : fichiers de configuration d'un agent (secrets masqués).
//!
//! `--file NAME` filtre par nom de fichier (sous-chaîne : `settings`, `.mcp`, `prompt`…), ce qui
//! évite de déverser les six fichiers d'un agent quand un seul intéresse.

use super::args::Options;
use super::output;
use super::target;
use crate::agents;
use serde_json::{Map, Value};
use std::path::Path;

pub fn run(options: &Options, ws: &Path, agent: &str, file: Option<&str>) -> Result<(), String> {
    target::validate(agent)?;
    let runner = target::runner(ws, agent, options.runner)?;
    let files = agents::get_configs(ws, agent, Some(runner))?;
    if files.is_empty() {
        return Err(format!(
            "agent '{agent}' ({}) : aucun fichier de configuration.",
            runner.as_str()
        ));
    }
    let available: Vec<String> = files.iter().map(|(name, _)| name.clone()).collect();
    let files = select(files, file);
    if files.is_empty() {
        return Err(format!(
            "agent '{agent}' : aucun fichier ne correspond à « {} » (disponibles : {}).",
            file.unwrap_or_default(),
            available.join(", ")
        ));
    }
    if options.json {
        let mut map = Map::new();
        for (name, content) in files {
            map.insert(name, Value::String(content));
        }
        output::json(&Value::Object(map));
    } else {
        for (index, (name, content)) in files.iter().enumerate() {
            if index > 0 {
                println!();
            }
            println!("--- {name} ---");
            println!("{content}");
        }
    }
    Ok(())
}

/// Filtre par sous-chaîne du nom (insensible à la casse) ; `None` garde tout.
fn select(files: Vec<(String, String)>, file: Option<&str>) -> Vec<(String, String)> {
    let Some(needle) = file.map(str::trim).filter(|needle| !needle.is_empty()) else {
        return files;
    };
    let needle = needle.to_lowercase();
    files.into_iter().filter(|(name, _)| name.to_lowercase().contains(&needle)).collect()
}

#[cfg(test)]
mod tests {
    use super::select;

    fn files() -> Vec<(String, String)> {
        vec![
            ("settings_alice.json".into(), "{}".into()),
            ("prompt_alice.md".into(), "tu es Alice".into()),
            (".mcp.alice.json".into(), "{}".into()),
        ]
    }

    #[test]
    fn without_a_filter_every_file_is_kept() {
        assert_eq!(select(files(), None).len(), 3);
        assert_eq!(select(files(), Some("  ")).len(), 3);
    }

    #[test]
    fn the_filter_matches_a_part_of_the_name_whatever_the_case() {
        let chosen = select(files(), Some("SETTINGS"));
        assert_eq!(chosen.len(), 1);
        assert_eq!(chosen[0].0, "settings_alice.json");
        assert_eq!(select(files(), Some("alice")).len(), 3);
        assert!(select(files(), Some("nexus")).is_empty());
    }
}
