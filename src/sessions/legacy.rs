//! Ancien fichier partagé `bridge/agents.json` (OverMind TS) : lecture seule,
//! sauf l'oubli d'une entrée à la suppression d'une session.

use super::entry::{extract_id, is_expired};
use crate::store::{now_ms, read_json, write_json_atomic};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

const FILE: &str = "bridge/agents.json";

static LOCK: Mutex<()> = Mutex::new(());

fn guard() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn path(ws: &Path) -> PathBuf {
    ws.join(FILE)
}

fn load(ws: &Path) -> Map<String, Value> {
    match read_json(&path(ws)) {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// Session connue de l'ancien fichier (clé `runner:agent`, puis clé nue).
pub fn get(ws: &Path, runner: &str, agent: &str) -> Option<String> {
    let _guard = guard();
    let map = load(ws);
    let now = now_ms();
    [format!("{runner}:{agent}"), agent.to_string()]
        .iter()
        .filter_map(|k| map.get(k))
        .find(|v| !is_expired(v, now))
        .and_then(extract_id)
}

/// Retire l'agent de l'ancien fichier. `true` si une entrée existait.
pub fn forget(ws: &Path, runner: &str, agent: &str) -> bool {
    let _guard = guard();
    let mut map = load(ws);
    let removed_namespaced = map.remove(&format!("{runner}:{agent}")).is_some();
    let removed_bare = map.remove(agent).is_some();
    let removed = removed_namespaced || removed_bare;
    if removed {
        let _ = write_json_atomic(&path(ws), &Value::Object(map));
    }
    removed
}
