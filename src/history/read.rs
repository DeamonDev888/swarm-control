//! Lecture du journal : les `limit` derniers tours (éventuellement avant un horodatage).

use super::entry::Entry;
use super::path;
use std::io;

/// Les tours du plus ancien au plus récent, au plus `limit`.
///
/// `before` (ms) exclut les tours à cet horodatage ou après : sert à remonter page par page.
/// Les lignes illisibles (fichier tronqué par un arrêt brutal) sont ignorées : le journal ne
/// doit jamais empêcher de lire le reste.
pub fn read(
    ws: &std::path::Path,
    runner: &str,
    agent: &str,
    limit: usize,
    before: Option<u64>,
) -> io::Result<Vec<Entry>> {
    let current = path::path(ws, runner, agent);
    let rotated = current.with_extension("jsonl.1");
    let mut entries = Vec::new();
    for file in [rotated, current] {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            if let Ok(entry) = serde_json::from_str::<Entry>(line) {
                if before.is_none_or(|b| entry.ts < b) {
                    entries.push(entry);
                }
            }
        }
    }
    if entries.len() > limit {
        entries.drain(..entries.len() - limit);
    }
    Ok(entries)
}

/// `(taille, mtime en ms)` du journal courant : la TUI ne relit que s'il a changé.
pub fn fingerprint(ws: &std::path::Path, runner: &str, agent: &str) -> (u64, u64) {
    let file = path::path(ws, runner, agent);
    let Ok(meta) = file.metadata() else { return (0, 0) };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    (meta.len(), mtime)
}
