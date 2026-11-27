//! Binaire Hermes.

use super::path_lookup::on_path;
use crate::config;
use std::path::PathBuf;

/// `hermes` : `HERMES_BIN`, puis le PATH (en évitant les wrappers `.cmd`).
pub fn hermes_binary() -> PathBuf {
    if let Some(bin) = config::var("HERMES_BIN").filter(|s| !s.is_empty()) {
        return PathBuf::from(bin);
    }
    let names: &[&str] = if cfg!(windows) { &["hermes.exe"] } else { &["hermes"] };
    on_path(names).unwrap_or_else(|| PathBuf::from("hermes"))
}
