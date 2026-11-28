//! Runner Antigravity CLI : `agy -p <prompt> --output-format json [--conversation <id>]`.

mod args;
mod envelope;
mod plan;

use super::{RunRequest, RunResult};
use crate::registry::LiveRun;
use crate::{binaries, config, process};
use litemcp::cancel::CancelToken;
use plan::Plan;
use std::process::Command;
use std::sync::Arc;

pub fn run(
    req: &RunRequest,
    live: &Arc<LiveRun>,
    resume: Option<&str>,
    cancel: Option<&CancelToken>,
) -> RunResult {
    let plan = match plan::build(req, resume) {
        Ok(plan) => plan,
        Err(result) => return result,
    };
    // Une ligne par commande lancée, secrets masqués (l'environnement n'est pas imprimé).
    crate::debug!(
        "commande {}",
        crate::debug::command_line(&binaries::antigravity_binary(), &plan.args)
    );
    let output = match process::run(command(req, &plan), None, live, plan.timeout, cancel) {
        Ok(output) => output,
        Err(e) => return spawn_error(&e),
    };
    envelope::parse(&output, plan.timeout, plan.model)
}

fn command(req: &RunRequest, plan: &Plan) -> Command {
    let mut cmd = process::command(&binaries::antigravity_binary());
    cmd.args(&plan.args).current_dir(&plan.cwd);
    for (k, v) in &config::get().dotenv {
        cmd.env(k, v);
    }
    for (k, v) in &plan.env {
        if let Some(s) = v.as_str() {
            cmd.env(k, s);
        }
    }
    cmd.env("SWARM_RUNNER", "antigravity");
    if let Some(name) = &req.agent {
        cmd.env("SWARM_AGENT_NAME", name);
    }
    cmd
}

fn spawn_error(e: &str) -> RunResult {
    RunResult::err(format!(
        "SPAWN_ERROR: {e} (binaire: {}). Définis AGY_BIN (ou ANTIGRAVITY_BIN) si `agy` est ailleurs.",
        binaries::antigravity_binary().display()
    ))
}
