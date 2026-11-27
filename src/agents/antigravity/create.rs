//! Création d'un agent Antigravity : prompt et settings JSON.

use crate::agents::files::io;
use crate::agents::paths::{antigravity_prompt_path, antigravity_settings_path};
use crate::agents::{CreateArgs, Created};
use crate::config;
use crate::store::{read_json, write_json_atomic};
use serde_json::{Map, Value, json};
use std::path::Path;

pub fn create(ws: &Path, args: &CreateArgs) -> Result<Created, String> {
    let model = args
        .model
        .map(String::from)
        .or_else(|| config::var("AGY_MODEL"))
        .or_else(|| config::var("ANTIGRAVITY_MODEL"))
        .filter(|m| !m.is_empty());

    let mut env = Map::new();
    if let Some(rel) = args.copy_env_from {
        env.extend(copy_env(ws, rel)?);
    }

    let prompt_path = antigravity_prompt_path(ws, args.name);
    std::fs::create_dir_all(prompt_path.parent().expect("agents/ a un parent")).map_err(io)?;
    std::fs::write(&prompt_path, args.prompt).map_err(io)?;

    let mut settings = json!({
        "env": env,
        "agent": args.name,
        "runner": "antigravity",
    });
    if let Some(m) = model {
        settings["model"] = json!(m);
    }

    let config_path = antigravity_settings_path(ws, args.name);
    write_json_atomic(&config_path, &settings).map_err(io)?;
    Ok(Created { prompt_path, config_path, warnings: Vec::new() })
}

fn copy_env(ws: &Path, rel: &str) -> Result<Map<String, Value>, String> {
    let path = ws.join(rel);
    let src =
        read_json(&path).ok_or_else(|| format!("copyEnvFrom introuvable: {}", path.display()))?;
    Ok(src.get("env").and_then(Value::as_object).cloned().unwrap_or_default())
}
