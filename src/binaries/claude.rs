//! Binaire Claude Code.

use super::path_lookup::on_path;
use crate::config;
use std::path::PathBuf;

/// `claude` : `CLAUDE_BIN`, puis l'exécutable npm global (Windows), puis le PATH.
pub fn claude_binary() -> PathBuf {
    if let Some(bin) = config::var("CLAUDE_BIN").filter(|s| !s.is_empty()) {
        return PathBuf::from(bin);
    }
    if cfg!(windows) {
        if let Some(npm) = npm_global_exe().filter(|p| p.is_file()) {
            return npm;
        }
        if let Some(p) = on_path(&["claude.exe"]) {
            return p;
        }
    } else if let Some(p) = on_path(&["claude"]) {
        return p;
    }
    PathBuf::from("claude")
}

fn npm_global_exe() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| config::home_dir().join("AppData").join("Roaming"));
    Some(
        appdata
            .join("npm")
            .join("node_modules")
            .join("@anthropic-ai")
            .join("claude-code")
            .join("bin")
            .join("claude.exe"),
    )
}
