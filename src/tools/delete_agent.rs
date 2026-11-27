//! Outil `delete_agent` : supprime un agent et sa session.

use super::errors::opt_runner;
use super::schema::{config_opt, name_schema, runner_opt};
use crate::agents;
use crate::config;
use litemcp::prelude::*;
use litemcp::zod;
use serde::Deserialize;
use serde_json::json;

const DESCRIPTION: &str = "Supprime un agent (fichiers, profil Hermes) et sa session sauvegardée. Refusé s'il \
    a un run en cours.";

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
    builder.tool_parse("delete_agent", DESCRIPTION, schema, handle).annotate(
        "delete_agent",
        ToolAnnotations::new().title("Supprimer un agent").destructive(true),
    )
}

fn handle(a: Args) -> ToolResult {
    let ws = config::workspace_for(a.config.as_deref());
    let deleted =
        agents::delete(&ws, &a.name, opt_runner(&a.runner)?).map_err(ToolError::invalid_input)?;
    let mut out = json!({ "deleted": a.name, "files": deleted.files });
    if !deleted.errors.is_empty() {
        out["errors"] = json!(deleted.errors);
    }
    Ok(out)
}
