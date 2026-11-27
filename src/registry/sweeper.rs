//! Balayeur périodique du registre.

use super::{disk, index};
use crate::store::now_ms;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

const INTERVAL: Duration = Duration::from_secs(30);

/// Un passage : battement de cœur des baux, mémoire, puis fichiers des workspaces.
fn sweep(workspaces: &HashSet<PathBuf>) {
    let now = now_ms();
    index::heartbeat();
    let active_pids = index::prune_finished(now);
    for workspace in workspaces {
        disk::sweep_workspace(workspace, &active_pids, now);
    }
}

/// Lance le balayeur en arrière-plan (un passage immédiat, puis toutes les 30 s).
pub fn start_sweeper(default_workspace: PathBuf) {
    std::thread::Builder::new()
        .name("swarm-sweeper".into())
        .spawn(move || {
            loop {
                let mut workspaces = index::workspaces();
                workspaces.insert(default_workspace.clone());
                sweep(&workspaces);
                std::thread::sleep(INTERVAL);
            }
        })
        .expect("thread du balayeur");
}
