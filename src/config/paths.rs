//! Dossiers de l'utilisateur.

use std::path::{Path, PathBuf};

/// Dossier d'état sous le home : agents et sessions y sont conservés.
pub const STATE_DIRS: [&str; 1] = [".swarm-control"];

pub fn home_dir() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn state_dirs(home: &Path) -> Vec<PathBuf> {
    STATE_DIRS.iter().map(|d| home.join(d)).collect()
}

/// Premier dossier d'état existant, sinon le nouveau.
pub fn default_state_dir(home: &Path) -> PathBuf {
    state_dirs(home).into_iter().find(|d| d.is_dir()).unwrap_or_else(|| home.join(STATE_DIRS[0]))
}
