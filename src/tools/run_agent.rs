//! Outil `run_agent` : lance un agent, en direct ou en arrière-plan.

use super::run_args::{RunArgs, result_json, routed_json};
use super::schema::run_object;
use crate::registry::{LiveRun, Outcome, Status};
use crate::routing;
use crate::runners::{self, RunRequest};
use litemcp::prelude::*;
use litemcp::zod;
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

const DESCRIPTION: &str = "Lance un agent persistant (Claude Code ou Hermes). Avec agentName, la session \
    précédente est reprise automatiquement (autoResume) et la nouvelle est sauvegardée : l'agent garde sa \
    mémoire de conversation d'un appel à l'autre. background:true rend la main tout de suite avec un runId \
    à suivre via agent_control. Un agent résident (agent_start) est servi par son hôte, quelle que soit \
    l'instance : la demande entre dans sa file (champ routedTo).";

pub fn register(builder: ServerBuilder) -> ServerBuilder {
    let schema = run_object()
        .opt("background", zod::describe("Ne pas attendre : renvoie un runId", zod::boolean()))
        .strict();
    builder.tool_parse("run_agent", DESCRIPTION, schema, handle)
}

fn handle(args: RunArgs) -> ToolResult {
    let req = args.request()?;
    if !args.background {
        let started = Instant::now();
        if let Some(routed) = routing::forward(&req) {
            let out = routed_json(&req, &routed, started.elapsed());
            return match &routed.result.error {
                Some(e) => Err(ToolError::upstream(format!("{e}\n{out}"))),
                None => Ok(out),
            };
        }
    }
    let live = runners::prepare(&req)
        .map_err(|r| ToolError::new(ErrorCode::InvalidInput, r.error.unwrap_or_default()))?;
    if args.background {
        return spawn_background(req, live);
    }
    let started = Instant::now();
    let cancel = litemcp::cancel::token();
    let result = runners::execute(&req, &live, cancel.as_ref());
    let out = result_json(&req, &result, started.elapsed());
    match &result.error {
        Some(e) => Err(ToolError::upstream(format!("{e}\n{out}"))),
        None => Ok(out),
    }
}

/// Lance le run dans un fil et rend un `runId` immédiatement.
fn spawn_background(req: RunRequest, live: Arc<LiveRun>) -> ToolResult {
    let summary = json!({
        "runId": live.run_id,
        "agentName": req.agent,
        "runner": req.runner.as_str(),
        "status": "running",
        "hint": "suivre avec agent_control (status | stream | wait | kill)",
    });
    let for_thread = Arc::clone(&live);
    let spawned =
        std::thread::Builder::new().name(format!("swarm-run-{}", live.run_id)).spawn(move || {
            runners::execute(&req, &for_thread, None);
        });
    if let Err(e) = spawned {
        // Sans cela le run resterait « running » à jamais, et son bail avec lui.
        let error = format!("thread de run impossible: {e}");
        live.finish(
            Status::Failed,
            None,
            Outcome { result: String::new(), error: Some(error.clone()) },
        );
        return Err(ToolError::internal(error));
    }
    Ok(summary)
}
