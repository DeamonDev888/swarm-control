//! Commande `hermes` avec son environnement.

use crate::runners::RunRequest;
use crate::{binaries, config, process};
use std::process::Command;

pub fn build(req: &RunRequest, args: &[String]) -> Command {
    let mut cmd = process::command(&binaries::hermes_binary());
    cmd.args(args).current_dir(req.working_dir());
    for (k, v) in &config::get().dotenv {
        cmd.env(k, v);
    }
    let agent = req.agent.as_deref().unwrap_or("");
    cmd.env("HERMES_HOME", config::native_hermes_home())
        .env("SWARM_AGENT_NAME", agent)
        .env("OVERMIND_AGENT_NAME", agent)
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUNBUFFERED", "1")
        .env("TERM", "emacs")
        .env("PROMPT_TOOLKIT_NO_INTERACTIVE", "1");
    cmd
}
