//! Sessions persistantes : `<workspace>/bridge/agents.json`.
//!
//! Format partagé avec l'OverMind TS :
//! `{ "claude:expert": { "id": "<session>", "ts": 1790000000000 } }`
//! (les anciennes entrées sans runner, ou en chaîne nue, restent lues).
//! Les entrées de plus de 30 jours sont purgées à chaque écriture.

use crate::store::{now_ms, read_json, write_json_atomic};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const SESSIONS_FILE: &str = "bridge/agents.json";
const SESSION_TTL_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// Un seul écrivain à la fois dans ce process (lecture-modification-écriture).
static LOCK: Mutex<()> = Mutex::new(());

fn path(workspace: &Path) -> PathBuf {
    workspace.join(SESSIONS_FILE)
}

fn key(runner: &str, agent: &str) -> String {
    format!("{runner}:{agent}")
}

fn extract_id(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Object(o) => o.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from),
        _ => None,
    }
}

fn is_expired(value: &Value, now: u64) -> bool {
    value
        .get("ts")
        .and_then(Value::as_u64)
        .is_some_and(|ts| now.saturating_sub(ts) >= SESSION_TTL_MS)
}

fn load(workspace: &Path) -> Map<String, Value> {
    match read_json(&path(workspace)) {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

fn purge(map: &mut Map<String, Value>) {
    let now = now_ms();
    map.retain(|_, v| !is_expired(v, now));
}

/// Dernière session connue de l'agent (clé `runner:agent`, puis clé nue).
pub fn get(workspace: &Path, runner: &str, agent: &str) -> Option<String> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let map = load(workspace);
    let now = now_ms();
    [key(runner, agent), agent.to_string()]
        .iter()
        .filter_map(|k| map.get(k))
        .find(|v| !is_expired(v, now))
        .and_then(extract_id)
}

pub fn save(workspace: &Path, runner: &str, agent: &str, session_id: &str) -> std::io::Result<()> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut map = load(workspace);
    purge(&mut map);
    map.insert(key(runner, agent), json!({ "id": session_id, "ts": now_ms() }));
    write_json_atomic(&path(workspace), &Value::Object(map))
}

/// Oublie la session de l'agent. Renvoie `true` si une entrée existait.
pub fn delete(workspace: &Path, runner: &str, agent: &str) -> bool {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut map = load(workspace);
    let removed_ns = map.remove(&key(runner, agent)).is_some();
    let removed_bare = map.remove(agent).is_some();
    if removed_ns || removed_bare {
        purge(&mut map);
        let _ = write_json_atomic(&path(workspace), &Value::Object(map));
    }
    removed_ns || removed_bare
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_ws(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("overmind-rs-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn save_get_delete_roundtrip() {
        let ws = temp_ws("sessions");
        assert_eq!(get(&ws, "claude", "a"), None);
        save(&ws, "claude", "a", "s1").unwrap();
        save(&ws, "hermes", "a", "h1").unwrap();
        assert_eq!(get(&ws, "claude", "a").as_deref(), Some("s1"));
        assert_eq!(get(&ws, "hermes", "a").as_deref(), Some("h1"));
        assert!(delete(&ws, "claude", "a"));
        assert_eq!(get(&ws, "claude", "a"), None);
        assert_eq!(get(&ws, "hermes", "a").as_deref(), Some("h1"));
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn reads_legacy_and_skips_expired() {
        let ws = temp_ws("legacy");
        let old = now_ms() - SESSION_TTL_MS - 1;
        write_json_atomic(
            &path(&ws),
            &json!({ "bare": "legacy-id", "claude:old": { "id": "x", "ts": old } }),
        )
        .unwrap();
        assert_eq!(get(&ws, "claude", "bare").as_deref(), Some("legacy-id"));
        assert_eq!(get(&ws, "claude", "old"), None);
        let _ = std::fs::remove_dir_all(&ws);
    }
}
