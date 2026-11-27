//! Écriture de la section `mcp_servers:` du `config.yaml` Hermes.

use super::yaml::{strip_mcp_section, yaml_scalar};
use crate::agents::files::io;
use crate::agents::mcp::global_mcp_servers;
use serde_json::Value;
use std::path::Path;

/// Réécrit la section `mcp_servers:` avec les `servers` demandés (connus du `.mcp.json`).
pub fn set_hermes_mcp(ws: &Path, profile_dir: &Path, servers: &[String]) -> Result<(), String> {
    let all = global_mcp_servers(ws);
    let config_path = profile_dir.join("config.yaml");
    let current = std::fs::read_to_string(&config_path).unwrap_or_default();

    let mut body = strip_mcp_section(&current);
    let section: Vec<String> = servers
        .iter()
        .filter_map(|name| all.get(name).map(|cfg| render_server(name, cfg)))
        .flatten()
        .collect();
    if !section.is_empty() {
        body.push_str("\n\nmcp_servers:\n");
        body.push_str(&section.join("\n"));
    }
    body.push('\n');
    std::fs::write(&config_path, body).map_err(io)
}

/// Lignes YAML d'un serveur (`url` ou `command` + `args`).
fn render_server(name: &str, cfg: &Value) -> Vec<String> {
    if let Some(url) = cfg.get("url").and_then(Value::as_str) {
        return vec![format!("  {name}:"), format!("    url: {}", yaml_scalar(url))];
    }
    let Some(command) = cfg.get("command").and_then(Value::as_str) else {
        return Vec::new();
    };
    let mut lines = vec![format!("  {name}:"), format!("    command: {}", yaml_scalar(command))];
    let args: Vec<&str> = cfg
        .get("args")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if !args.is_empty() {
        lines.push("    args:".into());
        lines.extend(args.iter().map(|a| format!("    - {}", yaml_scalar(a))));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::render_server;
    use serde_json::json;

    #[test]
    fn renders_url_and_command_servers() {
        assert_eq!(
            render_server("memory", &json!({"url": "http://127.0.0.1:3099/mcp"})),
            vec!["  memory:", "    url: http://127.0.0.1:3099/mcp"]
        );
        let cmd = render_server("db", &json!({"command": "node", "args": ["db.js"]}));
        assert_eq!(cmd, vec!["  db:", "    command: node", "    args:", "    - db.js"]);
        assert!(render_server("x", &json!({})).is_empty());
    }
}
