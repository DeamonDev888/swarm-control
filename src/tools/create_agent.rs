//! Outil `create_agent` : crée un agent persistant (et le rend résident si demandé).

use super::errors::{invalid, parse_runner};
use super::schema::{config_opt, name_schema, runner_opt};
use crate::agents::{self, CreateArgs, Runner};
use crate::{config, residents};
use litemcp::prelude::*;
use litemcp::zod;
use serde::Deserialize;
use serde_json::json;
use std::path::Path;

const DESCRIPTION: &str = "Crée un agent persistant. Claude : .claude/agents/<nom>.md + settings_<nom>.json + \
    .mcp.<nom>.json. Hermes : profil natif (hermes profile create) + SOUL.md. Le prompt définit sa persona. \
    resident:true le rend aussitôt résident (serveur MCP propre, file de demandes).";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Args {
    name: String,
    prompt: String,
    runner: Option<String>,
    model: Option<String>,
    copy_env_from: Option<String>,
    clone_from: Option<String>,
    #[serde(default)]
    resident: bool,
    config: Option<String>,
}

pub fn register(builder: ServerBuilder) -> ServerBuilder {
    let schema = zod::object()
        .field("name", name_schema("Nom unique [a-zA-Z0-9_-] (Hermes : minuscules uniquement)"))
        .field("prompt", zod::describe("Prompt système (persona, missions, outils autorisés)", zod::string().min(1)))
        .opt("runner", runner_opt())
        .opt("model", zod::describe("Modèle ou surnom (voir model-aliases.json)", zod::string().min(1)))
        .opt("copyEnvFrom", zod::describe("Claude : settings.json existant dont copier env + serveurs MCP", zod::string().min(1)))
        .opt("cloneFrom", zod::describe("Hermes : profil existant dont copier config, .env, SOUL, skills (les credentials sont par profil)", zod::string().min(1)))
        .opt("resident", zod::describe("Rendre l'agent résident tout de suite", zod::boolean()))
        .opt("config", config_opt())
        .strict();
    builder.tool_parse("create_agent", DESCRIPTION, schema, handle)
}

fn handle(a: Args) -> ToolResult {
    let ws = config::workspace_for(a.config.as_deref());
    let runner = a.runner.as_deref().map(parse_runner).transpose()?.unwrap_or(Runner::Claude);
    if agents::exists_for(&ws, &a.name, runner) {
        return Err(invalid(format!(
            "L'agent '{}' existe déjà ({}). update_agent_config pour le modifier, ou delete_agent d'abord.",
            a.name,
            runner.as_str()
        )));
    }
    let created = agents::create(
        &ws,
        &CreateArgs {
            name: &a.name,
            prompt: &a.prompt,
            runner,
            model: a.model.as_deref(),
            copy_env_from: a.copy_env_from.as_deref(),
            clone_from: a.clone_from.as_deref(),
        },
    )
    .map_err(ToolError::invalid_input)?;

    let mut out = json!({
        "created": a.name,
        "runner": runner.as_str(),
        "promptPath": created.prompt_path.display().to_string(),
        "configPath": created.config_path.display().to_string(),
        "next": format!("run_agent(runner:\"{}\", agentName:\"{}\", prompt:\"…\")", runner.as_str(), a.name),
    });
    if !created.warnings.is_empty() {
        out["warnings"] = json!(created.warnings);
    }
    if a.resident {
        add_residency(&mut out, &ws, runner, &a.name);
    }
    Ok(out)
}

/// L'agent est créé quoi qu'il arrive : un échec de résidence est signalé, pas fatal.
fn add_residency(out: &mut serde_json::Value, ws: &Path, runner: Runner, name: &str) {
    match residents::start(ws, runner, name) {
        Ok(info) => out["resident"] = info.to_json(),
        Err(e) => out["residentError"] = json!(e),
    }
}
