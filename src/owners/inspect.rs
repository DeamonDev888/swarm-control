//! Consultation d'un bail sans le prendre.

use super::holder::{self, Holder};
use super::path;
use crate::store::now_ms;
use std::path::Path;

/// Détenteur **vivant** du bail de run d'un agent (bail périmé = `None`).
pub fn holder_of(ws: &Path, runner: &str, agent: &str) -> Option<Holder> {
    live(&path::file(ws, runner, agent))
}

/// Hébergeur **vivant** d'un agent résident, avec l'URL de son serveur.
pub fn host_of(ws: &Path, runner: &str, agent: &str) -> Option<Holder> {
    live(&path::host_file(ws, runner, agent))
}

fn live(file: &Path) -> Option<Holder> {
    holder::read(file).filter(|h| !h.is_stale(now_ms()))
}
