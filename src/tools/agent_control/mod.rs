//! Outil `agent_control` : status / stream / wait / kill / list des runs.
//!
//! Un run absent de la mémoire (serveur redémarré depuis) est cherché dans le
//! registre disque (`disk.rs`).

mod args;
mod disk;
mod find;
mod kill;
mod list;
mod snapshot;
mod stream;
mod wait;

use super::errors::invalid;
use super::schema::{config_opt, name_schema, runner_opt};
use crate::config;
use args::ControlArgs;
use litemcp::prelude::*;
use litemcp::zod;

const DESCRIPTION: &str = "Contrôle les runs : status (état + PID + session), stream (sortie en direct, \
    since=timestamp), wait (bloque jusqu'à la fin), kill (arrêt de l'arbre de processus), list (tous les runs).";

pub fn register(builder: ServerBuilder) -> ServerBuilder {
    let schema = zod::object()
        .field("action", zod::enumeration(&["status", "stream", "wait", "kill", "list"]))
        .opt("agentName", name_schema("Agent ciblé"))
        .opt(
            "runId",
            zod::describe("Run ciblé (rendu par run_agent background:true)", zod::string().min(1)),
        )
        .opt("runner", runner_opt())
        .opt(
            "timeoutMs",
            zod::describe(
                "Délai de wait en ms (défaut 900000, max 3600000)",
                zod::uint().min(1000).max(3_600_000),
            ),
        )
        .opt(
            "sinceTimestamp",
            zod::describe("stream : sortie après ce timestamp (ms)", zod::uint()),
        )
        .opt("config", config_opt())
        .strict();
    builder
        .tool_parse("agent_control", DESCRIPTION, schema, handle)
        .annotate("agent_control", ToolAnnotations::new().title("Contrôler les runs"))
}

fn handle(args: ControlArgs) -> ToolResult {
    if args.action == "list" {
        return Ok(list::all());
    }
    let workspace = config::workspace_for(args.config.as_deref());
    let Some(run) = find::find_run(&args)? else {
        return disk::from_disk(&args, &workspace);
    };
    match args.action.as_str() {
        "status" => Ok(snapshot::to_json(&run.snapshot())),
        "stream" => Ok(stream::stream(&run, &args)),
        "kill" => Ok(kill::kill(&run)),
        "wait" => wait::wait(&run, &args),
        other => Err(invalid(format!(
            "action inconnue '{other}' (status | stream | kill | wait | list)"
        ))),
    }
}
