//! Dernier run de chaque agent, dans son fichier d'état (`bridge/state/…`).

use super::debris;
use crate::store::now_ms;
use crate::{owners, process, state};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::Path;

const RUNNERS: [&str; 2] = ["claude", "hermes"];

/// Modifie (ou crée) l'objet `lastRun` de l'agent.
pub fn record(ws: &Path, runner: &str, agent: &str, edit: impl FnOnce(&mut Value)) {
    let _ = state::update(ws, runner, agent, |doc| {
        let Some(object) = doc.as_object_mut() else { return };
        let run = object.entry("lastRun").or_insert_with(|| json!({}));
        if !run.is_object() {
            *run = json!({});
        }
        edit(run);
    });
}

/// Dernier run connu d'un agent, avec son runner : `(runner, {pid, status, id, …})`.
/// Sert quand le run n'est pas dans la mémoire de ce process (autre instance, ou
/// serveur redémarré depuis). Sans runner : le plus récent des deux.
pub fn disk_entry(ws: &Path, agent: &str, runner: Option<&str>) -> Option<(String, Value)> {
    let runners: Vec<&str> = runner.map_or_else(|| RUNNERS.to_vec(), |r| vec![r]);
    runners
        .into_iter()
        .filter_map(|r| {
            let doc = state::read(ws, r, agent);
            let mut run = doc.get("lastRun")?.clone();
            run["id"] = doc.pointer("/session/id").cloned().unwrap_or_else(|| json!(""));
            Some((r.to_string(), run))
        })
        .max_by_key(|(_, run)| run.get("ts").and_then(Value::as_u64).unwrap_or(0))
}

/// Marque le dernier run (process hors de ce serveur) comme arrêté.
pub fn mark_killed(ws: &Path, runner: &str, agent: &str) {
    record(ws, runner, agent, |run| {
        run["status"] = json!("failed");
        run["exitCode"] = Value::Null;
        run["lastOutputAt"] = json!(now_ms());
    });
}

/// Un passage du balayeur sur les fichiers d'état et les baux d'un workspace.
pub fn sweep_workspace(ws: &Path, active_pids: &HashSet<u32>, now: u64) {
    for doc in state::list(ws) {
        let (Some(runner), Some(agent)) =
            (doc.get("runner").and_then(Value::as_str), doc.get("agent").and_then(Value::as_str))
        else {
            continue;
        };
        if is_orphan(doc.get("lastRun"), active_pids) {
            record(ws, runner, agent, |run| {
                run["status"] = json!("orphaned");
                run["lastOutputAt"] = json!(now);
            });
        }
    }
    owners::sweep(ws);
    debris::cleanup(&state::dir(ws));
    if let Some(bridge) = state::dir(ws).parent() {
        debris::cleanup(bridge);
    }
}

/// Un run « running » dont le PID est mort (et que ce process ne suit pas).
fn is_orphan(last_run: Option<&Value>, active_pids: &HashSet<u32>) -> bool {
    let Some(run) = last_run else { return false };
    let running = run.get("status").and_then(Value::as_str) == Some("running");
    let pid = run.get("pid").and_then(Value::as_u64).map(|p| p as u32);
    match (running, pid) {
        (true, Some(pid)) => !active_pids.contains(&pid) && !process::pid_alive(pid),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_dead_untracked_running_pids_are_orphans() {
        let none = HashSet::new();
        let running_dead = json!({"status": "running", "pid": u32::MAX});
        assert!(is_orphan(Some(&running_dead), &none));
        // Suivi par ce process : jamais orphelin.
        assert!(!is_orphan(Some(&running_dead), &HashSet::from([u32::MAX])));
        // Terminé ou sans run : rien à faire.
        assert!(!is_orphan(Some(&json!({"status": "done", "pid": u32::MAX})), &none));
        assert!(!is_orphan(None, &none));
    }
}
