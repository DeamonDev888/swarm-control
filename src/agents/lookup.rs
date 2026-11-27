//! Existence et résolution du runner d'un agent.

use super::name::validate_name;
use super::paths::{claude_prompt_path, claude_settings_path, hermes_profile_dir};
use super::runner::Runner;
use std::path::Path;

pub fn hermes_profile_exists(name: &str) -> bool {
    hermes_profile_dir(name).join("config.yaml").is_file()
}

/// L'agent existe-t-il pour **ce** runner ? (Un même nom peut exister côté
/// Claude et côté Hermes : chacun a ses fichiers et sa session.)
pub fn exists_for(ws: &Path, name: &str, runner: Runner) -> bool {
    match runner {
        Runner::Hermes => hermes_profile_exists(name),
        Runner::Claude => claude_settings_path(ws, name).is_file(),
    }
}

/// Runner effectif d'un agent : profil Hermes d'abord, sinon fichiers Claude.
pub fn peek_runner(ws: &Path, name: &str) -> Option<Runner> {
    if hermes_profile_exists(name) {
        return Some(Runner::Hermes);
    }
    claude_files_exist(ws, name).then_some(Runner::Claude)
}

fn claude_files_exist(ws: &Path, name: &str) -> bool {
    claude_settings_path(ws, name).is_file() || claude_prompt_path(ws, name).is_file()
}

/// Runner visé : celui demandé (s'il existe pour cet agent), sinon l'unique
/// runner possible ; ambigu (Claude **et** Hermes) → erreur explicite.
pub fn resolve(ws: &Path, name: &str, wanted: Option<Runner>) -> Result<Runner, String> {
    validate_name(name)?;
    match wanted {
        Some(r) if exists_for(ws, name, r) => Ok(r),
        Some(r) => Err(format!("Agent '{name}' introuvable pour le runner {}.", r.as_str())),
        None => match (claude_files_exist(ws, name), hermes_profile_exists(name)) {
            (true, true) => {
                Err(format!("Agent '{name}' existe côté claude ET hermes : précise runner."))
            }
            (true, false) => Ok(Runner::Claude),
            (false, true) => Ok(Runner::Hermes),
            (false, false) => Err(format!("Agent '{name}' introuvable.")),
        },
    }
}
