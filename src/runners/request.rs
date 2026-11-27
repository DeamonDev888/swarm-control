//! Demande d'exécution d'un agent.

use crate::agents::Runner;
use std::path::PathBuf;

pub struct RunRequest {
    pub runner: Runner,
    pub prompt: String,
    pub agent: Option<String>,
    pub session_id: Option<String>,
    pub auto_resume: bool,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub cwd: Option<String>,
    pub workspace: PathBuf,
}

impl RunRequest {
    /// Répertoire d'exécution : `path` demandé, sinon le workspace.
    pub fn working_dir(&self) -> PathBuf {
        self.cwd.as_deref().map_or_else(|| self.workspace.clone(), PathBuf::from)
    }
}
