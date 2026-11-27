//! Home Hermes natif.

use super::{env, paths};
use std::path::PathBuf;

/// Home Hermes **natif** : là où le CLI `hermes` lit ses profils
/// (`%LOCALAPPDATA%\hermes` sous Windows, `~/.hermes` ailleurs).
pub fn native_hermes_home() -> PathBuf {
    if let Some(home) = env::var("HERMES_HOME").filter(|s| !s.is_empty()) {
        return PathBuf::from(home);
    }
    if cfg!(windows) {
        let base = std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(paths::home_dir);
        base.join("hermes")
    } else {
        paths::home_dir().join(".hermes")
    }
}
