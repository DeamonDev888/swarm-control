//! Runner Claude Code : `claude -p --output-format json`, prompt sur stdin.

pub(super) mod aliases;
mod args;
pub(super) mod envelope;
mod mcp;
mod persona;
mod plan;
mod retry;
mod settings;
mod temp;

use super::{RunRequest, RunResult};
use crate::registry::LiveRun;
use crate::{binaries, config, process};
use litemcp::cancel::CancelToken;
use plan::Plan;
use process::End;
use std::process::Command;
use std::sync::Arc;

pub fn run(
    req: &RunRequest,
    live: &Arc<LiveRun>,
    resume: Option<&str>,
    cancel: Option<&CancelToken>,
) -> RunResult {
    let plan = match plan::build(req, live, resume) {
        Ok(plan) => plan,
        Err(result) => return result,
    };
    // Jeton primaire d'abord, puis chaque jeton de secours sur 401/429/5xx.
    let tokens: Vec<Option<&String>> =
        std::iter::once(None).chain(plan.fallbacks.iter().map(Some)).collect();
    let mut last = RunResult::default();

    for (i, token) in tokens.iter().enumerate() {
        // Une ligne par commande lancée, secrets masqués (l'environnement n'est pas imprimé).
        crate::debug!(
            "commande {}",
            crate::debug::command_line(&binaries::claude_binary(), &plan.args)
        );
        let output = match process::run(
            command(req, &plan, *token),
            Some(&plan.stdin_prompt),
            live,
            plan.timeout,
            cancel,
        ) {
            Ok(output) => output,
            Err(e) => return spawn_error(&e),
        };
        last = envelope::parse(&output, plan.timeout, plan.model.clone());
        let retryable =
            last.error.is_some() && output.end == End::Exited && retry::is_retryable(&output);
        let more = i + 1 < tokens.len();
        if retryable && more && !live.kill_requested() {
            crate::debug!(
                "retente {} ({}): jeton de secours n°{}",
                req.agent.as_deref().unwrap_or("?"),
                req.runner.as_str(),
                i + 1
            );
            continue;
        }
        if retryable && tokens.len() > 1 {
            last.error = Some(format!(
                "RETRYABLE_ERROR_ALL_FALLBACKS_EXHAUSTED: {}",
                last.error.clone().unwrap_or_default()
            ));
        }
        break;
    }
    last
}

/// Commande `claude` avec l'environnement de l'agent (et un jeton de secours éventuel).
fn command(req: &RunRequest, plan: &Plan, token: Option<&String>) -> Command {
    let mut cmd = process::command(&binaries::claude_binary());
    cmd.args(&plan.args).current_dir(&plan.cwd);
    for (k, v) in &config::get().dotenv {
        cmd.env(k, v);
    }
    for (k, v) in &plan.env {
        if let Some(s) = v.as_str() {
            cmd.env(k, s);
        }
    }
    if let Some(name) = &req.agent {
        cmd.env("SWARM_AGENT_NAME", name).env("OVERMIND_AGENT_NAME", name);
    }
    if let Some(t) = token {
        cmd.env("ANTHROPIC_AUTH_TOKEN", t);
    }
    cmd
}

fn spawn_error(e: &str) -> RunResult {
    RunResult::err(format!(
        "SPAWN_ERROR: {e} (binaire: {}). Définis CLAUDE_BIN si Claude Code est ailleurs.",
        binaries::claude_binary().display()
    ))
}
