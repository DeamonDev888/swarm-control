//! Dossiers de l'utilisateur.

use std::path::{Path, PathBuf};

/// Dossiers d'état reconnus sous le home : le nouveau d'abord, l'ancien
/// (OverMind) ensuite, pour retrouver agents et sessions existants.
pub const STATE_DIRS: [&str; 2] = [".swarm-control", ".overmind"];

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
