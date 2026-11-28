//! Outil `run_agent` : lance un agent, en direct ou en arrière-plan.

use super::errors::invalid;
use super::run_args::{
    RunArgs, annotate_verified, result_json, routed_json, run_verifier, with_archetype,
};
use super::schema::run_object;
use crate::registry::{LiveRun, Outcome, Status};
use crate::routing;
use crate::runners::{self, RunRequest};
use litemcp::prelude::*;
use litemcp::zod;
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

const DESCRIPTION: &str = "Lance un agent persistant (Claude Code, Hermes ou Antigravity). Avec agentName, la session \
    précédente est reprise automatiquement (autoResume) et la nouvelle est sauvegardée : l'agent garde sa \
    mémoire de conversation d'un appel à l'autre. SANS agentName, l'agent est éphémère : un tour, aucune \
    session gardée. archetype lance un tour éphémère avec la persona d'un modèle (ex. coder) : sans \
    agentName ni sessionId, rien de gardé ; explorer et verifier tournent en lecture seule. verify:true \
    fait relire le résultat par l'archétype verifier (champ verified). background:true rend la main tout \
    de suite avec un runId à suivre via agent_control (verify indisponible). Un agent résident \
    (agent_start) est servi par son hôte, quelle que soit l'instance : la demande entre dans sa file \
    (champ routedTo).";

pub fn register(builder: ServerBuilder) -> ServerBuilder {
    let schema = run_object()
        .opt(
            "archetype",
            zod::describe(
                "Persona d'un modèle (ex. coder) pour un tour éphémère, sans état ni agentName",
                zod::string().min(1),
            ),
        )
        .opt(
            "verify",
            zod::describe(
                "Fait relire le résultat par l'archétype verifier (champ verified)",
                zod::boolean(),
            ),
        )
        .opt("background", zod::describe("Ne pas attendre : renvoie un runId", zod::boolean()))
        .strict();
    builder.tool_parse("run_agent", DESCRIPTION, schema, handle)
}

fn handle(args: RunArgs) -> ToolResult {
    if args.verify && args.background {
        return Err(invalid(
            "verify:true exige un tour attendu (sans background:true) : la relecture doit annoter la réponse.",
        ));
    }
    let req = args.request()?;
    let archetype = args.archetype.clone();
    let verify = args.verify;
    if !args.background {
        let started = Instant::now();
        if let Some(routed) = routing::forward(&req) {
            let out = routed_json(&req, &routed, started.elapsed());
            return match &routed.result.error {
                Some(e) => Err(ToolError::upstream(format!("{e}\n{out}"))),
                None => Ok(out),
            };
        }
        let live = runners::prepare(&req)
            .map_err(|r| ToolError::new(ErrorCode::InvalidInput, r.error.unwrap_or_default()))?;
        let cancel = litemcp::cancel::token();
        let result =
            with_archetype(archetype.as_deref(), || runners::execute(&req, &live, cancel.as_ref()));
        let mut out = result_json(&req, &result, started.elapsed());
        if verify {
            annotate_verified(
                &mut out,
                run_verifier(&req.workspace, args.prompt_text(), &result.result),
            );
        }
        return match &result.error {
            Some(e) => Err(ToolError::upstream(format!("{e}\n{out}"))),
            None => Ok(out),
        };
    }
    let live = runners::prepare(&req)
        .map_err(|r| ToolError::new(ErrorCode::InvalidInput, r.error.unwrap_or_default()))?;
    spawn_background(req, live, archetype)
}

/// Lance le run dans un fil et rend un `runId` immédiatement.
fn spawn_background(req: RunRequest, live: Arc<LiveRun>, archetype: Option<String>) -> ToolResult {
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
            with_archetype(archetype.as_deref(), || runners::execute(&req, &for_thread, None));
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
