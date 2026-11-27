//! Outil `ask` : une demande à l'agent, mise en file, réponse rendue.

use super::chain;
use crate::residents::ctx::Ctx;
use crate::residents::turn::{Turn, TurnOutcome};
use litemcp::cancel::CancelToken;
use litemcp::prelude::*;
use litemcp::zod;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const DESCRIPTION: &str = "Pose une question à cet agent. La demande entre dans sa file : ses tours \
    s'exécutent un par un, sur la même session (il se souvient). `from` / `via` tracent la chaîne \
    d'agents (un cycle est refusé). timeoutMs borne l'attente (défaut 1 h) ; stop interrompt un tour.";

/// Attente par défaut : au-delà du délai d'un run (15 min) et d'une petite file.
const DEFAULT_WAIT: Duration = Duration::from_secs(3600);
const POLL: Duration = Duration::from_millis(200);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Args {
    prompt: String,
    from: Option<String>,
    via: Option<Vec<String>>,
    timeout_ms: Option<u64>,
    model: Option<String>,
}

pub fn register(builder: ServerBuilder, ctx: Arc<Ctx>) -> ServerBuilder {
    let schema = zod::object()
        .field("prompt", zod::describe("La demande faite à l'agent", zod::string().min(1)))
        .opt("from", zod::describe("Agent qui demande (traçabilité)", zod::string().min(1)))
        .opt(
            "via",
            zod::describe("Chaîne des agents déjà traversés", zod::array(zod::string().min(1))),
        )
        .opt(
            "timeoutMs",
            zod::describe(
                "Attente maximale en ms (défaut 3600000)",
                zod::uint().min(1000).max(3_600_000),
            ),
        )
        .opt("model", zod::describe("Modèle pour ce tour", zod::string().min(1)))
        .strict();
    builder.tool_parse("ask", DESCRIPTION, schema, move |args: Args| handle(&ctx, args))
}

fn handle(ctx: &Arc<Ctx>, args: Args) -> ToolResult {
    let via = args.via.unwrap_or_default();
    chain::check(&ctx.agent, &via).map_err(ToolError::invalid_input)?;

    let (turn, outcome) = Turn::with_via(args.prompt, args.from, args.model, via.clone());
    let cancelled = Arc::clone(&turn.cancelled);
    ctx.submit(turn).map_err(ToolError::upstream)?;

    let timeout = args.timeout_ms.map_or(DEFAULT_WAIT, Duration::from_millis);
    let outcome = wait_for(&outcome, timeout, &cancelled)?;
    reply(ctx, via, outcome)
}

/// Attend la réponse ; si l'appelant renonce (délai ou déconnexion), le tour est annulé
/// tant qu'il n'a pas démarré (un tour déjà lancé se termine, ou se tue par `stop`).
fn wait_for(
    outcome: &Receiver<TurnOutcome>,
    timeout: Duration,
    cancelled: &AtomicBool,
) -> Result<TurnOutcome, ToolError> {
    let deadline = Instant::now() + timeout;
    let client = litemcp::cancel::token();
    loop {
        match outcome.recv_timeout(POLL) {
            Ok(done) => return Ok(done),
            Err(RecvTimeoutError::Disconnected) => {
                return Err(ToolError::internal("l'agent s'est arrêté avant de répondre"));
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if Instant::now() >= deadline {
            cancelled.store(true, Ordering::SeqCst);
            return Err(ToolError::timeout(format!(
                "pas de réponse après {} ms : la demande est annulée si elle n'a pas démarré",
                timeout.as_millis()
            )));
        }
        if client.as_ref().is_some_and(CancelToken::is_cancelled) {
            cancelled.store(true, Ordering::SeqCst);
            return Err(ToolError::timeout("annulé par le client"));
        }
    }
}

fn reply(ctx: &Ctx, mut via: Vec<String>, outcome: TurnOutcome) -> ToolResult {
    via.push(ctx.agent.clone());
    let result = outcome.result;
    let out = json!({
        "agent": ctx.agent,
        "reply": result.result,
        "sessionId": result.session_id,
        "queuedMs": outcome.queued_ms,
        "durationMs": outcome.duration_ms,
        "via": via,
    });
    match result.error {
        Some(error) => Err(ToolError::upstream(format!("{error}\n{out}"))),
        None => Ok(out),
    }
}
