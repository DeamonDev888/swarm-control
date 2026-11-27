//! Interruption : tuer le tour en cours, vider la file, arrêter l'agent.

use super::ctx::Ctx;
use crate::registry::{self, Status};
use std::sync::atomic::Ordering;

impl Ctx {
    /// Tue le tour en cours (arbre de processus). `true` s'il y en avait un.
    pub fn kill_current(&self) -> bool {
        let running = registry::find(&self.agent, Some(self.runner.as_str()))
            .filter(|run| run.snapshot().status == Status::Running);
        match running {
            Some(run) => {
                run.request_kill();
                true
            }
            None => false,
        }
    }

    /// Annule les tours en attente ; chaque appelant reçoit `reason`. Rend leur nombre.
    pub fn clear_queue(&self, reason: &str) -> usize {
        let pending = self.inbox.drain();
        let count = pending.len();
        for turn in pending {
            turn.fail(reason);
        }
        count
    }

    /// Première étape de l'arrêt : plus de nouvelle demande, tour en cours tué, file vidée.
    pub fn begin_stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        self.kill_current();
        self.clear_queue("STOPPED: l'agent a été arrêté.");
        self.inbox.wake_all();
    }
}
