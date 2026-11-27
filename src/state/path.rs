//! Emplacement des fichiers d'état.

use std::path::{Path, PathBuf};

pub fn dir(ws: &Path) -> PathBuf {
    ws.join("bridge").join("state")
}

/// `<runner>-<agent>.json` : le nom d'agent est validé en amont (`[a-zA-Z0-9_-]`).
pub fn file(ws: &Path, runner: &str, agent: &str) -> PathBuf {
    dir(ws).join(format!("{runner}-{agent}.json"))
}
