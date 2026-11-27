//! Outil `update_agent_config` : modifie un agent.

use super::errors::{invalid, opt_runner};
use super::schema::{config_opt, name_schema, runner_opt, str_array};
use crate::agents::{self, Updates};
use crate::config;
use litemcp::prelude::*;
use litemcp::zod;
use serde::Deserialize;
use serde_json::{Map, Value, json};

const DESCRIPTION: &str = "Modifie un agent : model, mcpServers, env (clé→valeur), prompt — ou réécrit un \
    fichier entier (file: prompt.md | settings.json | .mcp.json | skill.md, avec content).";

const FILES: &[&str] = &["prompt.md", "settings.json", ".mcp.json", "skill.md"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Args {
    name: String,
    runner: Option<String>,
    model: Option<String>,
    mcp_servers: Option<Vec<String>>,
    env: Option<Map<String, Value>>,
    prompt: Option<String>,
    file: Option<String>,
    content: Option<String>,
    config: Option<String>,
}

pub fn register(builder: ServerBuilder) -> ServerBuilder {
    let schema = zod::object()
        .field("name", name_schema("Agent"))
        .opt("runner", runner_opt())
        .opt("model", zod::string().min(1))
        .opt("mcpServers", str_array("Serveurs MCP (noms de .mcp.json)"))
        .opt("env", zod::describe("Variables d'environnement à écrire", zod::record(zod::string())))
        .opt("prompt", zod::describe("Nouveau prompt système", zod::string().min(1)))
        .opt("file", zod::enumeration(FILES))
        .opt("content", zod::describe("Contenu complet du fichier (avec file)", zod::string()))
        .opt("config", config_opt())
        .strict();
    builder.tool_parse("update_agent_config", DESCRIPTION, schema, handle)
}

fn handle(a: Args) -> ToolResult {
    let ws = config::workspace_for(a.config.as_deref());
    let file = match (a.file, a.content) {
        (Some(f), Some(c)) => Some((f, c)),
        (None, None) => None,
        _ => return Err(invalid("file et content vont ensemble")),
    };
    let up =
        Updates { model: a.model, mcp_servers: a.mcp_servers, env: a.env, prompt: a.prompt, file };
    let changes = agents::update(&ws, &a.name, opt_runner(&a.runner)?, &up)
        .map_err(ToolError::invalid_input)?;
    Ok(json!({ "agent": a.name, "changes": changes }))
}
