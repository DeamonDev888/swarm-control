//! Existence et résolution du runner d'un agent.

use super::name::validate_name;
use super::paths::{
    antigravity_prompt_path, antigravity_settings_path, claude_prompt_path, claude_settings_path,
    hermes_profile_dir,
};
use super::runner::Runner;
use std::path::Path;

pub fn hermes_profile_exists(name: &str) -> bool {
    hermes_profile_dir(name).join("config.yaml").is_file()
}

/// L'agent existe-t-il pour **ce** runner ? (Un même nom peut exister sur
/// plusieurs runners : chacun a ses fichiers et sa session.)
pub fn exists_for(ws: &Path, name: &str, runner: Runner) -> bool {
    match runner {
        Runner::Hermes => hermes_profile_exists(name),
        Runner::Claude => claude_settings_path(ws, name).is_file(),
        Runner::Antigravity => antigravity_settings_path(ws, name).is_file(),
    }
}

/// Runner effectif d'un agent : profil Hermes, puis Claude, puis Antigravity.
pub fn peek_runner(ws: &Path, name: &str) -> Option<Runner> {
    if hermes_profile_exists(name) {
        return Some(Runner::Hermes);
    }
    if claude_files_exist(ws, name) {
        return Some(Runner::Claude);
    }
    antigravity_files_exist(ws, name).then_some(Runner::Antigravity)
}

fn claude_files_exist(ws: &Path, name: &str) -> bool {
    claude_settings_path(ws, name).is_file() || claude_prompt_path(ws, name).is_file()
}

fn antigravity_files_exist(ws: &Path, name: &str) -> bool {
    antigravity_settings_path(ws, name).is_file() || antigravity_prompt_path(ws, name).is_file()
}

/// Runner visé : celui demandé (s'il existe pour cet agent), sinon l'unique
/// runner possible ; ambigu → erreur explicite.
pub fn resolve(ws: &Path, name: &str, wanted: Option<Runner>) -> Result<Runner, String> {
    validate_name(name)?;
    match wanted {
        Some(r) if exists_for(ws, name, r) => Ok(r),
        Some(r) => Err(format!("Agent '{name}' introuvable pour le runner {}.", r.as_str())),
        None => {
            let mut found = Vec::new();
            if claude_files_exist(ws, name) {
                found.push(Runner::Claude);
            }
            if hermes_profile_exists(name) {
                found.push(Runner::Hermes);
            }
            if antigravity_files_exist(ws, name) {
                found.push(Runner::Antigravity);
            }
            match found.as_slice() {
                [only] => Ok(*only),
                [] => Err(format!("Agent '{name}' introuvable.")),
                many => {
                    let names: Vec<&str> = many.iter().map(|r| r.as_str()).collect();
                    Err(format!(
                        "Agent '{name}' existe côté {} : précise runner.",
                        names.join(" ET ")
                    ))
                }
            }
        }
    }
}
