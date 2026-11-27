//! Outil `stop` : interrompre l'agent sans l'arrêter.

use crate::residents::ctx::Ctx;
use litemcp::prelude::*;
use litemcp::zod;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

const DESCRIPTION: &str = "Tue le tour en cours (arbre de processus). clearQueue:true annule aussi \
    les demandes en attente. L'agent reste résident et accepte de nouvelles demandes.";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Args {
    #[serde(default)]
    clear_queue: bool,
}

pub fn register(builder: ServerBuilder, ctx: Arc<Ctx>) -> ServerBuilder {
    let schema = zod::object()
        .opt("clearQueue", zod::describe("Annuler aussi les demandes en attente", zod::boolean()))
        .strict();
    builder
        .tool_parse("stop", DESCRIPTION, schema, move |args: Args| {
            let stopped = ctx.kill_current();
            let cleared = if args.clear_queue {
                ctx.clear_queue("CANCELLED: file vidée par stop.")
            } else {
                0
            };
            Ok(json!({ "agent": ctx.agent, "stopped": stopped, "cleared": cleared }))
        })
        .annotate("stop", ToolAnnotations::new().title("Interrompre l'agent").destructive(true))
}
