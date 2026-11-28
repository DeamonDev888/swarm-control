//! Lancement d'un fil par tâche.

use super::Indexed;
use crate::registry::{Outcome, Status};
use crate::routing;
use crate::runners;
use crate::tools::run_args::{
    RunArgs, annotate_verified, result_json, run_verifier, with_archetype,
};
use litemcp::cancel::CancelToken;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::Instant;

/// Une tâche terminée.
pub struct Finished {
    pub index: usize,
    pub json: Value,
    pub ok: bool,
}

pub struct Launched {
    /// Tâches refusées avant lancement (nom invalide, agent occupé…).
    pub immediate: Vec<Indexed>,
    /// Nombre de fils démarrés.
    pub count: usize,
}

pub fn all(tasks: Vec<RunArgs>, tx: &Sender<Finished>) -> Launched {
    let cancel = litemcp::cancel::token();
    let mut launched = Launched { immediate: Vec::new(), count: 0 };
    for (index, args) in tasks.into_iter().enumerate() {
        let task_id = args.task_id.clone().unwrap_or_else(|| format!("task_{}", index + 1));
        match start_one(index, &task_id, &args, tx, cancel.clone()) {
            Ok(()) => launched.count += 1,
            Err(message) => {
                launched.immediate.push((index, json!({ "taskId": task_id, "error": message })));
            }
        }
    }
    launched
}

fn start_one(
    index: usize,
    task_id: &str,
    args: &RunArgs,
    tx: &Sender<Finished>,
    cancel: Option<CancelToken>,
) -> Result<(), String> {
    let req = args.request().map_err(|e| e.message)?;
    let (tx, task_id) = (tx.clone(), task_id.to_string());
    if routing::host_of(&req).is_some() {
        // Agent hébergé : pas d'archétype possible (tour éphémère sans agentName),
        // mais verify s'applique : la relecture reste locale à cette instance.
        return super::routed::start(
            index,
            req,
            task_id,
            tx,
            cancel,
            args.verify,
            args.prompt_text().to_string(),
        );
    }
    let live = runners::prepare(&req).map_err(|r| r.error.unwrap_or_default())?;
    let archetype = args.archetype.clone();
    let verify = args.verify;
    let prompt = args.prompt_text().to_string();
    let ws = req.workspace.clone();
    let for_thread = Arc::clone(&live);
    std::thread::Builder::new()
        .name(format!("swarm-par-{index}"))
        .spawn(move || {
            let started = Instant::now();
            let result = with_archetype(archetype.as_deref(), || {
                runners::execute(&req, &for_thread, cancel.as_ref())
            });
            let mut json = result_json(&req, &result, started.elapsed());
            if verify {
                annotate_verified(&mut json, run_verifier(&ws, &prompt, &result.result));
            }
            json["taskId"] = Value::from(task_id);
            let _ = tx.send(Finished { index, json, ok: result.error.is_none() });
        })
        .map(|_| ())
        .map_err(|e| {
            // Sans cela le run resterait « running » à jamais, et son bail avec lui.
            let error = format!("thread impossible: {e}");
            live.finish(
                Status::Failed,
                None,
                Outcome { result: String::new(), error: Some(error.clone()) },
            );
            error
        })
}
