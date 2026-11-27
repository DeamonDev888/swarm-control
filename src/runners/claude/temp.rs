//! Fichiers temporaires d'un run, supprimés à la fin (même en cas d'erreur).

use std::path::PathBuf;

#[derive(Default)]
pub struct TempFiles(Vec<PathBuf>);

impl TempFiles {
    pub fn track(&mut self, path: PathBuf) {
        self.0.push(path);
    }
}

impl Drop for TempFiles {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}
