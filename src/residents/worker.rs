//! Le fil de travail d'un agent résident : un tour à la fois, dans l'ordre d'arrivée.
//!
//! Au démarrage, il lance aussi le planificateur des tâches planifiées (`schedule.rs`)
//! lues dans la définition de l'agent : leurs tours arrivent dans la même file.

use super::ctx::Ctx;
use super::turn::Turn;
use super::{schedule, waiting};
use crate::history;
use crate::owners::Lease;
use crate::runners::{self, RunResult};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// Battement de cœur du bail d'hébergement.
const HEARTBEAT: Duration = Duration::from_secs(30);

/// Boucle du fil de travail. Détient le bail d'hébergement : il est libéré à la sortie.
pub fn run(ctx: Arc<Ctx>, lease: Lease) {
    // Tâche planifiée (PLAN §4.4) : un fil à part qui dépose ses tours dans la même file.
    // Il sonde `stopping` pendant chaque attente ; sa fin est attendue avant de rendre le
    // bail, pour qu'aucun tour ne soit déposé après l'arrêt.
    let scheduler = start_scheduler(&ctx);
    while !ctx.stopping() {
        if let Some(turn) = ctx.inbox.pop(HEARTBEAT) {
            run_turn(&ctx, turn);
        }
        lease.touch();
    }
    // Les demandes arrivées pendant l'arrêt reçoivent une erreur, jamais le silence.
    ctx.clear_queue("STOPPED: l'agent a été arrêté.");
    if let Some(handle) = scheduler {
        let _ = handle.join();
    }
}

/// Lance le planificateur si la définition de l'agent porte un `schedule` ; `None` sinon
/// (ou schedule illisible : signalé au journal de diagnostic, l'agent démarre quand même).
fn start_scheduler(ctx: &Arc<Ctx>) -> Option<std::thread::JoinHandle<()>> {
    let task = schedule::from_definition(&ctx.workspace, ctx.runner, &ctx.agent)?;
    match schedule::spawn(Arc::clone(ctx), task) {
        Ok(handle) => Some(handle),
        Err(e) => {
            crate::debug!("résidence {} : planificateur non démarré · {e}", ctx.agent);
            None
        }
    }
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
    history::record(
        &ctx.workspace,
        ctx.runner.as_str(),
        &ctx.agent,
        history::Entry {
            from: turn.from.clone(),
            via: turn.via.clone(),
            prompt: turn.prompt.clone(),
            reply: result.result.clone(),
            error: result.error.clone(),
            session_id: result.session_id.clone(),
            duration_ms,
            queued_ms,
            ..Default::default()
        },
    );
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

    /// Un `schedule` dans la définition lance le planificateur au démarrage du fil de
    /// travail : son tour arrive dans la même file, marqué `from: "schedule"` ; sans
    /// `schedule`, rien n'est lancé.
    #[test]
    fn un_schedule_de_la_definition_lance_le_planificateur() {
        let ctx = ctx("planifie");
        assert!(start_scheduler(&ctx).is_none(), "sans définition : rien");
        let path = crate::agents::claude_settings_path(&ctx.workspace, "alice");
        crate::store::write_json_atomic(&path, &serde_json::json!({ "schedule": "1s résume" }))
            .unwrap();
        let scheduler = start_scheduler(&ctx).expect("planificateur lancé");
        let turn = ctx.inbox.pop(Duration::from_secs(5)).expect("tour planifié déposé");
        assert_eq!((turn.from.as_deref(), turn.prompt.as_str()), (Some("schedule"), "résume"));
        let queued = turn.queued_ms();
        turn.complete(RunResult::default(), queued, 0);
        ctx.stopping.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !scheduler.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(scheduler.is_finished(), "le planificateur s'arrête avec l'agent");
        scheduler.join().unwrap();
        let _ = std::fs::remove_dir_all(&ctx.workspace);
    }

    /// La sortie gardée pour `logs` est plafonnée à 8 Kio, sur une frontière de caractère.
    #[test]
    fn the_kept_output_is_bounded_on_a_char_boundary() {
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
