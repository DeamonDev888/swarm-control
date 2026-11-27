//! État d'un agent résident, au format JSON de l'outil `status`.

use super::ctx::Ctx;
use crate::sessions;
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

impl Ctx {
    pub fn status_json(&self) -> Value {
        let current = self.current.lock().unwrap_or_else(|e| e.into_inner());
        let last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        json!({
            "agent": self.agent,
            "runner": self.runner.as_str(),
            "name": self.name(),
            "url": self.url(),
            "instance": self.instance,
            "state": if current.is_some() { "busy" } else { "idle" },
            "queueLength": self.inbox.len(),
            "current": current.as_ref().map(|c| json!({
                "from": c.from,
                "runId": c.run_id,
                "elapsedMs": c.started.elapsed().as_millis() as u64,
            })),
            "session": sessions::get(&self.workspace, self.runner.as_str(), &self.agent),
            "turnsDone": self.turns_done.load(Ordering::SeqCst),
            "lastTurn": last.as_ref().map(|l| json!({
                "ok": l.ok,
                "error": l.error,
                "durationMs": l.duration_ms,
                "at": l.at,
            })),
        })
    }
}
