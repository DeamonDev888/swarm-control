//! Un PID désigne-t-il un process vivant ?
//!
//! Sous Windows, chaque `tasklist` coûte 50–100 ms. Choix retenu, en std uniquement :
//! un **cache court par PID** (`CACHE_TTL`, une seconde) — il profite à tous les appelants
//! épars (reprise de bail, inspection, découverte de pairs, `kill`) sans changer leurs
//! signatures — complété par la requête **groupée** `pids_alive`, un seul `tasklist` pour
//! tous les PID d'un même passage du balayeur. Une entrée est invalidée par le temps (TTL) :
//! une erreur éventuelle de `tasklist` ou la mort d'un process ne peuvent tromper personne
//! plus d'une seconde.

use std::process::Stdio;

#[cfg(windows)]
use std::collections::{HashMap, HashSet};
#[cfg(windows)]
use std::path::Path;
#[cfg(windows)]
use std::sync::Mutex;
#[cfg(windows)]
use std::time::{Duration, Instant};

/// Sous Windows, `tasklist` coûte 50–100 ms : un résultat est gardé une seconde.
#[cfg(windows)]
const CACHE_TTL: Duration = Duration::from_secs(1);
#[cfg(windows)]
const CACHE_MAX: usize = 256;

#[cfg(windows)]
static CACHE: Mutex<Option<HashMap<u32, (bool, Instant)>>> = Mutex::new(None);

#[cfg(windows)]
fn cached(pid: u32) -> Option<bool> {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.as_mut()?;
    let (alive, at) = cache.get(&pid)?;
    if at.elapsed() <= CACHE_TTL {
        return Some(*alive);
    }
    cache.remove(&pid);
    None
}

#[cfg(windows)]
fn remember(pid: u32, alive: bool) {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    if cache.len() >= CACHE_MAX {
        cache.retain(|_, (_, at)| at.elapsed() <= CACHE_TTL);
    }
    cache.insert(pid, (alive, Instant::now()));
}

#[cfg(windows)]
pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    if let Some(alive) = cached(pid) {
        return alive;
    }
    let filter = format!("PID eq {pid}");
    let alive = super::command(std::path::Path::new("tasklist"))
        .args(["/FI", &filter, "/FO", "CSV", "/NH"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")))
        .unwrap_or(false);
    remember(pid, alive);
    alive
}

/// Les PID vivants parmi `pids`, avec **un seul** `tasklist` sous Windows.
#[cfg(windows)]
pub fn pids_alive(pids: &[u32]) -> HashSet<u32> {
    let wanted: HashSet<u32> = pids.iter().copied().filter(|p| *p != 0).collect();
    if wanted.is_empty() {
        return HashSet::new();
    }
    let output = super::command(Path::new("tasklist"))
        .args(["/FO", "CSV", "/NH"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let mut alive = HashSet::new();
    if let Ok(output) = output {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if let Some(pid) = csv_pid(line) {
                if wanted.contains(&pid) {
                    alive.insert(pid);
                }
            }
        }
    }
    for pid in &wanted {
        remember(*pid, alive.contains(pid));
    }
    alive
}

/// Deuxième champ d'une ligne `"image.exe","1234",…` de `tasklist /FO CSV`.
#[cfg(windows)]
fn csv_pid(line: &str) -> Option<u32> {
    line.split(',').nth(1)?.trim().trim_matches('"').parse().ok()
}

#[cfg(unix)]
pub fn pids_alive(pids: &[u32]) -> std::collections::HashSet<u32> {
    pids.iter().copied().filter(|pid| pid_alive(*pid)).collect()
}

/// PID utilisable avec `kill` : `0` vise notre propre groupe, et au-delà de
/// `i32::MAX` la valeur repasse en négatif (`u32::MAX` devient `-1` : tous les process).
#[cfg(unix)]
pub(super) fn valid_unix_pid(pid: u32) -> bool {
    pid != 0 && i32::try_from(pid).is_ok()
}

#[cfg(unix)]
pub fn pid_alive(pid: u32) -> bool {
    if !valid_unix_pid(pid) {
        return false;
    }
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
