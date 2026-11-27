//! Binaire Antigravity CLI (`agy`).

use super::path_lookup::on_path;
use crate::config;
use std::path::PathBuf;

/// `agy` : `AGY_BIN`, `ANTIGRAVITY_BIN`, dossier d'installation officiel, puis le PATH.
pub fn antigravity_binary() -> PathBuf {
    for var in ["AGY_BIN", "ANTIGRAVITY_BIN"] {
        if let Some(bin) = config::var(var).filter(|s| !s.is_empty()) {
            return PathBuf::from(bin);
        }
    }
    if let Some(installed) = default_install_exe().filter(|p| p.is_file()) {
        return installed;
    }
    let names: &[&str] = if cfg!(windows) { &["agy.exe", "agy.cmd", "agy"] } else { &["agy"] };
    on_path(names).unwrap_or_else(|| PathBuf::from("agy"))
}

fn default_install_exe() -> Option<PathBuf> {
    if cfg!(windows) {
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| config::home_dir().join("AppData").join("Local"));
        Some(local.join("agy").join("bin").join("agy.exe"))
    } else {
        Some(config::home_dir().join(".local").join("bin").join("agy"))
    }
}
