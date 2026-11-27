//! Un bail détenu : rafraîchi pendant sa vie, libéré à la fin.

use super::holder::{self, Holder};
use crate::store::{now_ms, write_json_atomic};
use std::path::PathBuf;

#[derive(Debug)]
pub struct Lease {
    path: PathBuf,
    token: String,
}

impl Lease {
    pub(super) fn new(path: PathBuf, token: String) -> Self {
        Lease { path, token }
    }

    /// Modifie le contenu du bail, s'il est toujours le nôtre.
    pub fn edit(&self, change: impl FnOnce(&mut Holder)) {
        if let Some(mut holder) = holder::read(&self.path).filter(|h| h.run_id == self.token) {
            change(&mut holder);
            if let Ok(value) = serde_json::to_value(&holder) {
                let _ = write_json_atomic(&self.path, &value);
            }
        }
    }

    /// Battement de cœur : renouvelle `ts` (le reste du bail, URL comprise, est conservé).
    pub fn touch(&self) {
        self.edit(|h| h.ts = now_ms());
    }

    fn is_ours(&self) -> bool {
        holder::read(&self.path).is_some_and(|h| h.run_id == self.token)
    }
}

impl Drop for Lease {
    /// Libération : le fichier n'est retiré que s'il porte encore notre jeton
    /// (un repreneur a pu le remplacer si nous avions cessé de battre).
    fn drop(&mut self) {
        if self.is_ours() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
