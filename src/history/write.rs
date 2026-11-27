//! Écriture d'un tour et rotation du journal.

use super::entry::Entry;
use super::path;
use crate::store::now_ms;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;

/// Taille au-delà de laquelle le journal est tourné (5 Mio par défaut).
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// Ajoute un tour au journal de l'agent. Crée le dossier au besoin.
///
/// Une erreur d'écriture n'interrompt jamais un tour : l'appelant la journalise sur stderr.
pub fn append(ws: &Path, runner: &str, agent: &str, entry: &Entry) -> io::Result<()> {
    append_with_limit(ws, runner, agent, entry, max_bytes())
}

/// Consigne l'entrée d'un tour : horodatée et rattachée à `runner`/`agent` ici, puis écrite.
///
/// Le commun du worker résident et du tour local de la CLI (`ask`) : même format, même
/// rotation. Une erreur d'écriture n'interrompt jamais le tour : elle sort sur stderr.
pub fn record(ws: &Path, runner: &str, agent: &str, mut entry: Entry) {
    entry.ts = now_ms();
    entry.runner = runner.to_string();
    entry.agent = agent.to_string();
    if let Err(e) = append(ws, runner, agent, &entry) {
        eprintln!("[swarm-control] journal du tour non écrit: {e}");
    }
}

/// Comme [`append`], avec une limite explicite (tests de rotation).
fn append_with_limit(
    ws: &Path,
    runner: &str,
    agent: &str,
    entry: &Entry,
    limit: u64,
) -> io::Result<()> {
    std::fs::create_dir_all(path::dir(ws))?;
    let file = path::path(ws, runner, agent);
    rotate_if_needed(&file, limit)?;
    let mut out = OpenOptions::new().create(true).append(true).open(&file)?;
    let line = serde_json::to_string(entry).map_err(io::Error::other)?;
    writeln!(out, "{line}")
}

/// Fait tourner le journal s'il dépasse la limite (ou celle de `SWARM_HISTORY_MAX`, en octets,
/// pour les tests). Une seule génération conservée : `<fichier>.1`.
fn rotate_if_needed(file: &Path, limit: u64) -> io::Result<()> {
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    if size < limit {
        return Ok(());
    }
    let rotated = file.with_extension("jsonl.1");
    let _ = std::fs::remove_file(&rotated);
    std::fs::rename(file, rotated)
}

#[cfg(test)]
pub(super) fn append_below(
    ws: &Path,
    runner: &str,
    agent: &str,
    entry: &Entry,
    limit: u64,
) -> io::Result<()> {
    append_with_limit(ws, runner, agent, entry, limit)
}

fn max_bytes() -> u64 {
    std::env::var("SWARM_HISTORY_MAX")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(MAX_BYTES)
}
