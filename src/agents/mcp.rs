//! Serveurs MCP déclarés dans le `.mcp.json` du workspace.

use super::paths::global_mcp_path;
use crate::store::read_json;
use serde_json::{Map, Value};
use std::path::Path;

/// Configuration complète des serveurs (`mcpServers`), vide si absente.
pub fn global_mcp_servers(ws: &Path) -> Map<String, Value> {
    read_json(&global_mcp_path(ws))
        .and_then(|v| v.get("mcpServers").and_then(Value::as_object).cloned())
        .unwrap_or_default()
}

pub fn mcp_server_names(ws: &Path) -> Vec<String> {
    global_mcp_servers(ws).keys().cloned().collect()
}
