//! Lecture des fichiers de configuration d'un agent.

use super::runner::Runner;
use super::{claude, hermes, lookup};
use std::path::Path;

/// Fichiers de l'agent sous forme `(nom, contenu)` ; secrets masqués.
pub fn get_configs(
    ws: &Path,
    name: &str,
    wanted: Option<Runner>,
) -> Result<Vec<(String, String)>, String> {
    Ok(match lookup::resolve(ws, name, wanted)? {
        Runner::Claude => claude::configs::read(ws, name),
        Runner::Hermes => hermes::configs::read(name),
    })
}
