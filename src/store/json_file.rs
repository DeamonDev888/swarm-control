//! Lecture / écriture atomique de fichiers JSON.

use serde_json::Value;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Lit un fichier JSON ; fichier absent ou illisible → `None`.
pub fn read_json(path: &Path) -> Option<Value> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(content.trim_start_matches('\u{feff}')).ok()
}

/// Écrit `value` de façon atomique : fichier temporaire unique dans le même
/// dossier, puis renommage (remplace la cible, y compris sous Windows).
/// En cas d'échec, le temporaire est supprimé — pas de débris `*.tmp.*`.
pub fn write_json_atomic(path: &Path, value: &Value) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("json.tmp.{}.{n}", std::process::id()));
    let body = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    let result = std::fs::write(&tmp, body).and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{read_json, write_json_atomic};
    use serde_json::json;
    use std::path::{Path, PathBuf};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("swarm-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Fichiers du dossier autres que la cible : les temporaires laissés en débris.
    fn debris(dir: &Path, target: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| Path::new(n).file_name() != target.file_name())
            .collect()
    }

    #[test]
    fn write_creates_missing_directories_then_reads_back() {
        let root = temp_dir("creer");
        let file = root.join("a").join("b").join("doc.json");
        assert!(read_json(&file).is_none(), "le fichier n'existe pas encore");
        write_json_atomic(&file, &json!({ "id": "s1", "ts": 42, "nested": { "ok": true } }))
            .unwrap();
        assert!(file.is_file());
        assert_eq!(
            read_json(&file),
            Some(json!({ "id": "s1", "ts": 42, "nested": { "ok": true } }))
        );
        // Le dossier parent (`a`) n'est pas un débris : seuls les voisins du fichier comptent.
        assert!(debris(file.parent().unwrap(), &file).is_empty(), "aucun temporaire résiduel");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn write_replaces_an_existing_file_with_the_new_content() {
        let root = temp_dir("remplace");
        let file = root.join("doc.json");
        write_json_atomic(&file, &json!({ "session": { "id": "ancien", "ts": 1 }, "gardé": true }))
            .unwrap();
        write_json_atomic(&file, &json!({ "session": { "id": "nouveau", "ts": 2 } })).unwrap();
        // Le contenu est entièrement remplacé (pas fusionné, pas tronqué) : la clé
        // `gardé` de la première version disparaît avec l'ancien fichier.
        assert_eq!(read_json(&file), Some(json!({ "session": { "id": "nouveau", "ts": 2 } })));
        assert!(debris(&root, &file).is_empty(), "l'ancien temporaire doit être renommé");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn successive_writes_use_distinct_temporaries_and_leave_none() {
        let root = temp_dir("succes");
        let file = root.join("doc.json");
        for i in 0..20 {
            write_json_atomic(&file, &json!({ "n": i })).unwrap();
            assert_eq!(read_json(&file).unwrap()["n"], i);
        }
        // Chaque écriture passe par un temporaire numéroté : aucun ne reste à la fin.
        let names: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["doc.json"], "{names:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_failing_rename_leaves_the_target_intact_and_no_debris() {
        let root = temp_dir("echec");
        // Une **directory** à la place de la cible : le temporaire s'écrit bien (même
        // dossier, nom libre) mais `rename` vers un dossier non vide échoue sous Windows
        // comme sous Unix. C'est le seul échec de renommage reproducible sans droits.
        let blocked = root.join("bloque.json");
        std::fs::create_dir_all(blocked.join("interne")).unwrap();
        let err = write_json_atomic(&blocked, &json!({ "x": 1 })).unwrap_err();
        // L'erreur est remontée, pas avalée.
        // Windows rend `PermissionDenied` ; Linux `IsADirectory` ou `DirectoryNotEmpty`.
        assert!(
            matches!(
                err.kind(),
                std::io::ErrorKind::AlreadyExists
                    | std::io::ErrorKind::Other
                    | std::io::ErrorKind::PermissionDenied
                    | std::io::ErrorKind::IsADirectory
                    | std::io::ErrorKind::DirectoryNotEmpty
            ),
            "{err:?}"
        );
        // La cible est intacte.
        assert!(blocked.join("interne").is_dir(), "la cible a été déplacée");
        // Aucun `bloque.json.tmp.*` derrière : le dossier ne contient que la cible.
        let left: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, ["bloque.json"], "débris : {left:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_temporary_is_a_neighbour_of_the_target_with_the_pid_in_its_name() {
        // Le renommage atomique exige même volume : le temporaire doit être un voisin
        // direct de la cible, et son nom porté par le PID pour ne jamais percuter un
        // autre process écrivant le même fichier.
        let target = Path::new("/ws/bridge/state/claude-alice.json");
        let tmp = target.with_extension(format!("json.tmp.{}.7", std::process::id()));
        assert_eq!(tmp.parent(), target.parent());
        assert_eq!(
            tmp.file_name().and_then(|n| n.to_str()),
            Some(format!("claude-alice.json.tmp.{}.7", std::process::id()).as_str())
        );
    }

    #[test]
    fn read_json_tolerates_a_bom_and_rejects_invalid_content() {
        let root = temp_dir("lecture");
        let bom = root.join("bom.json");
        std::fs::write(&bom, "\u{feff}{\"a\": 1}").unwrap();
        assert_eq!(read_json(&bom), Some(json!({ "a": 1 })));

        for (name, text) in [
            ("vide.json", ""),
            ("tronque.json", "{\"a\": "),
            ("pasjson.json", "hello"),
            ("tableau.json", "[]"),
        ] {
            let p = root.join(name);
            std::fs::write(&p, text).unwrap();
            // Un tableau est du JSON valide : `read_json` le rend tel quel (c'est l'appelant
            // qui filtre sur `is_object`, voir `state::doc::load`).
            let expected = (name == "tableau.json").then(|| json!([]));
            assert_eq!(read_json(&p), expected, "{name}");
        }
        assert_eq!(read_json(&root.join("absent.json")), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn read_json_returns_none_for_a_directory() {
        let root = temp_dir("dossier");
        assert_eq!(read_json(&root), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_written_file_is_pretty_printed_utf8_without_bom() {
        let root = temp_dir("forme");
        let file = root.join("doc.json");
        write_json_atomic(&file, &json!({ "accent": "é", "list": [1, 2] })).unwrap();
        let bytes = std::fs::read(&file).unwrap();
        assert_ne!(bytes.first_chunk::<3>(), Some(&[0xEF, 0xBB, 0xBF]), "BOM présent");
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("\n  \"accent\": \"é\""), "{text}");
        assert!(text.ends_with('}'), "pas de fin de ligne finale : {text:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
