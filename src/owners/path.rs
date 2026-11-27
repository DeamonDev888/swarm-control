//! Emplacement des baux.

use std::path::{Path, PathBuf};

/// Baux de run.
pub fn dir(ws: &Path) -> PathBuf {
    ws.join("bridge").join("owners")
}

pub fn file(ws: &Path, runner: &str, agent: &str) -> PathBuf {
    dir(ws).join(format!("{runner}-{agent}.json"))
}

/// Baux d'hébergement (agents résidents).
pub fn hosts_dir(ws: &Path) -> PathBuf {
    ws.join("bridge").join("hosts")
}

pub fn host_file(ws: &Path, runner: &str, agent: &str) -> PathBuf {
    hosts_dir(ws).join(format!("{runner}-{agent}.json"))
}
