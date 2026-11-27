//! Édition de `settings_<nom>.json` : modèle, variables d'environnement, MCP.

use super::mcp::write_agent_mcp;
use crate::agents::Updates;
use crate::agents::files::io;
use crate::agents::mcp::mcp_server_names;
use crate::agents::paths::claude_settings_path;
use crate::store::{read_json, write_json_atomic};
use serde_json::{Map, Value, json};
use std::path::Path;

pub fn apply(ws: &Path, name: &str, up: &Updates, changes: &mut Vec<String>) -> Result<(), String> {
    let path = claude_settings_path(ws, name);
    let mut settings = read_json(&path)
        .filter(Value::is_object)
        .ok_or_else(|| format!("settings_{name}.json introuvable ou invalide."))?;

    let env = env_object(&mut settings);
    if let Some(model) = &up.model {
        set_model(env, model, changes);
    }
    if let Some(extra) = &up.env {
        merge_env(env, extra, changes);
    }
    if let Some(servers) = &up.mcp_servers {
        settings["enabledMcpjsonServers"] = json!(servers);
        write_agent_mcp(ws, name, servers)?;
        note_servers(ws, servers, changes);
    }
    write_json_atomic(&path, &settings).map_err(io)
}

/// L'objet `env` des settings (créé s'il manque ou s'il n'est pas un objet).
fn env_object(settings: &mut Value) -> &mut Map<String, Value> {
    let env =
        settings.as_object_mut().expect("objet vérifié").entry("env").or_insert_with(|| json!({}));
    if !env.is_object() {
        *env = json!({});
    }
    env.as_object_mut().expect("objet vérifié")
}

fn set_model(env: &mut Map<String, Value>, model: &str, changes: &mut Vec<String>) {
    let old = env.get("ANTHROPIC_MODEL").and_then(Value::as_str).unwrap_or("(aucun)").to_string();
    env.insert("ANTHROPIC_MODEL".into(), json!(model));
    changes.push(format!("Modèle : {old} → {model}"));
}

fn merge_env(env: &mut Map<String, Value>, extra: &Map<String, Value>, changes: &mut Vec<String>) {
    for (k, v) in extra {
        env.insert(k.clone(), v.clone());
        let shown = if v.as_str().is_some_and(str::is_empty) { "(vide)" } else { "***" };
        changes.push(format!("Env '{k}' → {shown}"));
    }
}

/// Compte rendu des serveurs MCP, avec alerte pour ceux absents de `.mcp.json`.
fn note_servers(ws: &Path, servers: &[String], changes: &mut Vec<String>) {
    let known = mcp_server_names(ws);
    let unknown: Vec<&str> =
        servers.iter().filter(|s| !known.contains(s)).map(String::as_str).collect();
    if !unknown.is_empty() && !known.is_empty() {
        changes.push(format!(
            "ATTENTION: serveurs absents de .mcp.json: {} (valides: {})",
            unknown.join(", "),
            known.join(", ")
        ));
    }
    changes.push(format!("Serveurs MCP → [{}]", servers.join(", ")));
}
