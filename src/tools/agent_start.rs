//! Outil `agent_start` : rend un agent résident dans cette instance.

use super::errors::opt_runner;
use super::schema::{config_opt, name_schema, runner_opt};
use crate::{agents, config, residents};
use litemcp::prelude::*;
use litemcp::zod;
use serde::Deserialize;

const DESCRIPTION: &str = "Rend un agent RÉSIDENT : il est chargé en mémoire avec son propre serveur MCP \
    (annoncé sous swarm-<agent>), une file de demandes et un fil de travail. Rend son URL ; ses outils \
    sont ask, status, stop. Un seul process héberge un agent à la fois.";

#[derive(Deserialize)]
struct Args {
    name: String,
    runner: Option<String>,
    config: Option<String>,
}

pub fn register(builder: ServerBuilder) -> ServerBuilder {
    let schema = zod::object()
        .field("name", name_schema("Agent à rendre résident"))
        .opt("runner", runner_opt())
        .opt("config", config_opt())
        .strict();
    builder.tool_parse("agent_start", DESCRIPTION, schema, handle)
}

fn handle(a: Args) -> ToolResult {
    let ws = config::workspace_for(a.config.as_deref());
    let runner =
        agents::resolve(&ws, &a.name, opt_runner(&a.runner)?).map_err(ToolError::invalid_input)?;
    let info = residents::start(&ws, runner, &a.name).map_err(ToolError::invalid_input)?;
    Ok(info.to_json())
}
