//! Ce que l'agent est en train de faire, et ce qu'il a fait en dernier.

use super::ctx::Ctx;
use crate::runners::RunResult;
use crate::store::now_ms;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

pub struct Current {
    pub from: Option<String>,
    pub run_id: String,
    pub started: Instant,
}

pub struct Last {
    pub ok: bool,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub at: u64,
}

impl Ctx {
    pub(super) fn begin_turn(&self, from: Option<String>, run_id: &str) {
        let current = Current { from, run_id: run_id.to_string(), started: Instant::now() };
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = Some(current);
    }

    pub(super) fn end_turn(&self, result: &RunResult, took: Duration) {
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *self.last.lock().unwrap_or_else(|e| e.into_inner()) = Some(Last {
            ok: result.error.is_none(),
            error: result.error.clone(),
            duration_ms: took.as_millis() as u64,
            at: now_ms(),
        });
        self.turns_done.fetch_add(1, Ordering::SeqCst);
    }
}
