//! Création d'un agent : aiguillage vers le runner.

use super::runner::Runner;
use super::{claude, hermes};
use std::path::{Path, PathBuf};

pub struct CreateArgs<'a> {
    pub name: &'a str,
    pub prompt: &'a str,
    pub runner: Runner,
    pub model: Option<&'a str>,
    /// Claude : settings existant dont copier `env` et serveurs MCP.
    pub copy_env_from: Option<&'a str>,
    /// Hermes : profil source dont copier config.yaml, .env, SOUL.md et skills.
    pub clone_from: Option<&'a str>,
}

pub struct Created {
    pub prompt_path: PathBuf,
    pub config_path: PathBuf,
    pub warnings: Vec<String>,
}

pub fn create(ws: &Path, args: &CreateArgs) -> Result<Created, String> {
    super::name::validate_name(args.name)?;
    match args.runner {
        Runner::Claude => claude::create::create(ws, args),
        Runner::Hermes => hermes::create::create(ws, args),
    }
}
