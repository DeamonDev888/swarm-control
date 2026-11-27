//! Fichier MCP propre à un agent Claude : `.mcp.<nom>.json`.

use crate::agents::files::io;
use crate::agents::mcp::global_mcp_servers;
use crate::agents::paths::claude_mcp_path;
use crate::store::write_json_atomic;
use serde_json::{Map, Value, json};
use std::path::Path;

/// Écrit `.mcp.<nom>.json` avec les seuls `servers` demandés (connus du `.mcp.json`).
pub fn write_agent_mcp(ws: &Path, name: &str, servers: &[String]) -> Result<(), String> {
    let all = global_mcp_servers(ws);
    let picked: Map<String, Value> =
        servers.iter().filter_map(|s| all.get(s).map(|cfg| (s.clone(), cfg.clone()))).collect();
    write_json_atomic(&claude_mcp_path(ws, name), &json!({ "mcpServers": picked })).map_err(io)
}
