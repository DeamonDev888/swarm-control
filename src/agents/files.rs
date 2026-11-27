//! Petits utilitaires de lecture de fichiers d'agent.

use std::path::Path;

/// Contenu d'un fichier, ou `MISSING` s'il est illisible.
pub fn read_or_missing(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|_| "MISSING".into())
}

/// Erreur d'E/S → message.
pub fn io(e: std::io::Error) -> String {
    e.to_string()
}
