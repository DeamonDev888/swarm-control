//! Prise d'un bail (de run ou d'hébergement).

use super::holder::{self, Holder};
use super::lease::Lease;
use super::{path, takeover};
use crate::store::now_ms;
use std::io::{ErrorKind, Write};
use std::path::Path;

const ATTEMPTS: usize = 5;

/// Pourquoi le bail n'a pas été pris.
#[derive(Debug)]
pub enum Busy {
    /// Un autre détenteur, vivant, a le bail (boîte : garde `Result<_, Busy>` petit).
    Held(Box<Holder>),
    /// Le bail est en cours de création ou de reprise par un autre process : réessayer.
    Pending,
    /// Le dossier des baux est inutilisable.
    Io(String),
}

/// Bail de **run** : `runner`/`agent` pour le run `run_id`.
pub fn acquire(ws: &Path, runner: &str, agent: &str, run_id: &str) -> Result<Lease, Busy> {
    acquire_at(&path::dir(ws), &path::file(ws, runner, agent), Holder::current(run_id))
}

/// Bail d'**hébergement** : `agent` devient résident dans ce process (`token` identifie l'hébergement).
pub fn acquire_host(ws: &Path, runner: &str, agent: &str, token: &str) -> Result<Lease, Busy> {
    acquire_at(
        &path::hosts_dir(ws),
        &path::host_file(ws, runner, agent),
        Holder::hosting(token, runner, agent),
    )
}

/// Bail générique : le fichier `file` (dans `dir`) pour `holder`, avec les mêmes règles
/// d'exclusivité et de péremption que les baux d'agent.
pub fn acquire_at(dir: &Path, file: &Path, holder: Holder) -> Result<Lease, Busy> {
    std::fs::create_dir_all(dir).map_err(|e| {
        crate::debug!("bail ✗ {} · dossier inutilisable: {e}", file.display());
        Busy::Io(e.to_string())
    })?;
    for _ in 0..ATTEMPTS {
        match try_create(file, &holder) {
            Ok(()) => {
                crate::debug!("bail pris {} · {}", file.display(), holder.run_id);
                return Ok(Lease::new(file.to_path_buf(), holder.run_id.clone()));
            }
            // Déjà pris (os 80/183), ou nom en suppression (os 5) sous Windows :
            // réessayer, ne jamais abandonner dessus.
            Err(e)
                if matches!(e.kind(), ErrorKind::AlreadyExists | ErrorKind::PermissionDenied) => {}
            Err(e) => {
                crate::debug!("bail ✗ {} · {e}", file.display());
                return Err(Busy::Io(e.to_string()));
            }
        }
        match holder::read(file) {
            Some(h) if !h.is_stale(now_ms()) => {
                crate::debug!(
                    "bail refusé {} · détenu par {} (pid {})",
                    file.display(),
                    h.instance,
                    h.pid
                );
                return Err(Busy::Held(Box::new(h)));
            }
            None if holder::is_young(file) => {
                crate::debug!("bail en cours de création {} · réessai", file.display());
                return Err(Busy::Pending);
            }
            _ => {
                crate::debug!("bail périmé {} · reprise", file.display());
                match takeover::take_over(file, &holder) {
                    // Gagné sous la garde : le remplacement est déjà créé.
                    Ok(true) => {
                        crate::debug!("bail repris {} · {}", file.display(), holder.run_id);
                        return Ok(Lease::new(file.to_path_buf(), holder.run_id.clone()));
                    }
                    Ok(false) => {}
                    Err(e) => {
                        crate::debug!("bail ✗ {} · reprise : {e}", file.display());
                        return Err(Busy::Io(e.to_string()));
                    }
                }
            }
        }
    }
    crate::debug!("bail ✗ {} · toujours en attente après {ATTEMPTS} essais", file.display());
    Err(Busy::Pending)
}

/// Création exclusive : une seule des tentatives concurrentes réussit.
pub(super) fn try_create(file: &Path, holder: &Holder) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(file)?;
    f.write_all(&serde_json::to_vec(holder).map_err(std::io::Error::other)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_ws(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("swarm-owners-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn ghost() -> Holder {
        Holder {
            instance: "mort".into(),
            pid: u32::MAX,
            run_id: "r0".into(),
            ts: now_ms(),
            ..Default::default()
        }
    }

    #[test]
    fn second_acquire_is_refused_until_the_first_is_released() {
        let ws = temp_ws("busy");
        let first = acquire(&ws, "claude", "a", "run-1").unwrap();
        let Err(Busy::Held(h)) = acquire(&ws, "claude", "a", "run-2") else {
            panic!("Held attendu");
        };
        assert_eq!((h.run_id.as_str(), h.pid), ("run-1", std::process::id()));
        // Un autre agent, ou le même nom sous un autre runner, n'est pas bloqué.
        assert!(acquire(&ws, "claude", "b", "run-3").is_ok());
        assert!(acquire(&ws, "hermes", "a", "run-4").is_ok());
        drop(first);
        assert!(acquire(&ws, "claude", "a", "run-5").is_ok());
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn host_and_run_leases_are_independent() {
        let ws = temp_ws("kinds");
        let _host = acquire_host(&ws, "claude", "a", "host-1").unwrap();
        assert!(matches!(acquire_host(&ws, "claude", "a", "host-2"), Err(Busy::Held(_))));
        // Héberger un agent n'empêche pas ses runs : chaque tour prend son propre bail.
        assert!(acquire(&ws, "claude", "a", "run-1").is_ok());
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn a_dead_holders_lease_is_taken_over() {
        let ws = temp_ws("dead");
        std::fs::create_dir_all(path::dir(&ws)).unwrap();
        std::fs::write(path::file(&ws, "claude", "a"), serde_json::to_vec(&ghost()).unwrap())
            .unwrap();
        let lease = acquire(&ws, "claude", "a", "run-1").unwrap();
        assert_eq!(holder::read(&path::file(&ws, "claude", "a")).unwrap().run_id, "run-1");
        drop(lease);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn a_late_drop_does_not_remove_the_new_owners_lease() {
        let ws = temp_ws("late");
        let old = acquire(&ws, "claude", "a", "run-old").unwrap();
        // Le bail est repris (simulé : fichier remplacé par un autre run).
        let file = path::file(&ws, "claude", "a");
        std::fs::write(&file, serde_json::to_vec(&Holder::current("run-new")).unwrap()).unwrap();
        drop(old);
        assert_eq!(holder::read(&file).unwrap().run_id, "run-new");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn touch_keeps_the_url_of_a_host_lease() {
        let ws = temp_ws("touch");
        let lease = acquire_host(&ws, "claude", "a", "host-1").unwrap();
        lease.edit(|h| h.url = Some("http://127.0.0.1:9/mcp".into()));
        lease.touch();
        let h = holder::read(&path::host_file(&ws, "claude", "a")).unwrap();
        assert_eq!(h.url.as_deref(), Some("http://127.0.0.1:9/mcp"));
        let _ = std::fs::remove_dir_all(&ws);
    }
}
