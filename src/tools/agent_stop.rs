//! Outil `agent_stop` : arrête la résidence d'un agent hébergé par cette instance.

use super::errors::opt_runner;
use super::schema::{config_opt, name_schema, runner_opt};
use crate::{config, residents};
use litemcp::prelude::*;
use litemcp::zod;
use serde::Deserialize;
use serde_json::json;

const DESCRIPTION: &str = "Arrête un agent résident hébergé par cette instance : tour en cours tué, file \
    vidée (chaque demandeur reçoit une erreur), serveur fermé, annonce retirée. L'agent reste défini \
    sur disque et sa session est conservée.";

#[derive(Deserialize)]
struct Args {
    name: String,
    runner: Option<String>,
    config: Option<String>,
}

pub fn register(builder: ServerBuilder) -> ServerBuilder {
    let schema = zod::object()
        .field("name", name_schema("Agent résident à arrêter"))
        .opt("runner", runner_opt())
        .opt("config", config_opt())
        .strict();
    builder
        .tool_parse("agent_stop", DESCRIPTION, schema, handle)
        .annotate("agent_stop", ToolAnnotations::new().title("Arrêter un agent résident"))
}

fn handle(a: Args) -> ToolResult {
    let ws = config::workspace_for(a.config.as_deref());
    let info =
        residents::stop(&ws, &a.name, opt_runner(&a.runner)?).map_err(ToolError::invalid_input)?;
    Ok(json!({ "stopped": info.to_json() }))
}
