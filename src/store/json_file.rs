//! Lecture / écriture atomique de fichiers JSON.

use serde_json::Value;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Lit un fichier JSON ; fichier absent ou illisible → `None`.
pub fn read_json(path: &Path) -> Option<Value> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(content.trim_start_matches('\u{feff}')).ok()
}

/// Écrit `value` de façon atomique : fichier temporaire unique dans le même
/// dossier, puis renommage (remplace la cible, y compris sous Windows).
/// En cas d'échec, le temporaire est supprimé — pas de débris `*.tmp.*`.
pub fn write_json_atomic(path: &Path, value: &Value) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("json.tmp.{}.{n}", std::process::id()));
    let body = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    let result = std::fs::write(&tmp, body).and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}
