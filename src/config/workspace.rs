//! Résolution du dossier de travail.

use super::{env, paths};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn resolve(
    override_dir: Option<PathBuf>,
    dotenv: &HashMap<String, String>,
    home: &Path,
) -> PathBuf {
    override_dir
        .or_else(|| env::app_lookup_in(dotenv, "WORKSPACE").map(|raw| expand_tilde(&raw, home)))
        .or_else(find_mcp_root)
        .unwrap_or_else(|| paths::default_state_dir(home))
}

fn expand_tilde(raw: &str, home: &Path) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        home.join(rest)
    } else if let Some(rest) = raw.strip_prefix('~') {
        home.join(rest)
    } else {
        PathBuf::from(raw)
    }
}

/// Premier dossier contenant `.mcp.json` en remontant depuis le répertoire courant.
fn find_mcp_root() -> Option<PathBuf> {
    let mut current = std::env::current_dir().ok()?;
    loop {
        if current.join(".mcp.json").exists() || current.join(".mcp.local.json").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}
