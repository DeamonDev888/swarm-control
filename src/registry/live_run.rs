//! Un run vivant : identité, drapeau d'arrêt, bail et état partagé.

use super::Snapshot;
use super::state::State;
use crate::owners::Lease;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

pub struct LiveRun {
    pub run_id: String,
    pub runner: &'static str,
    pub agent: Option<String>,
    pub workspace: PathBuf,
    pub started_at: u64,
    kill_requested: AtomicBool,
    state: Mutex<State>,
    /// Bail de l'agent (runs nommés seulement) : libéré par `finish`.
    lease: Mutex<Option<Lease>>,
}

impl LiveRun {
    pub fn new(
        run_id: String,
        runner: &'static str,
        agent: Option<&str>,
        workspace: &Path,
        now: u64,
    ) -> Self {
        LiveRun {
            run_id,
            runner,
            agent: agent.map(String::from),
            workspace: workspace.to_path_buf(),
            started_at: now,
            kill_requested: AtomicBool::new(false),
            state: Mutex::new(State::new(now)),
            lease: Mutex::new(None),
        }
    }

    pub(super) fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn snapshot(&self) -> Snapshot {
        let s = self.state();
        Snapshot {
            run_id: self.run_id.clone(),
            runner: self.runner,
            agent: self.agent.clone(),
            pid: s.pid,
            status: s.status,
            exit_code: s.exit_code,
            session_id: s.session_id.clone(),
            started_at: self.started_at,
            last_output_at: s.last_output_at,
            outcome: s.outcome.clone(),
        }
    }

    pub fn request_kill(&self) {
        self.kill_requested.store(true, Ordering::SeqCst);
    }

    pub fn kill_requested(&self) -> bool {
        self.kill_requested.load(Ordering::SeqCst)
    }

    pub(super) fn finished_at(&self) -> Option<u64> {
        self.state().finished_at
    }

    pub(super) fn attach_lease(&self, lease: Option<Lease>) {
        *self.lease.lock().unwrap_or_else(|e| e.into_inner()) = lease;
    }

    /// Battement de cœur du bail (sans effet une fois le run terminé).
    pub(super) fn heartbeat(&self) {
        if let Some(lease) = self.lease.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            lease.touch();
        }
    }

    /// Libère le bail : l'agent peut être relancé, ici ou ailleurs.
    pub(super) fn release_lease(&self) {
        drop(self.lease.lock().unwrap_or_else(|e| e.into_inner()).take());
    }
}
