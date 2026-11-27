//! Arguments d'un run, communs à `run_agent` et `run_agents_parallel`.

use super::errors::parse_runner;
use crate::config;
use crate::routing::Forwarded;
use crate::runners::{RunRequest, RunResult};
use litemcp::prelude::ToolError;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunArgs {
    runner: String,
    prompt: String,
    agent_name: Option<String>,
    session_id: Option<String>,
    auto_resume: Option<bool>,
    model: Option<String>,
    provider: Option<String>,
    path: Option<String>,
    config: Option<String>,
    #[serde(default)]
    pub background: bool,
    pub task_id: Option<String>,
}

impl RunArgs {
    pub fn request(&self) -> Result<RunRequest, ToolError> {
        Ok(RunRequest {
            runner: parse_runner(&self.runner)?,
            prompt: self.prompt.clone(),
            agent: self.agent_name.clone(),
            session_id: self.session_id.clone(),
            // Agent nommé = agent persistant : on reprend sa session sauf demande contraire.
            auto_resume: self.auto_resume.unwrap_or(self.agent_name.is_some()),
            model: self.model.clone(),
            provider: self.provider.clone(),
            cwd: self.path.clone(),
            workspace: config::workspace_for(self.config.as_deref()),
        })
    }
}

/// Résultat d'un run transmis à un hôte : même forme, plus `routedTo`.
pub fn routed_json(req: &RunRequest, routed: &Forwarded, duration: Duration) -> Value {
    let mut v = result_json(req, &routed.result, duration);
    v["routedTo"] = json!({ "instance": routed.host.instance, "url": routed.host.url });
    v
}

/// Résultat d'un run au format JSON de l'outil.
pub fn result_json(req: &RunRequest, result: &RunResult, duration: Duration) -> Value {
    let mut v = json!({
        "runner": req.runner.as_str(),
        "agentName": req.agent,
        "result": result.result,
        "sessionId": result.session_id,
        "durationMs": duration.as_millis() as u64,
    });
    if let Some(m) = &result.model {
        v["model"] = json!(m);
    }
    if let Some(e) = &result.error {
        v["error"] = json!(e);
    }
    v
}
