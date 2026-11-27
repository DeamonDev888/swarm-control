//! Run absent de la mémoire : repli sur l'état de l'agent sur disque.
//!
//! Le run tourne dans une autre instance, ou le serveur a redémarré depuis son
//! lancement : on connaît son PID et son statut, pas sa sortie.

use super::args::ControlArgs;
use crate::process;
use crate::registry;
use crate::tools::errors::invalid;
use litemcp::prelude::{ToolError, ToolResult};
use serde_json::{Value, json};
use std::path::Path;

pub fn from_disk(args: &ControlArgs, workspace: &Path) -> ToolResult {
    let Some(name) = args.agent_name.as_deref() else {
        return Err(invalid(format!("run introuvable: {}", args.run_id.as_deref().unwrap_or("?"))));
    };
    let Some((runner, entry)) = registry::disk_entry(workspace, name, args.runner.as_deref())
    else {
        return Err(invalid(format!("aucun run connu pour l'agent '{name}'")));
    };
    let pid = entry.get("pid").and_then(Value::as_u64).map(|p| p as u32);
    let alive = pid.is_some_and(process::pid_alive);
    let recorded = entry.get("status").and_then(Value::as_str).unwrap_or("unknown");
    let status = if recorded == "running" && !alive { "orphaned" } else { recorded };
    match args.action.as_str() {
        "status" | "stream" | "wait" => Ok(describe(name, pid, status, alive, &entry)),
        "kill" => Ok(kill(workspace, &runner, name, pid, alive && recorded == "running", status)),
        other => Err(ToolError::invalid_input(format!("action inconnue '{other}'"))),
    }
}

fn describe(name: &str, pid: Option<u32>, status: &str, alive: bool, entry: &Value) -> Value {
    json!({
        "source": "disk",
        "agentName": name,
        "pid": pid,
        "status": status,
        "alive": alive,
        "sessionId": entry.get("id"),
        "note": "run hors de ce process (autre instance ou serveur redémarré) : pas de sortie en direct disponible",
    })
}

fn kill(
    workspace: &Path,
    runner: &str,
    agent: &str,
    pid: Option<u32>,
    killable: bool,
    status: &str,
) -> Value {
    match pid {
        Some(pid) if killable => {
            process::kill_tree(pid);
            registry::mark_killed(workspace, runner, agent);
            json!({ "killed": true, "pid": pid, "source": "disk" })
        }
        _ => json!({ "killed": false, "reason": "aucun process vivant", "status": status }),
    }
}
