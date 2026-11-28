//! Tâche dont l'agent est hébergé : confiée à son hôte, sinon lancée à la demande.

use super::launch::Finished;
use crate::routing;
use crate::runners::{self, RunRequest};
use crate::tools::run_args::{annotate_verified, result_json, routed_json, run_verifier};
use litemcp::cancel::CancelToken;
use serde_json::Value;
use std::sync::mpsc::Sender;
use std::time::Instant;

/// `prompt` est la demande d'origine : `verify` enchaîne une relecture locale par
/// l'archétype verifier, même quand le tour lui-même est parti chez son hôte.
pub fn start(
    index: usize,
    req: RunRequest,
    task_id: String,
    tx: Sender<Finished>,
    cancel: Option<CancelToken>,
    verify: bool,
    prompt: String,
) -> Result<(), String> {
    std::thread::Builder::new()
        .name(format!("swarm-par-{index}"))
        .spawn(move || {
            let started = Instant::now();
            let (mut json, ok) = match routing::forward(&req) {
                Some(routed) => {
                    (routed_json(&req, &routed, started.elapsed()), routed.result.error.is_none())
                }
                None => on_demand(&req, cancel.as_ref(), started),
            };
            if verify {
                let text = json["result"].as_str().unwrap_or_default().to_string();
                annotate_verified(&mut json, run_verifier(&req.workspace, &prompt, &text));
            }
            json["taskId"] = Value::from(task_id);
            let _ = tx.send(Finished { index, json, ok });
        })
        .map(|_| ())
        .map_err(|e| format!("thread impossible: {e}"))
}

/// Repli : l'hôte a disparu entre-temps, l'agent tourne comme sans résident.
fn on_demand(req: &RunRequest, cancel: Option<&CancelToken>, started: Instant) -> (Value, bool) {
    match runners::prepare(req) {
        Ok(live) => {
            let result = runners::execute(req, &live, cancel);
            (result_json(req, &result, started.elapsed()), result.error.is_none())
        }
        Err(refused) => {
            let error = refused.error.unwrap_or_default();
            let result = runners::RunResult::err(error);
            (result_json(req, &result, started.elapsed()), false)
        }
    }
}
