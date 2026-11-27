//! Réservation d'un tour : attendre que l'agent soit libre.
//!
//! Un run « à la demande » lancé par `run_agent` (ici ou dans une autre instance) peut
//! tenir le bail de l'agent : le tour attend alors la fin de ce run au lieu d'échouer.

use super::ctx::Ctx;
use crate::registry::LiveRun;
use crate::runners::{self, RunRequest, RunResult};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const POLL: Duration = Duration::from_millis(200);

/// Réserve le run du tour ; en cas d'`AGENT_BUSY`, réessaie jusqu'à annulation ou arrêt.
pub fn prepare(
    ctx: &Ctx,
    req: &RunRequest,
    cancelled: &AtomicBool,
) -> Result<Arc<LiveRun>, RunResult> {
    loop {
        match runners::prepare(req) {
            Ok(live) => return Ok(live),
            Err(result) if is_busy(&result) => {
                if cancelled.load(Ordering::SeqCst) || ctx.stopping() {
                    return Err(RunResult::err("CANCELLED: tour annulé avant d'avoir démarré."));
                }
                std::thread::sleep(POLL);
            }
            Err(result) => return Err(result),
        }
    }
}

fn is_busy(result: &RunResult) -> bool {
    result.error.as_deref().is_some_and(|e| e.starts_with("AGENT_BUSY"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_agent_busy_errors_are_waited_for() {
        assert!(is_busy(&RunResult::err("AGENT_BUSY: déjà un run")));
        assert!(!is_busy(&RunResult::err("INVALID_AGENT: introuvable")));
        assert!(!is_busy(&RunResult::default()));
    }
}
