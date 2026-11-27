//! Nettoyage des fichiers temporaires d'écriture atomique laissés par un crash
//! (les nôtres, et ceux de l'ancien serveur : `process-registry.json.tmp.*`…).

use std::path::Path;
use std::time::Duration;

const MARKER: &str = ".json.tmp.";
const MIN_AGE: Duration = Duration::from_secs(300);

/// Supprime, dans `dir`, les `*.json.tmp.*` de plus de 5 minutes.
pub fn cleanup(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().contains(MARKER) && is_old(&entry) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn is_old(entry: &std::fs::DirEntry) -> bool {
    entry
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age > MIN_AGE)
}
