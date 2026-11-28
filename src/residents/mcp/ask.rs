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

#[cfg(test)]
mod tests {
    use super::{Args, Ctx, Turn, TurnOutcome, handle, wait_for};
    use crate::runners::RunResult;
    use serde_json::json;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::time::Duration;

    /// Un Ctx sur un workspace temporaire unique (pid + nom) : aucun serveur, aucun réseau.
    fn ctx(agent: &str) -> Arc<Ctx> {
        let ws = std::env::temp_dir().join(format!("swarm-mcp-ask-{}-{agent}", std::process::id()));
        Arc::new(Ctx::new(agent, crate::agents::Runner::Claude, ws, "swarm-control-1".into()))
    }

    /// Récupère le tour que `handle` (dans un autre fil) vient de mettre en file.
    fn wait_turn(ctx: &Ctx) -> Turn {
        for _ in 0..400 {
            if let Some(turn) = ctx.inbox.drain().into_iter().next() {
                return turn;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("le tour n'est jamais entré dans la file");
    }

    #[test]
    fn un_cycle_via_est_refuse_avant_la_file() {
        let ctx = ctx("carol");
        let args = Args {
            prompt: "boucle".into(),
            from: Some("bob".into()),
            via: Some(vec!["alice".into(), "carol".into()]),
            timeout_ms: None,
            model: None,
        };
        let error = handle(&ctx, args).unwrap_err();
        assert!(error.message.starts_with("CYCLE"), "{error:?}");
        assert!(error.message.contains("[alice, carol]"), "{error:?}");
        assert_eq!(ctx.inbox.len(), 0, "rien n'a été mis en file");
    }

    #[test]
    fn une_chaine_via_est_traversee_et_rendue_dans_la_reponse() {
        let ctx = ctx("carol");
        let worker_ctx = Arc::clone(&ctx);
        let worker = std::thread::spawn(move || {
            let args = Args {
                prompt: "relaye à carol".into(),
                from: Some("bob".into()),
                via: Some(vec!["alice".into()]),
                timeout_ms: Some(3000),
                model: None,
            };
            handle(&worker_ctx, args)
        });
        let turn = wait_turn(&ctx);
        assert_eq!(turn.via, ["alice"], "la chaîne demandée par le pair est conservée");
        assert_eq!(turn.from.as_deref(), Some("bob"));
        turn.complete(
            RunResult {
                result: "voilà".into(),
                session_id: Some("s1".into()),
                ..Default::default()
            },
            12,
            34,
        );
        let reply = worker.join().unwrap().expect("demande aboutie");
        assert_eq!(reply["agent"], json!("carol"));
        assert_eq!(reply["reply"], json!("voilà"));
        assert_eq!(reply["sessionId"], json!("s1"));
        assert_eq!(reply["via"], json!(["alice", "carol"]), "l'agent s'ajoute en fin de chaîne");
        assert_eq!(reply["queuedMs"], json!(12));
        assert_eq!(reply["durationMs"], json!(34));
    }

    #[test]
    fn une_reponse_en_erreur_resort_en_upstream_avec_le_detail() {
        let ctx = ctx("carol");
        let worker_ctx = Arc::clone(&ctx);
        let worker = std::thread::spawn(move || {
            let args = Args {
                prompt: "boum".into(),
                from: None,
                via: None,
                timeout_ms: Some(3000),
                model: None,
            };
            handle(&worker_ctx, args)
        });
        let turn = wait_turn(&ctx);
        turn.complete(RunResult::err("boum"), 0, 5);
        let error = worker.join().unwrap().unwrap_err();
        assert!(error.message.contains("boum"), "{error:?}");
        assert!(error.message.contains("carol"), "le détail JSON nomme l'agent : {error:?}");
    }

    #[test]
    fn au_dela_du_delai_le_tour_est_annule_et_l_appel_rejete() {
        let ctx = ctx("dave");
        let worker_ctx = Arc::clone(&ctx);
        let worker = std::thread::spawn(move || {
            let args = Args {
                prompt: "personne ne répond".into(),
                from: None,
                via: None,
                timeout_ms: Some(30),
                model: None,
            };
            handle(&worker_ctx, args)
        });
        let turn = wait_turn(&ctx);
        let cancelled = Arc::clone(&turn.cancelled);
        let error = worker.join().unwrap().unwrap_err();
        assert!(error.message.contains("pas de réponse après 30 ms"), "{error:?}");
        assert!(cancelled.load(Ordering::SeqCst), "le jeton du tour a été levé à l'abandon");
    }

    #[test]
    fn un_agent_arrete_avant_de_repondre_est_une_erreur_interne() {
        let (sender, receiver) = channel::<TurnOutcome>();
        drop(sender);
        let cancelled = AtomicBool::new(false);
        let Err(error) = wait_for(&receiver, Duration::from_secs(3600), &cancelled) else {
            panic!("attendu : erreur d'agent arrêté");
        };
        assert!(error.message.contains("s'est arrêté avant de répondre"), "{error:?}");
        assert!(!cancelled.load(Ordering::SeqCst), "rien à annuler : le tour n'existe plus");
    }
}
