//! Lecture / écriture du document d'état d'un agent.

use super::path;
use crate::store::{read_json, write_json_atomic};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

/// Sérialise les lecture-modification-écriture de ce process. Entre process, un
/// agent n'a qu'un seul écrivain à la fois : le détenteur de son bail (`owners`).
static LOCK: Mutex<()> = Mutex::new(());

fn guard() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn load(ws: &Path, runner: &str, agent: &str) -> Value {
    read_json(&path::file(ws, runner, agent)).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

/// Document d'état (objet vide s'il n'existe pas encore).
pub fn read(ws: &Path, runner: &str, agent: &str) -> Value {
    let _guard = guard();
    load(ws, runner, agent)
}

/// Modifie le document puis l'écrit de façon atomique.
pub fn update(
    ws: &Path,
    runner: &str,
    agent: &str,
    edit: impl FnOnce(&mut Value),
) -> std::io::Result<()> {
    let _guard = guard();
    let mut doc = load(ws, runner, agent);
    edit(&mut doc);
    doc["runner"] = json!(runner);
    doc["agent"] = json!(agent);
    write_json_atomic(&path::file(ws, runner, agent), &doc)
}

/// Supprime l'état d'un agent (à la suppression de l'agent).
pub fn remove(ws: &Path, runner: &str, agent: &str) {
    let _guard = guard();
    let _ = std::fs::remove_file(path::file(ws, runner, agent));
}

/// Tous les documents d'état du workspace.
pub fn list(ws: &Path) -> Vec<Value> {
    let _guard = guard();
    let Ok(entries) = std::fs::read_dir(path::dir(ws)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| read_json(&e.path()))
        .filter(Value::is_object)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{list, read, remove, update};
    use serde_json::json;

    fn temp_ws(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("swarm-state-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn update_read_list_remove() {
        let ws = temp_ws("doc");
        assert_eq!(read(&ws, "claude", "a"), json!({}));
        update(&ws, "claude", "a", |d| d["session"] = json!({"id": "s1"})).unwrap();
        update(&ws, "hermes", "a", |d| d["session"] = json!({"id": "h1"})).unwrap();
        assert_eq!(read(&ws, "claude", "a")["session"]["id"], "s1");
        assert_eq!(read(&ws, "claude", "a")["agent"], "a");
        assert_eq!(list(&ws).len(), 2);
        remove(&ws, "claude", "a");
        assert_eq!(read(&ws, "claude", "a"), json!({}));
        assert_eq!(list(&ws).len(), 1);
        let _ = std::fs::remove_dir_all(&ws);
    }
}
