//! Modification d'un profil Hermes.

use super::mcp::set_hermes_mcp;
use super::provider::detect_provider;
use super::{cli, env_edit, file_rewrite};
use crate::agents::Updates;
use crate::agents::files::io;
use crate::agents::paths::hermes_profile_dir;
use std::path::Path;

pub fn apply(ws: &Path, name: &str, up: &Updates, changes: &mut Vec<String>) -> Result<(), String> {
    let dir = hermes_profile_dir(name);
    if let Some((file, content)) = &up.file {
        return file_rewrite::apply(&dir, file, content, changes);
    }
    if let Some(model) = &up.model {
        cli::run(&["-p", name, "config", "set", "model.default", model])?;
        cli::run(&["-p", name, "config", "set", "model.provider", detect_provider(model)])?;
        changes.push(format!("Modèle → {model}"));
    }
    if let Some(prompt) = &up.prompt {
        std::fs::write(dir.join("SOUL.md"), prompt).map_err(io)?;
        changes.push("SOUL.md mis à jour".into());
    }
    if let Some(env) = &up.env {
        env_edit::apply(&dir, env, changes)?;
    }
    if let Some(servers) = &up.mcp_servers {
        set_hermes_mcp(ws, &dir, servers)?;
        changes.push(format!("MCPs : [{}]", servers.join(", ")));
    }
    Ok(())
}
