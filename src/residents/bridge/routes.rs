//! Routes du pont : `/send`, `/reset`, `/status`, `/health`.

use super::discord;
use super::http::{self, Request};
use crate::residents::ctx::Ctx;
use crate::residents::turn::{Turn, TurnOutcome};
use serde_json::{Value, json};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

/// Attente maximale d'une réponse : au-delà du délai d'un run (15 min) et d'une petite file
/// (le routeur Discord attend lui-même 20 min).
const WAIT: Duration = Duration::from_secs(3600);
/// Période de surveillance du client pendant l'attente.
const POLL: Duration = Duration::from_millis(200);

pub fn handle(ctx: &Ctx, request: &Request, stream: &mut TcpStream) -> (u16, Value) {
    // `/health` reste une sonde ouverte ; le reste exige le jeton partagé s'il est configuré.
    if request.path != "/health" {
        let token = crate::server::token();
        if !crate::server::authorized(request.header("Authorization"), token.as_deref()) {
            return (401, json!({"error": "jeton requis (Authorization: Bearer)"}));
        }
    }
    match (request.method.as_str(), request.path.as_str()) {
        ("POST", "/send") => send(ctx, &request.body, stream),
        ("POST", "/reset") => {
            let had = ctx.reset_session();
            (200, json!({"ok": true, "agent": ctx.agent, "hadSession": had}))
        }
        ("GET", "/status") => (200, ctx.status_json()),
        ("GET", "/health") => (200, json!({"status": "ok", "agent": ctx.agent})),
        (_, "/send" | "/reset" | "/status" | "/health") => {
            (405, json!({"error": format!("méthode {} non permise", request.method)}))
        }
        _ => (404, json!({"error": "route inconnue"})),
    }
}

fn send(ctx: &Ctx, body: &[u8], stream: &mut TcpStream) -> (u16, Value) {
    let body: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return (400, json!({"error": format!("JSON invalide: {e}")})),
    };
    let Some(message) = text(&body["message"]).filter(|m| !m.trim().is_empty()) else {
        return (400, json!({"error": "Message is required"}));
    };
    let username = text(&body["username"]);
    let prompt = discord::context(
        &message,
        text(&body["channelId"]).as_deref(),
        username.as_deref(),
        text(&body["userId"]).as_deref(),
    );
    let from = Some(format!("discord:{}", username.as_deref().unwrap_or("?")));
    let (turn, outcome) = Turn::new(prompt, from, None);
    let cancelled = std::sync::Arc::clone(&turn.cancelled);
    if let Err(e) = ctx.submit(turn) {
        return (503, json!({"error": e}));
    }
    wait(ctx, &outcome, &cancelled, stream)
}

/// Attend la réponse du fil de travail. Si le client HTTP s'est déconnecté, le tour est
/// annulé tant qu'il n'a pas démarré : le routeur Discord ne lirait de toute façon personne.
fn wait(
    ctx: &Ctx,
    outcome: &std::sync::mpsc::Receiver<TurnOutcome>,
    cancelled: &AtomicBool,
    stream: &mut TcpStream,
) -> (u16, Value) {
    let deadline = Instant::now() + WAIT;
    loop {
        match outcome.recv_timeout(POLL) {
            Ok(done) => return reply(done),
            Err(RecvTimeoutError::Disconnected) => {
                return (500, json!({"error": "l'agent s'est arrêté avant de répondre"}));
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if http::client_gone(stream) {
            cancelled.store(true, Ordering::SeqCst);
            return (
                400,
                json!({
                    "error": "client déconnecté : demande annulée si elle n'a pas démarré",
                    "cancelled": true,
                }),
            );
        }
        if Instant::now() >= deadline || ctx.stopping() {
            cancelled.store(true, Ordering::SeqCst);
            return (
                504,
                json!({"error": "pas de réponse de l'agent à temps : demande annulée si elle n'a pas démarré"}),
            );
        }
    }
}

fn reply(done: TurnOutcome) -> (u16, Value) {
    let result = done.result;
    match result.error {
        Some(error) if error.contains("401") || error.to_lowercase().contains("authentication") => {
            (
                401,
                json!({
                    "error": "🔑 **Erreur d'authentification API (401)**\n\nLa clé API n'est pas valide ou a expiré.",
                    "details": error,
                    "auth_error": true,
                }),
            )
        }
        Some(error) => (500, json!({"error": error})),
        None => (
            200,
            json!({
                "result": discord::reply_text(&result.result),
                "session_id": result.session_id,
            }),
        ),
    }
}

/// Une chaîne, ou un nombre écrit en chaîne (les IDs Discord arrivent sous les deux formes).
fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::Runner;
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::mpsc::channel;

    /// (ctx, extrémité « serveur » d'une connexion, extrémité client encore connectée).
    fn connected(name: &str) -> (Arc<Ctx>, TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        let ws =
            std::env::temp_dir().join(format!("swarm-bridge-routes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ws);
        (Arc::new(Ctx::new("alice", Runner::Claude, ws, "test".into())), server, client)
    }

    /// C3 : un client qui se déconnecte pendant l'attente annule le tour encore en file —
    /// même mécanisme que `ask` (jeton d'annulation sondé par le fil de travail).
    #[test]
    fn a_disconnected_client_cancels_the_queued_turn() {
        let (ctx, mut server, client) = connected("déco");
        let (turn, outcome) = Turn::new("salut".into(), Some("discord:bob".into()), None);
        let cancelled = std::sync::Arc::clone(&turn.cancelled);
        ctx.submit(turn).unwrap();

        // `wait` sonde la socket toutes les 200 ms ; on attend sa réponse dans un fil
        // (aucun fil de travail ici : seul le départ du client peut la faire sortir).
        let (done, done_rx) = channel();
        let waiter_ctx = Arc::clone(&ctx);
        let waiter = std::thread::spawn(move || {
            let status = wait(&waiter_ctx, &outcome, &cancelled, &mut server).0;
            let _ = done.send((status, cancelled.load(Ordering::SeqCst)));
        });
        drop(client);
        let (status, was_cancelled) =
            done_rx.recv_timeout(Duration::from_secs(5)).expect("wait n'est pas revenu");
        assert_eq!(status, 400);
        assert!(was_cancelled, "jeton d'annulation non levé");
        waiter.join().unwrap();
        let _ = std::fs::remove_dir_all(&ctx.workspace);
    }
}
