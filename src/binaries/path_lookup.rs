//! Recherche d'un exécutable dans le `PATH`.

use std::path::PathBuf;

/// Premier fichier nommé `names[i]` trouvé dans un dossier du `PATH`.
pub fn on_path(names: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
