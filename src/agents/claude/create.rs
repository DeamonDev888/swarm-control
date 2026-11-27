//! Création d'un agent Claude : prompt, settings et fichier MCP.

use super::env::{copy_from, default_env, has_credentials};
use super::mcp::write_agent_mcp;
use crate::agents::files::io;
use crate::agents::mcp::mcp_server_names;
use crate::agents::paths::{claude_prompt_path, claude_settings_path};
use crate::agents::{CreateArgs, Created};
use crate::config;
use crate::store::write_json_atomic;
use serde_json::json;
use std::path::Path;

const DEFAULT_MODEL: &str = "claude-sonnet-4-6";

pub fn create(ws: &Path, args: &CreateArgs) -> Result<Created, String> {
    let model = args
        .model
        .map(String::from)
        .or_else(|| config::var("ANTHROPIC_MODEL"))
        .unwrap_or_else(|| DEFAULT_MODEL.into());

    let mut env = default_env(&model, args.name);
    let mut servers = mcp_server_names(ws);
    if let Some(rel) = args.copy_env_from {
        let copied = copy_from(ws, rel)?;
        env.extend(copied.env);
        servers = copied.servers.unwrap_or(servers);
    }
    env.insert("ANTHROPIC_MODEL".into(), json!(model));

    // Sans jeton, l'agent ne pourrait pas s'authentifier : on refuse plutôt
    // que de créer un agent inutilisable.
    if !has_credentials(&env) {
        return Err("MISSING_AUTH_TOKEN: ANTHROPIC_AUTH_TOKEN (ou ANTHROPIC_AUTH_TOKEN_<N>) absent \
                    de l'environnement et des .env — ou passe copyEnvFrom vers un settings existant."
            .into());
    }

    let prompt_path = claude_prompt_path(ws, args.name);
    std::fs::create_dir_all(prompt_path.parent().expect("agents/ a un parent")).map_err(io)?;
    std::fs::write(&prompt_path, args.prompt).map_err(io)?;

    let settings = json!({
        "env": env,
        "enableAllProjectMcpServers": false,
        "enabledMcpjsonServers": servers,
        "agent": args.name,
        "runner": "claude",
    });
    let config_path = claude_settings_path(ws, args.name);
    write_json_atomic(&config_path, &settings).map_err(io)?;
    write_agent_mcp(ws, args.name, &servers)?;
    Ok(Created { prompt_path, config_path, warnings: Vec::new() })
}
