//! Un tour : une demande faite à l'agent, et sa réponse.

use crate::runners::RunResult;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

pub struct Turn {
    pub prompt: String,
    /// Qui demande (un pair), pour la traçabilité.
    pub from: Option<String>,
    /// Chaîne des agents déjà traversés (outil `ask`), journalisée avec le tour.
    pub via: Vec<String>,
    pub model: Option<String>,
    pub enqueued: Instant,
    /// Levé par l'appelant qui renonce : le tour est alors ignoré s'il n'a pas démarré.
    pub cancelled: Arc<AtomicBool>,
    reply: Sender<TurnOutcome>,
}

pub struct TurnOutcome {
    pub result: RunResult,
    pub queued_ms: u64,
    pub duration_ms: u64,
}

impl Turn {
    pub fn new(
        prompt: String,
        from: Option<String>,
        model: Option<String>,
    ) -> (Turn, Receiver<TurnOutcome>) {
        Self::with_via(prompt, from, model, Vec::new())
    }

    /// Comme [`Turn::new`], en conservant la chaîne `via` demandée par un pair.
    pub fn with_via(
        prompt: String,
        from: Option<String>,
        model: Option<String>,
        via: Vec<String>,
    ) -> (Turn, Receiver<TurnOutcome>) {
        let (reply, outcome) = mpsc::channel();
        let turn = Turn {
            prompt,
            from,
            via,
            model,
            enqueued: Instant::now(),
            cancelled: Arc::new(AtomicBool::new(false)),
            reply,
        };
        (turn, outcome)
    }

    pub fn queued_ms(&self) -> u64 {
        self.enqueued.elapsed().as_millis() as u64
    }

    /// Rend la réponse à l'appelant (s'il est parti, personne ne la lira : sans importance).
    pub fn complete(self, result: RunResult, queued_ms: u64, duration_ms: u64) {
        let _ = self.reply.send(TurnOutcome { result, queued_ms, duration_ms });
    }

    /// Termine le tour sans l'exécuter.
    pub fn fail(self, message: &str) {
        let queued = self.queued_ms();
        self.complete(RunResult::err(message), queued, 0);
    }
}
