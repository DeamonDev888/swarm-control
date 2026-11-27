//! Le fil de travail d'un agent résident : un tour à la fois, dans l'ordre d'arrivée.

use super::ctx::Ctx;
use super::turn::Turn;
use super::waiting;
use crate::owners::Lease;
use crate::runners::{self, RunResult};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// Battement de cœur du bail d'hébergement.
const HEARTBEAT: Duration = Duration::from_secs(30);

/// Boucle du fil de travail. Détient le bail d'hébergement : il est libéré à la sortie.
pub fn run(ctx: Arc<Ctx>, lease: Lease) {
    while !ctx.stopping() {
        if let Some(turn) = ctx.inbox.pop(HEARTBEAT) {
            run_turn(&ctx, turn);
        }
        lease.touch();
    }
    // Les demandes arrivées pendant l'arrêt reçoivent une erreur, jamais le silence.
    ctx.clear_queue("STOPPED: l'agent a été arrêté.");
}

fn run_turn(ctx: &Ctx, turn: Turn) {
    let queued_ms = turn.queued_ms();
    if turn.cancelled.load(Ordering::SeqCst) {
        return turn.complete(RunResult::err("CANCELLED: demande annulée."), queued_ms, 0);
    }
    let req = ctx.request_for(&turn);
    let live = match waiting::prepare(ctx, &req, &turn.cancelled) {
        Ok(live) => live,
        Err(result) => return turn.complete(result, queued_ms, 0),
    };
    ctx.begin_turn(turn.from.clone(), &live.run_id);
    let started = Instant::now();
    let result = runners::execute(&req, &live, None);
    let took = started.elapsed();
    ctx.end_turn(&result, took);
    turn.complete(result, queued_ms, took.as_millis() as u64);
}
