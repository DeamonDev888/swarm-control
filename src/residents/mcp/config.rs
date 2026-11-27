//! Outil `config` : les fichiers de configuration de l'agent, secrets masqués.
//!
//! Même lecture que l'outil d'instance `get_agent_configs` (`agents::get_configs`) : un seul
//! chemin de masquage, donc jamais de secret en clair dans une réponse d'outil.

use crate::agents;
use crate::residents::ctx::Ctx;
use litemcp::prelude::*;
use litemcp::zod;
use serde_json::{Value, json};
use std::sync::Arc;

const DESCRIPTION: &str = "Les fichiers de configuration de cet agent (prompt, settings, MCP, env…), \
    secrets masqués. Lecture seule.";

pub fn register(builder: ServerBuilder, ctx: Arc<Ctx>) -> ServerBuilder {
    builder
        .tool_parse("config", DESCRIPTION, zod::object().strict(), move |_: serde_json::Value| {
            handle(&ctx)
        })
        .annotate(
            "config",
            ToolAnnotations::new().title("Configuration de l'agent").read_only(true),
        )
}

fn handle(ctx: &Ctx) -> ToolResult {
    let files = agents::get_configs(&ctx.workspace, &ctx.agent, Some(ctx.runner))
        .map_err(ToolError::invalid_input)?;
    Ok(rendered(&ctx.agent, ctx.runner.as_str(), files))
}

/// Réponse de l'outil : l'ordre des fichiers est celui du runner, jamais retrié.
fn rendered(agent: &str, runner: &str, files: Vec<(String, String)>) -> Value {
    let files: Vec<Value> = files
        .into_iter()
        .map(|(name, content)| json!({ "name": name, "content": content }))
        .collect();
    json!({ "agent": agent, "runner": runner, "files": files })
}

#[cfg(test)]
mod tests {
    use super::{handle, rendered};
    use crate::agents::Runner;
    use crate::residents::ctx::Ctx;
    use serde_json::json;
    use std::sync::Arc;

    fn temp_ctx() -> Arc<Ctx> {
        let ws = std::env::temp_dir().join(format!("swarm-mcp-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ws);
        Arc::new(Ctx::new("alice", Runner::Claude, ws, "test".into()))
    }

    #[test]
    fn files_keep_their_order_and_their_fields() {
        let out = rendered(
            "mina",
            "claude",
            vec![("prompt".into(), "Tu es Mina.".into()), ("settings.json".into(), "{}".into())],
        );
        assert_eq!(out["agent"], json!("mina"));
        assert_eq!(out["runner"], json!("claude"));
        assert_eq!(out["files"][0], json!({ "name": "prompt", "content": "Tu es Mina." }));
        assert_eq!(out["files"][1]["name"], json!("settings.json"));
        assert_eq!(out["files"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn no_file_gives_an_empty_array() {
        let out = rendered("mina", "hermes", Vec::new());
        assert_eq!(out["files"], json!([]));
    }

    #[test]
    fn an_unknown_agent_is_an_invalid_input() {
        let ctx = temp_ctx();
        assert!(handle(&ctx).is_err());
        let _ = std::fs::remove_dir_all(&ctx.workspace);
    }
}
