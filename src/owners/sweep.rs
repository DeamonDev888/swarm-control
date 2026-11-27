//! Nettoyage des baux périmés d'un workspace.
//!
//! Un passage lit tous les baux, écarte d'abord ceux dont l'âge suffit (aucun appel système),
//! puis vérifie en **un seul** `tasklist` les PID restants (Windows) avant de les reprendre.

use super::{holder, path, takeover};
use crate::process;
use crate::store::now_ms;
use std::path::{Path, PathBuf};

/// Retire les baux (de run et d'hébergement) dont le détenteur est mort ou silencieux.
pub fn sweep(ws: &Path) {
    let now = now_ms();
    let mut fresh: Vec<(PathBuf, holder::Holder)> = Vec::new();
    for dir in [path::dir(ws), path::hosts_dir(ws)] {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let file = entry.path();
            if file.extension().is_none_or(|x| x != "json") {
                continue;
            }
            match holder::read(&file) {
                // Périmé par l'âge : le retirer ne demande aucune interrogation du système.
                Some(h) if now.saturating_sub(h.ts) > holder::STALE_MS => {
                    takeover::remove_if_stale(&file)
                }
                Some(h) => fresh.push((file, h)),
                None => takeover::remove_if_stale(&file),
            }
        }
    }
    let pids: Vec<u32> = fresh.iter().map(|(_, h)| h.pid).collect();
    let alive = process::pids_alive(&pids);
    for (file, seen) in fresh {
        takeover::remove_if_dead(&file, &seen, &alive);
    }
}
