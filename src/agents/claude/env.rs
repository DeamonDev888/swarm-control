//! Variables d'environnement d'un nouvel agent Claude.

use super::token::anthropic_token;
use crate::config;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// Variables par défaut d'un agent : modèle, jeton, URL de l'API, délai.
pub fn default_env(model: &str, agent: &str) -> Map<String, Value> {
    let mut env = Map::new();
    env.insert("ANTHROPIC_MODEL".into(), json!(model));
    if let Some(token) = anthropic_token() {
        env.insert("ANTHROPIC_AUTH_TOKEN".into(), json!(token));
    }
    let base_url =
        config::var("ANTHROPIC_BASE_URL").unwrap_or_else(|| "https://api.anthropic.com".into());
    env.insert("ANTHROPIC_BASE_URL".into(), json!(base_url));
    let timeout = config::var("AGENT_TIMEOUT_MS").unwrap_or_else(|| "3000000".into());
    env.insert("AGENT_TIMEOUT_MS".into(), json!(timeout));
    env.insert("agent".into(), json!(agent));
    env
}

/// Le jeu de variables contient-il un moyen de s'authentifier ?
pub fn has_credentials(env: &Map<String, Value>) -> bool {
    ["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"]
        .iter()
        .any(|k| env.get(*k).and_then(Value::as_str).is_some_and(|s| !s.is_empty()))
}

/// Contenu repris d'un settings existant (`copyEnvFrom`).
pub struct Copied {
    pub env: Map<String, Value>,
    pub servers: Option<Vec<String>>,
}

pub fn copy_from(ws: &Path, rel: &str) -> Result<Copied, String> {
    let source = resolve_inside(ws, rel)?;
    let content = std::fs::read_to_string(&source)
        .map_err(|e| format!("copyEnvFrom illisible ({}): {e}", source.display()))?;
    let parsed: Value = serde_json::from_str(content.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("copyEnvFrom n'est pas du JSON valide: {e}"))?;
    Ok(Copied {
        env: parsed.get("env").and_then(Value::as_object).cloned().unwrap_or_default(),
        servers: parsed
            .get("enabledMcpjsonServers")
            .and_then(Value::as_array)
            .map(|l| l.iter().filter_map(|v| v.as_str().map(String::from)).collect()),
    })
}

fn resolve_inside(ws: &Path, rel: &str) -> Result<PathBuf, String> {
    let p = Path::new(rel);
    if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(format!("copyEnvFrom sort du workspace: {rel}"));
    }
    Ok(if p.is_absolute() { p.to_path_buf() } else { ws.join(p) })
}

#[cfg(test)]
mod tests {
    use super::resolve_inside;
    use std::path::Path;

    #[test]
    fn parent_dir_is_refused() {
        assert!(resolve_inside(Path::new("/ws"), "../secret.json").is_err());
        assert!(resolve_inside(Path::new("/ws"), ".claude/settings_a.json").is_ok());
    }
}
