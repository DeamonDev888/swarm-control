//! Nettoyage des baux périmés d'un workspace.

use super::{path, takeover};
use std::path::Path;

/// Retire les baux (de run et d'hébergement) dont le détenteur est mort ou silencieux.
pub fn sweep(ws: &Path) {
    for dir in [path::dir(ws), path::hosts_dir(ws)] {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let file = entry.path();
            if file.extension().is_some_and(|x| x == "json") {
                takeover::remove_if_stale(&file);
            }
        }
    }
}
