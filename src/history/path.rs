//! Emplacement du journal.

use std::path::{Path, PathBuf};

/// Dossier des journaux d'un workspace.
pub fn dir(ws: &Path) -> PathBuf {
    ws.join("bridge").join("history")
}

/// Journal d'un agent : `bridge/history/<runner>-<agent>.jsonl`.
pub fn path(ws: &Path, runner: &str, agent: &str) -> PathBuf {
    dir(ws).join(format!("{runner}-{agent}.jsonl"))
}
