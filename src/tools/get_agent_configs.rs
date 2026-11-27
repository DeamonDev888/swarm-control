//! Outil `get_agent_configs` : lit les fichiers d'un agent (secrets masqués).

use super::errors::opt_runner;
use super::schema::{config_opt, name_schema, runner_opt};
use crate::agents;
use crate::config;
use litemcp::prelude::*;
use litemcp::zod;
use serde::Deserialize;
use serde_json::{Map, Value, json};

const DESCRIPTION: &str =
    "Lit les fichiers de configuration d'un agent (prompt, settings, MCP, env — secrets masqués).";

#[derive(Deserialize)]
struct Args {
    name: String,
    runner: Option<String>,
    config: Option<String>,
}

pub fn register(builder: ServerBuilder) -> ServerBuilder {
    let schema = zod::object()
        .field("name", name_schema("Agent"))
        .opt("runner", runner_opt())
        .opt("config", config_opt())
        .strict();
    builder.tool_parse("get_agent_configs", DESCRIPTION, schema, handle).annotate(
        "get_agent_configs",
        ToolAnnotations::new().title("Lire la config d'un agent").read_only(true),
    )
}

fn handle(a: Args) -> ToolResult {
    let ws = config::workspace_for(a.config.as_deref());
    let files = agents::get_configs(&ws, &a.name, opt_runner(&a.runner)?)
        .map_err(ToolError::invalid_input)?;
    let map: Map<String, Value> = files.into_iter().map(|(k, v)| (k, json!(v))).collect();
    Ok(json!({ "name": a.name, "files": map }))
}
