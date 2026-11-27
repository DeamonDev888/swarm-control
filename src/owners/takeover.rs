//! Reprise sérialisée d'un bail périmé.
//!
//! Sans garde, deux repreneurs pourraient chacun retirer le bail puis le recréer :
//! le second effacerait celui, tout neuf, du premier. Le dossier de garde
//! (`<bail>.takeover`, créé de façon exclusive) laisse un seul repreneur à la fois.

use super::holder::{self, Holder};
use crate::store::now_ms;
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

/// Une garde plus vieille que ça est celle d'un process mort : on la retire.
const GUARD_MAX_AGE: Duration = Duration::from_secs(10);
const RETRY_PAUSE: Duration = Duration::from_millis(50);

/// Retire le bail `file` s'il est (toujours) périmé, sous la garde.
pub fn remove_if_stale(file: &Path) {
    guarded(file, |current, now| match current {
        Some(h) => h.is_stale(now),
        None => !holder::is_young(file),
    });
}

/// Variante groupée : les PID de ce passage ont déjà été vérifiés en un seul `tasklist`.
/// Si le fichier a changé de détenteur entre-temps, on revérifie ce nouveau détenteur.
pub fn remove_if_dead(file: &Path, seen: &Holder, alive: &HashSet<u32>) {
    guarded(file, |current, now| match current {
        Some(h) if h.run_id == seen.run_id && h.pid == seen.pid => {
            now.saturating_sub(h.ts) > holder::STALE_MS || !alive.contains(&h.pid)
        }
        Some(h) => h.is_stale(now),
        None => !holder::is_young(file),
    });
}

fn guarded(file: &Path, stale: impl Fn(Option<&Holder>, u64) -> bool) {
    let guard = file.with_extension("takeover");
    if std::fs::create_dir(&guard).is_err() {
        drop_old_guard(&guard);
        std::thread::sleep(RETRY_PAUSE);
        return;
    }
    // Revérifié sous la garde : un autre repreneur a pu passer entre-temps.
    let current = holder::read(file);
    if stale(current.as_ref(), now_ms()) {
        let _ = std::fs::remove_file(file);
    }
    let _ = std::fs::remove_dir(&guard);
}

fn drop_old_guard(guard: &Path) {
    let old = guard
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age > GUARD_MAX_AGE);
    if old {
        let _ = std::fs::remove_dir(guard);
    }
}
