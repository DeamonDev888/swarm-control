//! Plan de lancement d'un run Antigravity : arguments, environnement, délai.

use super::args;
use crate::agents::{antigravity_prompt_path, antigravity_settings_path};
use crate::config;
use crate::runners::claude::aliases;
use crate::runners::{RunRequest, RunResult};
use crate::store::read_json;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Plan {
    pub args: Vec<String>,
    pub env: Map<String, Value>,
    pub timeout: Duration,
    pub model: Option<String>,
    pub cwd: PathBuf,
}

pub fn build(req: &RunRequest, resume: Option<&str>) -> Result<Plan, RunResult> {
    let (settings, persona) = match &req.agent {
        Some(name) => (load_settings(&req.workspace, name)?, load_persona(&req.workspace, name)),
        None => (None, None),
    };
    let env = extract_env(settings.as_ref());
    let model = pick_model(req, settings.as_ref(), &env);
    let timeout = pick_timeout(&env).unwrap_or_else(config::default_timeout);
    let args = args::build(req, persona.as_deref(), model.as_deref(), resume);
    Ok(Plan { args, env, timeout, model, cwd: req.working_dir() })
}

fn load_settings(ws: &Path, name: &str) -> Result<Option<Value>, RunResult> {
    let path = antigravity_settings_path(ws, name);
    if !path.is_file() {
        return Err(RunResult::err(format!(
            "Agent Antigravity '{name}' introuvable ({}).",
            path.display()
        )));
    }
    let raw = read_json(&path).ok_or_else(|| {
        RunResult::err(format!("settings_{name}.json invalide ({}).", path.display()))
    })?;
    Ok(Some(config::interpolate_json(&raw)))
}

fn load_persona(ws: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(antigravity_prompt_path(ws, name)).ok().filter(|p| !p.trim().is_empty())
}

fn extract_env(settings: Option<&Value>) -> Map<String, Value> {
    settings.and_then(|s| s.get("env")).and_then(Value::as_object).cloned().unwrap_or_default()
}

fn pick_model(
    req: &RunRequest,
    settings: Option<&Value>,
    env: &Map<String, Value>,
) -> Option<String> {
    req.model
        .clone()
        .or_else(|| settings.and_then(|s| s.get("model")).and_then(Value::as_str).map(String::from))
        .or_else(|| env.get("AGY_MODEL").and_then(Value::as_str).map(String::from))
        .or_else(|| env.get("ANTIGRAVITY_MODEL").and_then(Value::as_str).map(String::from))
        .filter(|m| !m.is_empty())
        .map(|m| aliases::resolve_model(&req.workspace, &m))
}

fn pick_timeout(env: &Map<String, Value>) -> Option<Duration> {
    env.get("AGENT_TIMEOUT_MS")
        .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
        .filter(|&ms| ms > 0)
        .map(Duration::from_millis)
}

#[cfg(test)]
mod tests {
    use super::build;
    use crate::agents::{self, CreateArgs, Runner};
    use crate::runners::RunRequest;
    use std::time::Duration;

    #[test]
    fn plan_resolves_model_alias_and_agent_timeout() {
        let ws = std::env::temp_dir().join(format!("swarm-agy-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ws);
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(
            ws.join("model-aliases.json"),
            r#"{"neon":"gemini-4-neon-medium"}"#,
        )
        .unwrap();
        agents::create(
            &ws,
            &CreateArgs {
                name: "pbot",
                prompt: "Tu es PBot.",
                runner: Runner::Antigravity,
                model: Some("neon"),
                copy_env_from: None,
                clone_from: None,
            },
        )
        .unwrap();
        // Le profil dédié (PLAN §4.4) est l'emplacement d'écriture des nouveaux agents.
        let settings = agents::antigravity_settings_path(&ws, "pbot");
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(
            settings,
            r#"{"model":"neon","env":{"AGENT_TIMEOUT_MS":"42000"},"agent":"pbot","runner":"antigravity"}"#,
        )
        .unwrap();

        let req = RunRequest {
            runner: Runner::Antigravity,
            prompt: "ping".into(),
            agent: Some("pbot".into()),
            session_id: None,
            auto_resume: true,
            model: None,
            provider: None,
            cwd: None,
            workspace: ws.clone(),
        };
        let plan = build(&req, None).unwrap();
        assert_eq!(plan.model.as_deref(), Some("gemini-4-neon-medium"));
        assert_eq!(plan.timeout, Duration::from_millis(42_000));
        assert!(plan.args[1].contains("Tu es PBot."));
        let _ = std::fs::remove_dir_all(&ws);
    }
}
