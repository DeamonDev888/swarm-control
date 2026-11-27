//! Oubli de la session d'un agent résident (le prochain tour repart à neuf).

use super::ctx::Ctx;
use crate::sessions;
use std::sync::atomic::Ordering;

impl Ctx {
    /// Oublie la session maintenant. Si un tour tourne, elle sera oubliée **aussi** à sa fin :
    /// le tour enregistre sa session en se terminant et annulerait sinon le reset.
    /// `true` si une session existait.
    pub(super) fn reset_session(&self) -> bool {
        let busy = self.current.lock().unwrap_or_else(|e| e.into_inner()).is_some();
        if busy {
            self.reset_after_turn.store(true, Ordering::SeqCst);
        }
        sessions::delete(&self.workspace, self.runner.as_str(), &self.agent)
    }

    /// Appelé par le fil de travail après chaque tour.
    pub(super) fn apply_pending_reset(&self) {
        if self.reset_after_turn.swap(false, Ordering::SeqCst) {
            sessions::delete(&self.workspace, self.runner.as_str(), &self.agent);
        }
    }
}
