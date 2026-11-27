//! Runner Hermes : `hermes [-p profil] chat --query-file - -Q --yolo …`.
//!
//! Le prompt passe par stdin (aucun souci de guillemets ni de longueur) ;
//! l'identifiant de session sort sur stderr (`session_id: …`).

mod args;
mod command;
mod hints;
mod outcome;
mod session_id;

use super::{RunRequest, RunResult};
use crate::registry::LiveRun;
use crate::{binaries, config, process};
use litemcp::cancel::CancelToken;
use std::sync::Arc;

pub fn run(
    req: &RunRequest,
    live: &Arc<LiveRun>,
    resume: Option<&str>,
    cancel: Option<&CancelToken>,
) -> RunResult {
    let timeout = config::default_timeout();
    let cmd = command::build(req, &args::build(req, resume));
    match process::run(cmd, Some(&req.prompt), live, timeout, cancel) {
        Ok(output) => outcome::from_output(req, &output, timeout, resume),
        Err(e) => RunResult::err(format!(
            "SPAWN_ERROR: {e} (binaire: {}). Définis HERMES_BIN si Hermes est ailleurs.",
            binaries::hermes_binary().display()
        )),
    }
}
