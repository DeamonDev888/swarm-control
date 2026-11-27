//! Plan de lancement d'un run Claude : arguments, environnement, délai.

use super::args;
use super::settings::{self, AgentSetup};
use super::temp::TempFiles;
use super::{aliases, retry};
use crate::config;
use crate::registry::LiveRun;
use crate::runners::{RunRequest, RunResult};
use serde_json::{Map, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub struct Plan {
    pub args: Vec<String>,
    pub stdin_prompt: String,
    pub env: Map<String, Value>,
    pub timeout: Duration,
    pub model: Option<String>,
    pub cwd: PathBuf,
    pub fallbacks: Vec<String>,
    /// Supprimés à la destruction du plan, donc après le run.
    _temps: TempFiles,
}

pub fn build(
    req: &RunRequest,
    live: &Arc<LiveRun>,
    resume: Option<&str>,
) -> Result<Plan, RunResult> {
    let mut temps = TempFiles::default();
    let setup = match &req.agent {
        Some(name) => settings::for_agent(&req.workspace, name, &live.run_id, &mut temps)
            .map_err(RunResult::err)?,
        None => settings::for_anonymous(&req.workspace),
    };
    let model = pick_model(req, &setup);
    let (args, stdin_prompt) = args::build(req, &setup, model.as_deref(), resume);
    Ok(Plan {
        args,
        stdin_prompt,
        fallbacks: retry::fallbacks(&setup.env),
        timeout: setup.timeout.unwrap_or_else(config::default_timeout),
        cwd: req.working_dir(),
        env: setup.env,
        model,
        _temps: temps,
    })
}

/// Modèle demandé, sinon `ANTHROPIC_MODEL` de l'agent, résolu par les alias.
fn pick_model(req: &RunRequest, setup: &AgentSetup) -> Option<String> {
    req.model
        .clone()
        .or_else(|| setup.env.get("ANTHROPIC_MODEL").and_then(Value::as_str).map(String::from))
        .filter(|m| !m.is_empty())
        .map(|m| aliases::resolve_model(&req.workspace, &m))
}
