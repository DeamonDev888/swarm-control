//! Le fil de travail d'un agent résident : un tour à la fois, dans l'ordre d'arrivée.

use super::ctx::Ctx;
use super::turn::Turn;
use super::waiting;
use crate::history;
use crate::owners::Lease;
use crate::runners::{self, RunResult};
use crate::store::now_ms;
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
        let result = RunResult::err("CANCELLED: demande annulée.");
        finish(ctx, &turn, &result, queued_ms, 0);
        return turn.complete(result, queued_ms, 0);
    }
    let req = ctx.request_for(&turn);
    let live = match waiting::prepare(ctx, &req, &turn.cancelled) {
        Ok(live) => live,
        Err(result) => {
            finish(ctx, &turn, &result, queued_ms, 0);
            return turn.complete(result, queued_ms, 0);
        }
    };
    ctx.begin_turn(turn.from.clone(), &live.run_id);
    let started = Instant::now();
    let result = runners::execute(&req, &live, None);
    ctx.apply_pending_reset();
    let took = started.elapsed();
    ctx.end_turn(&result, took);
    finish(ctx, &turn, &result, queued_ms, took.as_millis() as u64);
    turn.complete(result, queued_ms, took.as_millis() as u64);
}

/// Consigne le tour dans le journal et garde sa sortie pour `logs` / la TUI.
fn finish(ctx: &Ctx, turn: &Turn, result: &RunResult, queued_ms: u64, duration_ms: u64) {
    ctx.set_output(tail(result));
    let entry = history::Entry {
        ts: now_ms(),
        from: turn.from.clone(),
        via: turn.via.clone(),
        prompt: turn.prompt.clone(),
        reply: result.result.clone(),
        error: result.error.clone(),
        session_id: result.session_id.clone(),
        duration_ms,
        queued_ms,
        runner: ctx.runner.as_str().to_string(),
        agent: ctx.agent.clone(),
    };
    if let Err(e) = history::append(&ctx.workspace, ctx.runner.as_str(), &ctx.agent, &entry) {
        eprintln!("[swarm-control] journal du tour non écrit: {e}");
    }
}

/// Derniers 8 Kio de la sortie brute (ou de la réponse), sur une frontière de caractère.
fn tail(result: &RunResult) -> String {
    const MAX: usize = 8 * 1024;
    let source = if result.raw.is_empty() { &result.result } else { &result.raw };
    if source.len() <= MAX {
        return source.clone();
    }
    let mut start = source.len() - MAX;
    while !source.is_char_boundary(start) {
        start += 1;
    }
    source[start..].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::Runner;
    use crate::history;

    fn ctx(name: &str) -> Arc<Ctx> {
        let ws = std::env::temp_dir().join(format!("swarm-worker-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ws);
        Arc::new(Ctx::new("alice", Runner::Claude, ws, "test".into()))
    }

    /// Un tour annulé avant démarrage est quand même journalisé (avec son erreur), et son
    /// appelant reçoit le résultat au lieu d'attendre le silence.
    #[test]
    fn a_cancelled_turn_is_journaled_with_its_error() {
        let ctx = ctx("annulé");
        let (turn, outcome) = Turn::with_via(
            "la demande".into(),
            Some("discord:bob".into()),
            None,
            vec!["bob".into()],
        );
        turn.cancelled.store(true, Ordering::SeqCst);
        run_turn(&ctx, turn);

        let entries = history::read(&ctx.workspace, "claude", "alice", 10, None).unwrap();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert!(!entry.ok());
        assert!(entry.error.as_deref().unwrap().starts_with("CANCELLED"), "{:?}", entry.error);
        assert_eq!(entry.prompt, "la demande");
        assert_eq!(entry.from.as_deref(), Some("discord:bob"));
        assert_eq!(entry.via, ["bob".to_string()]);
        assert_eq!(entry.runner, "claude");
        assert_eq!(entry.agent, "alice");

        let done = outcome.recv().expect("l'appelant est notifié");
        assert!(done.result.error.as_deref().unwrap().starts_with("CANCELLED"));
        let _ = std::fs::remove_dir_all(&ctx.workspace);
    }

    /// La sortie gardée pour `logs` est plafonnée à 8 Kio, sur une frontière de caractère.
    #[test]
    fn the_kept_output_is_bounded_on_a_char_boundary() {
        let ctx = ctx("sortie");
        let big = "é".repeat(10_000); // 20 000 octets
        let result = RunResult { raw: big.clone(), ..Default::default() };
        let kept = tail(&result);
        assert!(kept.len() <= 8 * 1024 + 1, "taille {}", kept.len());
        assert!(std::str::from_utf8(kept.as_bytes()).is_ok());
        assert!(big.ends_with(&kept), "ce doit être la FIN de la sortie");
        // Une réponse sans sortie brute est gardée telle quelle.
        let small = RunResult { result: "courte".into(), ..Default::default() };
        assert_eq!(tail(&small), "courte");
    }
}
