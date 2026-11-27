//! Modification d'un agent Antigravity : prompt, modèle, variables, ou réécriture de fichier.

use crate::agents::Updates;
use crate::agents::files::io;
use crate::agents::paths::{antigravity_prompt_path, antigravity_settings_path};
use crate::store::{read_json, write_json_atomic};
use serde_json::{Map, Value, json};
use std::path::Path;

pub fn apply(ws: &Path, name: &str, up: &Updates, changes: &mut Vec<String>) -> Result<(), String> {
    if let Some((file, content)) = &up.file {
        return rewrite_file(ws, name, file, content, changes);
    }
    if let Some(prompt) = &up.prompt {
        let path = antigravity_prompt_path(ws, name);
        std::fs::create_dir_all(path.parent().expect("agents/ a un parent")).map_err(io)?;
        std::fs::write(&path, prompt).map_err(io)?;
        changes.push("Prompt mis à jour".into());
    }
    if up.model.is_some() || up.env.is_some() {
        edit_settings(ws, name, up, changes)?;
    }
    if up.mcp_servers.is_some() {
        changes.push(
            "Note : Antigravity lit ses serveurs MCP depuis ~/.gemini/config/mcp_config.json \
             ou .agents/mcp_config.json (mcpServers ignoré)."
                .into(),
        );
    }
    Ok(())
}

fn edit_settings(
    ws: &Path,
    name: &str,
    up: &Updates,
    changes: &mut Vec<String>,
) -> Result<(), String> {
    let path = antigravity_settings_path(ws, name);
    let mut settings = read_json(&path)
        .filter(Value::is_object)
        .ok_or_else(|| format!("settings_{name}.json introuvable ou invalide."))?;

    if let Some(model) = &up.model {
        let old = settings.get("model").and_then(Value::as_str).unwrap_or("(défaut)").to_string();
        settings["model"] = json!(model);
        changes.push(format!("Modèle : {old} → {model}"));
    }
    if let Some(extra) = &up.env {
        let env = env_object(&mut settings);
        for (k, v) in extra {
            env.insert(k.clone(), v.clone());
            let shown = if v.as_str().is_some_and(str::is_empty) { "(vide)" } else { "***" };
            changes.push(format!("Env '{k}' → {shown}"));
        }
    }
    write_json_atomic(&path, &settings).map_err(io)
}

fn env_object(settings: &mut Value) -> &mut Map<String, Value> {
    let env =
        settings.as_object_mut().expect("objet vérifié").entry("env").or_insert_with(|| json!({}));
    if !env.is_object() {
        *env = json!({});
    }
    env.as_object_mut().expect("objet vérifié")
}

fn rewrite_file(
    ws: &Path,
    name: &str,
    file: &str,
    content: &str,
    changes: &mut Vec<String>,
) -> Result<(), String> {
    let target = match file {
        "prompt.md" => antigravity_prompt_path(ws, name),
        "settings.json" => {
            serde_json::from_str::<Value>(content)
                .map_err(|e| format!("{file} n'est pas du JSON valide: {e}"))?;
            antigravity_settings_path(ws, name)
        }
        other => return Err(format!("Fichier non supporté pour Antigravity: {other}")),
    };
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    std::fs::write(&target, content).map_err(io)?;
    changes.push(format!("{file} réécrit → {}", target.display()));
    Ok(())
}
