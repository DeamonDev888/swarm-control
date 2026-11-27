//! Concurrence : même si 16 fils (comme 16 process) tentent de prendre le bail en
//! même temps — frais ou laissé par un mort — un seul réussit.

use super::holder::Holder;
use super::{acquire, path};
use crate::store::now_ms;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};

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
