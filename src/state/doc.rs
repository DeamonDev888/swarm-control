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

    /// Écrit `contenu` dans le dossier d'état de `ws`, sous le nom `fichier`.
    fn ecrire(ws: &std::path::Path, fichier: &str, contenu: &[u8]) {
        std::fs::create_dir_all(crate::state::dir(ws)).unwrap();
        std::fs::write(crate::state::dir(ws).join(fichier), contenu).unwrap();
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

    #[test]
    fn update_fusionne_avec_l_existant_et_impose_runner_et_agent() {
        let ws = temp_ws("fusion");
        update(&ws, "claude", "a", |d| d["session"] = json!({"id": "s1", "ts": 1})).unwrap();
        update(&ws, "claude", "a", |d| {
            d["lastRun"] = json!({"status": "running"});
            // Une édition ne peut pas renommer le document.
            d["runner"] = json!("hermes");
            d["agent"] = json!("autre");
        })
        .unwrap();
        let doc = read(&ws, "claude", "a");
        assert_eq!(doc["session"]["id"], "s1", "la session d'avant est gardée");
        assert_eq!(doc["lastRun"]["status"], "running");
        assert_eq!((doc["runner"].as_str(), doc["agent"].as_str()), (Some("claude"), Some("a")));
        assert_eq!(read(&ws, "claude", "autre"), json!({}), "rien écrit sous le mauvais nom");
        assert_eq!(read(&ws, "hermes", "a"), json!({}));
        // Un agent voisin a son propre document.
        update(&ws, "claude", "ab", |d| d["x"] = json!(1)).unwrap();
        assert!(read(&ws, "claude", "a").get("x").is_none());
        // Retirer ce qui n'existe pas n'est pas une erreur.
        remove(&ws, "claude", "personne");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn un_fichier_illisible_ou_pas_un_objet_est_lu_comme_vide_puis_repare() {
        let ws = temp_ws("illisible");
        ecrire(&ws, "claude-a.json", b"{pas du json");
        ecrire(&ws, "claude-b.json", b"[1, 2]");
        ecrire(&ws, "claude-c.json", b"\"texte\"");
        ecrire(&ws, "claude-e.json", b"");
        for agent in ["a", "b", "c", "e"] {
            assert_eq!(read(&ws, "claude", agent), json!({}), "claude/{agent}");
        }
        // Un BOM en tête n'empêche pas la lecture.
        ecrire(&ws, "claude-d.json", b"\xEF\xBB\xBF{\"session\":{\"id\":\"bom\"}}");
        assert_eq!(read(&ws, "claude", "d")["session"]["id"], "bom");
        // `update` répare : repart d'un document vide et écrit proprement.
        update(&ws, "claude", "a", |d| d["session"] = json!({"id": "s2"})).unwrap();
        assert_eq!(
            read(&ws, "claude", "a"),
            json!({"session": {"id": "s2"}, "runner": "claude", "agent": "a"})
        );
        // `list` ne garde que les objets lisables.
        let mut ids: Vec<String> = list(&ws)
            .into_iter()
            .map(|d| d["session"]["id"].as_str().unwrap_or_default().to_string())
            .collect();
        ids.sort();
        assert_eq!(ids, ["bom", "s2"]);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn list_ignore_ce_qui_n_est_pas_un_document() {
        let ws = temp_ws("liste");
        assert!(list(&ws).is_empty(), "dossier absent : rien");
        ecrire(&ws, "claude-a.json", br#"{"agent":"a"}"#);
        ecrire(&ws, "hermes-b.json", br#"{"agent":"b"}"#);
        ecrire(&ws, "notes.txt", br#"{"agent":"txt"}"#);
        ecrire(&ws, "claude-c.json.tmp.1.2", br#"{"agent":"tmp"}"#);
        ecrire(&ws, "claude-d.json", b"{casse");
        ecrire(&ws, "claude-e.json", b"[]");
        std::fs::create_dir_all(crate::state::dir(&ws).join("dossier.json")).unwrap();
        let mut agents: Vec<String> = list(&ws)
            .into_iter()
            .map(|d| d["agent"].as_str().unwrap_or_default().to_string())
            .collect();
        agents.sort();
        assert_eq!(agents, ["a", "b"]);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn un_dossier_d_etat_inutilisable_ne_fait_pas_echouer_lecture_liste_et_retrait() {
        let ws = temp_ws("bloque");
        std::fs::create_dir_all(ws.join("bridge")).unwrap();
        // Un fichier à la place du dossier d'état.
        std::fs::write(ws.join("bridge").join("state"), b"").unwrap();
        assert!(list(&ws).is_empty());
        assert_eq!(read(&ws, "claude", "a"), json!({}));
        remove(&ws, "claude", "a");
        // Seule l'écriture le signale.
        let erreur = update(&ws, "claude", "a", |d| d["x"] = json!(1)).unwrap_err();
        assert!(!erreur.to_string().is_empty());
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn des_mises_a_jour_concurrentes_ne_perdent_aucune_modification() {
        let ws = temp_ws("course");
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let ws = &ws;
                scope.spawn(move || {
                    for _ in 0..25 {
                        update(ws, "claude", "a", |d| {
                            let n = d["n"].as_u64().unwrap_or(0);
                            d["n"] = json!(n + 1);
                        })
                        .unwrap();
                    }
                });
            }
        });
        assert_eq!(
            read(&ws, "claude", "a")["n"],
            200,
            "chaque lecture-modification-écriture est sérialisée"
        );
        let _ = std::fs::remove_dir_all(&ws);
    }
}
