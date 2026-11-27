//! Concurrence : même si 16 fils (comme 16 process) tentent de prendre le bail en
//! même temps — frais ou laissé par un mort — un seul réussit.

use super::holder::{Holder, STALE_MS};
use super::{acquire, path};
use crate::store::now_ms;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

const THREADS: usize = 16;

fn temp_ws(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("swarm-owners-race-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Lance `THREADS` tentatives simultanées ; rend le nombre de réussites (baux gardés jusqu'au bout).
fn race(ws: &Path) -> usize {
    let barrier = Arc::new(Barrier::new(THREADS));
    let handles: Vec<_> = (0..THREADS)
        .map(|i| {
            let (ws, barrier) = (ws.to_path_buf(), Arc::clone(&barrier));
            std::thread::spawn(move || {
                barrier.wait();
                acquire(&ws, "claude", "alice", &format!("run-{i}"))
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    // Les baux gagnants sont encore détenus ici : on compte avant de les libérer.
    results.iter().filter(|r| r.is_ok()).count()
}

#[test]
fn exactly_one_of_many_simultaneous_acquirers_wins() {
    let ws = temp_ws("fresh");
    assert_eq!(race(&ws), 1);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn exactly_one_taker_over_a_dead_lease_wins() {
    let ws = temp_ws("dead");
    std::fs::create_dir_all(path::dir(&ws)).unwrap();
    let ghost = Holder {
        instance: "mort".into(),
        pid: u32::MAX,
        run_id: "r0".into(),
        ts: now_ms(),
        ..Default::default()
    };
    std::fs::write(path::file(&ws, "claude", "alice"), serde_json::to_vec(&ghost).unwrap())
        .unwrap();
    assert_eq!(race(&ws), 1);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn an_expired_lease_is_taken_over_even_if_its_pid_lives() {
    let ws = temp_ws("expired");
    std::fs::create_dir_all(path::dir(&ws)).unwrap();
    let silent = Holder {
        instance: "silencieux".into(),
        // PID vivant (le nôtre) : seul l'âge rend le bail reprenable.
        pid: std::process::id(),
        run_id: "r0".into(),
        ts: now_ms() - STALE_MS - 1_000,
        ..Default::default()
    };
    std::fs::write(path::file(&ws, "claude", "alice"), serde_json::to_vec(&silent).unwrap())
        .unwrap();
    assert_eq!(race(&ws), 1);
    let _ = std::fs::remove_dir_all(&ws);
}

/// C2 : un bail déjà périmé par l'âge doit être déclaré stale **sans** test de PID
/// (sous Windows, ce test lance `tasklist`, 50–100 ms). On mesure donc : même avec un
/// PID invérifiable, `is_stale` doit répondre quasi immédiatement ; on prend le plus
/// rapide de 5 essais, car une simple soustraction ne peut pas être ralentie cinq fois.
#[test]
#[cfg(windows)]
fn an_expired_lease_is_declared_stale_without_probing_the_pid() {
    let silent = Holder {
        instance: "x".into(),
        // Impair : les PID Windows sont des multiples de 4 — si `is_stale` testait le
        // PID avant l'âge, il lancerait un `tasklist` entier et trahirait la mesure.
        pid: 3_000_000_001,
        run_id: "r".into(),
        ts: now_ms() - STALE_MS - 1_000,
        ..Default::default()
    };
    let fastest = (0..5)
        .map(|_| {
            let start = Instant::now();
            assert!(silent.is_stale(now_ms()));
            start.elapsed()
        })
        .min()
        .unwrap();
    assert!(
        fastest < Duration::from_millis(20),
        "bail périmé par l'âge jugé trop lentement ({fastest:?}) : le PID a été testé avant l'âge ?"
    );
}

/// Invalidation du cache de `pid_alive` (TTL d'une seconde, voir `src/process/alive.rs`).
/// On n'utilise pas de vrai fils : sur une machine chargée, un PID libéré peut être
/// recyclé en quelques secondes, et la valeur rapportée changerait pour de mauvaises
/// raisons. On prend donc un PID fantôme jamais allouable (impair : les PID Windows sont
/// des multiples de 4) — la valeur reste « mort » d'un bout à l'autre, et c'est la
/// **latence** qui prouve l'expiration : dans le TTL la réponse vient du cache (µs),
/// après le TTL elle exige une nouvelle sonde `tasklist` (≥ 50 ms).
#[test]
#[cfg(windows)]
fn the_pid_cache_entry_expires_after_the_ttl() {
    let ghost = 3_000_000_001_u32;
    // Sonde initiale (tasklist, lent) : met « mort » en cache.
    let start = Instant::now();
    assert!(!crate::process::pid_alive(ghost));
    let probe = start.elapsed();
    // Toujours dans le TTL : servi du cache, quasi immédiat.
    let hit = Instant::now();
    assert!(!crate::process::pid_alive(ghost));
    let served = hit.elapsed();
    assert!(
        served < Duration::from_millis(15),
        "réponse servie trop lentement pour venir du cache : {served:?}"
    );
    // Expiration : TTL compté depuis la mise en cache, c'est-à-dire la fin de la sonde.
    let deadline = start + probe + Duration::from_millis(1_400);
    if let Some(rest) = deadline.checked_duration_since(Instant::now()) {
        std::thread::sleep(rest);
    }
    // Après expiration : l'entrée ne peut plus servir, il faut re-sonder tasklist.
    let after = Instant::now();
    assert!(!crate::process::pid_alive(ghost));
    let requeted = after.elapsed();
    assert!(
        requeted >= Duration::from_millis(15),
        "après le TTL, la réponse est encore venue du cache : {requeted:?} — entrée non invalidée ?"
    );
}
