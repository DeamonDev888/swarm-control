//! Routes du pont : `/send`, `/reset`, `/status`, `/health`.

use super::discord;
use super::http::Request;
use crate::residents::ctx::Ctx;
use crate::residents::turn::{Turn, TurnOutcome};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

/// Attente maximale d'une réponse : au-delà du délai d'un run (15 min) et d'une petite file
/// (le routeur Discord attend lui-même 20 min).
const WAIT: Duration = Duration::from_secs(3600);

pub fn handle(ctx: &Ctx, request: &Request) -> (u16, Value) {
    match (request.method.as_str(), request.path.as_str()) {
        ("POST", "/send") => send(ctx, &request.body),
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

fn send(ctx: &Ctx, body: &[u8]) -> (u16, Value) {
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
    match outcome.recv_timeout(WAIT) {
        Ok(done) => reply(done),
        Err(RecvTimeoutError::Timeout) => {
            cancelled.store(true, Ordering::SeqCst);
            (
                504,
                json!({"error": "pas de réponse de l'agent à temps : demande annulée si elle n'a pas démarré"}),
            )
        }
        Err(RecvTimeoutError::Disconnected) => {
            (500, json!({"error": "l'agent s'est arrêté avant de répondre"}))
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
