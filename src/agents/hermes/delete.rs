//! Suppression d'un profil Hermes.

use super::cli;
use crate::agents::Deleted;
use crate::agents::paths::hermes_profile_dir;

pub fn delete(name: &str) -> Deleted {
    let result = cli::run(&["profile", "delete", name, "--yes"]);
    let mut out = Deleted { files: Vec::new(), errors: Vec::new() };
    // Hermes peut sortir en code non nul (arrêt du gateway, alias…) alors que
    // le profil est bien supprimé : le disque fait foi.
    if hermes_profile_dir(name).exists() {
        let why = result.err().unwrap_or_else(|| "le dossier existe encore".into());
        out.errors.push(format!("hermes profile delete: {}", why.lines().last().unwrap_or("")));
    } else {
        out.files.push(format!("profil hermes: {name}"));
    }
    out
}
