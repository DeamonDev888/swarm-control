//! Écriture du fichier mémoire : ajout, lecture tolérante, oubli, compaction.
//!
//! Toutes les mutations d'un process (ajout, oubli, compaction) passent par un verrou global :
//! un `memory_store` d'un pair ne vient jamais s'intercaler dans la réécriture d'un
//! `memory_forget`. Les réécritures sont atomiques (fichier temporaire puis renommage) : une
//! lecture concurrente voit l'ancien ou le nouveau fichier, jamais un mélange.

use super::entry::Memory;
use super::normalize::normalize;
use super::path;
use super::secrets::{self, Leak};
use super::{MAX_BYTES, MAX_LINES, MAX_TAG, MAX_TAGS, MAX_TEXT};
use crate::store::now_ms;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Mémorise `text` pour `runner`-`agent` et rend le souvenir écrit (id, ts).
///
/// Refusé, avec un message clair, si le texte est vide ou trop long, si un tag est trop long
/// ou trop nombreux, ou si le texte ou un tag contient un jeton (clé, token, mot de passe) :
/// rien n'est alors écrit.
pub fn store(
    ws: &Path,
    runner: &str,
    agent: &str,
    text: &str,
    tags: &[String],
    source: &str,
) -> Result<Memory, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("texte vide : rien à mémoriser".into());
    }
    let chars = text.chars().count();
    if chars > MAX_TEXT {
        return Err(format!("texte trop long : {chars} caractères (max {MAX_TEXT})"));
    }
    if let Some(leak) = secrets::find_secret(text) {
        return Err(refused(leak));
    }
    let tags = clean_tags(tags)?;
    let now = now_ms();
    let memory = Memory {
        id: new_id(now),
        ts: now,
        text: text.to_string(),
        tags,
        source: source.to_string(),
    };
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    append_with_limits(ws, runner, agent, &memory, MAX_BYTES, MAX_LINES).map_err(|e| {
        format!("mémoire non écrite ({}) : {e}", path::path(ws, runner, agent).display())
    })?;
    Ok(memory)
}

/// Oublie le souvenir `id` : `Ok(true)` s'il existait, `Ok(false)` sinon. Le fichier est
/// réécrit sans lui (et sans les lignes illisibles).
pub fn forget(ws: &Path, runner: &str, agent: &str, id: &str) -> Result<bool, String> {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let file = path::path(ws, runner, agent);
    let entries = read_file(&file);
    let kept: Vec<Memory> = entries.iter().filter(|m| m.id != id).cloned().collect();
    if kept.len() == entries.len() {
        return Ok(false);
    }
    rewrite(&file, &kept)
        .map_err(|e| format!("mémoire non réécrite ({}) : {e}", file.display()))?;
    Ok(true)
}

/// Tous les souvenirs de `runner`-`agent`, dans l'ordre du fichier ; vide si le fichier
/// manque ou est illisible.
pub fn read(ws: &Path, runner: &str, agent: &str) -> Vec<Memory> {
    read_file(&path::path(ws, runner, agent))
}

/// Les souvenirs d'un fichier, en tolérant les lignes illisibles (tronquées par un arrêt
/// brutal, champ obligatoire absent).
pub(super) fn read_file(file: &Path) -> Vec<Memory> {
    let Ok(bytes) = std::fs::read(file) else { return Vec::new() };
    parse(&bytes)
}

fn parse(bytes: &[u8]) -> Vec<Memory> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn new_id(ts: u64) -> String {
    format!("{ts:x}-{:04x}", SEQ.fetch_add(1, Ordering::Relaxed))
}

fn refused(leak: Leak) -> String {
    format!(
        "SECRET: refusé — le souvenir contient {} ; aucun jeton n'est mémorisé. Retire-le du \
         texte (ou nomme la variable, `ANTHROPIC_API_KEY`, sans sa valeur).",
        leak.label()
    )
}

/// Tags nettoyés : espaces retirés, vides et doublons (à la casse et aux accents près)
/// écartés, jetons refusés, bornes respectées.
fn clean_tags(tags: &[String]) -> Result<Vec<String>, String> {
    if tags.len() > MAX_TAGS {
        return Err(format!("trop de tags : {} (max {MAX_TAGS})", tags.len()));
    }
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let tag = tag.trim();
        if tag.is_empty() {
            continue;
        }
        let chars = tag.chars().count();
        if chars > MAX_TAG {
            return Err(format!("tag trop long : {chars} caractères (max {MAX_TAG})"));
        }
        if let Some(leak) = secrets::find_secret(tag) {
            return Err(refused(leak));
        }
        let key = normalize(tag);
        if !out.iter().any(|kept| normalize(kept) == key) {
            out.push(tag.to_string());
        }
    }
    Ok(out)
}

fn append_with_limits(
    ws: &Path,
    runner: &str,
    agent: &str,
    memory: &Memory,
    max_bytes: u64,
    max_lines: usize,
) -> io::Result<()> {
    std::fs::create_dir_all(path::dir(ws))?;
    let file = path::path(ws, runner, agent);
    compact_if_needed(&file, max_bytes, max_lines)?;
    let line = serde_json::to_string(memory).map_err(io::Error::other)?;
    let mut out = OpenOptions::new().create(true).append(true).open(&file)?;
    writeln!(out, "{line}")
}

/// Compacte le fichier s'il dépasse un des plafonds, **avant** l'ajout : il garde alors les
/// plus récents dans la moitié du budget, et les lignes illisibles disparaissent.
fn compact_if_needed(file: &Path, max_bytes: u64, max_lines: usize) -> io::Result<()> {
    let Ok(bytes) = std::fs::read(file) else {
        return Ok(());
    };
    let lines = bytes.iter().filter(|b| **b == b'\n').count();
    if bytes.len() as u64 <= max_bytes && lines <= max_lines {
        return Ok(());
    }
    let kept = compact(parse(&bytes), max_bytes / 2, max_lines / 2);
    rewrite(file, &kept)
}

/// Les plus récents de `entries`, du plus ancien au plus récent, tant que le budget
/// (`max_bytes`, `max_lines`) le permet ; au moins le dernier.
pub(super) fn compact(entries: Vec<Memory>, max_bytes: u64, max_lines: usize) -> Vec<Memory> {
    let mut kept: Vec<Memory> = Vec::new();
    let mut bytes = 0u64;
    for memory in entries.into_iter().rev() {
        let line_len = serde_json::to_string(&memory).map_or(0, |l| l.len() as u64 + 1);
        if !kept.is_empty() && (bytes + line_len > max_bytes || kept.len() >= max_lines) {
            break;
        }
        bytes += line_len;
        kept.push(memory);
    }
    kept.reverse();
    kept
}

/// Réécrit le fichier avec `entries` : temporaire unique dans le dossier, puis renommage
/// (remplace la cible, y compris sous Windows) ; le temporaire est retiré en cas d'échec.
fn rewrite(file: &Path, entries: &[Memory]) -> io::Result<()> {
    let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp: PathBuf = file.with_extension(format!("jsonl.tmp.{}.{n}", std::process::id()));
    let mut body = String::new();
    for entry in entries {
        body.push_str(&serde_json::to_string(entry).map_err(io::Error::other)?);
        body.push('\n');
    }
    let result = std::fs::write(&tmp, body).and_then(|()| std::fs::rename(&tmp, file));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Comme l'ajout de [`store`], pour un souvenir déjà formé et des plafonds explicites
/// (tests de compaction et de récence).
#[cfg(test)]
pub(super) fn append_below(
    ws: &Path,
    runner: &str,
    agent: &str,
    memory: &Memory,
    max_bytes: u64,
    max_lines: usize,
) -> io::Result<()> {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    append_with_limits(ws, runner, agent, memory, max_bytes, max_lines)
}
