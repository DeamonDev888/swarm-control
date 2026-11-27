//! Outil `list_agents` : liste les agents et leur état.

use super::schema::config_opt;
use crate::agents::{self, AgentInfo};
use crate::registry::{self, Status};
use crate::{config, owners, sessions};
use litemcp::prelude::*;
use litemcp::zod;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

const DESCRIPTION: &str = "Liste les agents (Claude, Hermes et Antigravity) avec leur état de session, leur run en cours \
    et, s'ils sont résidents, l'instance et l'URL qui les héberge. details:true ajoute modèle, MCP, \
    taille du prompt.";

#[derive(Deserialize)]
struct Args {
    #[serde(default)]
    details: bool,
    config: Option<String>,
}

pub fn register(builder: ServerBuilder) -> ServerBuilder {
    let schema = zod::object().opt("details", zod::boolean()).opt("config", config_opt()).strict();
    builder
        .tool_parse("list_agents", DESCRIPTION, schema, handle)
        .annotate("list_agents", ToolAnnotations::new().title("Lister les agents").read_only(true))
}

fn handle(a: Args) -> ToolResult {
    let ws = config::workspace_for(a.config.as_deref());
    let items: Vec<Value> = agents::list(&ws).iter().map(|i| describe(&ws, i, a.details)).collect();
    Ok(json!({ "count": items.len(), "agents": items }))
}

fn describe(ws: &Path, info: &AgentInfo, details: bool) -> Value {
    let runner = info.runner.as_str();
    let session = sessions::get(ws, runner, &info.name);
    let running = registry::find(&info.name, Some(runner))
        .is_some_and(|r| r.snapshot().status == Status::Running);
    let mut v = json!({
        "name": info.name,
        "runner": runner,
        "hasSession": session.is_some(),
        "running": running,
    });
    if let Some(host) = owners::host_of(ws, runner, &info.name) {
        v["resident"] = json!({ "instance": host.instance, "url": host.url });
    }
    if details {
        v["model"] = json!(info.model);
        v["mcpServers"] = json!(info.mcp_servers);
        v["promptBytes"] = json!(info.prompt_bytes);
        v["sessionId"] = json!(session);
        if info.missing_config {
            v["warning"] = json!("configuration incomplète");
        }
    }
    v
}
