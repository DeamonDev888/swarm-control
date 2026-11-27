//! Sortie en direct d'un run.

use super::LiveRun;
use crate::store::now_ms;

impl LiveRun {
    pub fn push_output(&self, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        let now = now_ms();
        let mut s = self.state();
        s.output.push(now, chunk);
        s.last_output_at = now;
    }

    /// Sortie accumulée depuis `since_ms` (toute la fenêtre si `None`).
    pub fn output_since(&self, since_ms: Option<u64>) -> String {
        self.state().output.since(since_ms)
    }
}
