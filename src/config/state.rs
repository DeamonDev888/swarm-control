//! État global de configuration, initialisé une fois au démarrage.

use super::{dotenv, env, paths, workspace};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub struct Config {
    /// Racine : `.claude/`, `.mcp.json`, `bridge/`.
    pub workspace: PathBuf,
    /// Variables lues dans les `.env` et absentes de l'env système.
    pub dotenv: HashMap<String, String>,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

pub fn init(workspace_override: Option<PathBuf>) -> &'static Config {
    CONFIG.get_or_init(|| {
        let home = paths::home_dir();
        let mut vars = HashMap::new();
        for dir in paths::state_dirs(&home) {
            dotenv::load_into(&dir.join(".env"), &mut vars);
        }
        let workspace = workspace::resolve(workspace_override, &vars, &home);
        let _ = std::fs::create_dir_all(&workspace);

        dotenv::load_into(&workspace.join(".env"), &mut vars);
        if let Some(external) = env::app_lookup_in(&vars, "EXTERNAL_ENV_PATH") {
            dotenv::load_into(Path::new(&external), &mut vars);
        }
        Config { workspace, dotenv: vars }
    })
}

pub fn get() -> &'static Config {
    CONFIG.get().expect("config::init doit être appelé au démarrage")
}

/// Workspace d'un appel : le paramètre `config` de l'outil, sinon le global.
pub fn workspace_for(override_dir: Option<&str>) -> PathBuf {
    match override_dir.map(str::trim).filter(|s| !s.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => get().workspace.clone(),
    }
}

pub(super) fn dotenv_get(name: &str) -> Option<String> {
    CONFIG.get().and_then(|c| c.dotenv.get(name).cloned())
}
