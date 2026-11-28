//! Routes du pont : `/send` (synchrone ou `{"async":true}`), `/reset`, `/status`,
//! `/health` et `/turn/<turnId>` pour l'état d'un envoi asynchrone.

use super::dialect::{self, Dialect};
use super::http::{self, Request};
use crate::residents::ctx::Ctx;
use crate::residents::turn::{Turn, TurnOutcome};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Attente maximale d'une réponse : au-delà du délai d'un run (15 min) et d'une petite file
/// (le routeur Discord attend lui-même 20 min).
const WAIT: Duration = Duration::from_secs(3600);
/// Période de surveillance du client pendant l'attente.
const POLL: Duration = Duration::from_millis(200);
/// Un tour asynchrone reste lisible par `GET /turn` une heure après son inscription,
/// le temps d'être ramassé ; au-delà il est oublié (`404`).
const TURN_TTL: Duration = WAIT;

/// Demandes en attente maximum par agent : au-delà, `/send` répond `429` immédiatement.
/// C'est le plafond de la **file** de l'agent (tours pas encore démarrés), distinct du
/// plafond de connexions HTTP [`super::MAX_CONNECTIONS`] : on protège l'agent d'un
/// engorgement, pas le serveur d'un épuisement de fils.
const QUEUE_CAP: usize = 32;

pub fn handle(
    ctx: &Arc<Ctx>,
    turns: &Arc<AsyncTurns>,
    dialect: Dialect,
    request: &Request,
    stream: &mut TcpStream,
) -> (u16, Value) {
    // `/health` reste une sonde ouverte ; le reste exige le jeton partagé s'il est configuré.
    if request.path != "/health" {
        let token = crate::server::token();
        if !crate::server::authorized(request.header("Authorization"), token.as_deref()) {
            return (401, json!({"error": "jeton requis (Authorization: Bearer)"}));
        }
    }
    match (request.method.as_str(), request.path.as_str()) {
        ("POST", "/send") => send(ctx, turns, dialect, &request.body, stream),
        ("POST", "/reset") => {
            let had = ctx.reset_session();
            (200, json!({"ok": true, "agent": ctx.agent, "hadSession": had}))
        }
        ("GET", "/status") => (200, ctx.status_json()),
        ("GET", "/health") => (200, json!({"status": "ok", "agent": ctx.agent})),
        ("GET", path) if path.starts_with("/turn/") => turn_status(turns, &path["/turn/".len()..]),
        (_, "/send" | "/reset" | "/status" | "/health") => {
            (405, json!({"error": format!("méthode {} non permise", request.method)}))
        }
        // `/turn/<id>` existe ; `/turn` seul n'est pas une route (`404`).
        (_, path) if path.starts_with("/turn/") => {
            (405, json!({"error": format!("méthode {} non permise", request.method)}))
        }
        _ => (404, json!({"error": "route inconnue"})),
    }
}

fn send(
    ctx: &Arc<Ctx>,
    turns: &Arc<AsyncTurns>,
    dialect: Dialect,
    body: &[u8],
    stream: &mut TcpStream,
) -> (u16, Value) {
    let body: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return (400, json!({"error": format!("JSON invalide: {e}")})),
    };
    let Some(message) = dialect::text(body.get("message")).filter(|m| !m.trim().is_empty()) else {
        return (400, json!({"error": "Message is required"}));
    };
    // Plafond de file : refus immédiat avant toute soumission (429, distinct du 503
    // d'arrêt et du 503 de plafond de connexions).
    let pending = ctx.inbox.len();
    if pending >= QUEUE_CAP {
        return (
            429,
            json!({
                "error": "file pleine : trop de demandes en attente pour cet agent",
                "pending": pending,
                "cap": QUEUE_CAP,
            }),
        );
    }
    let (turn, outcome) = Turn::new(dialect.prompt(&body, &message), dialect.origin(&body), None);
    let cancelled = Arc::clone(&turn.cancelled);
    if let Err(e) = ctx.submit(turn) {
        return (503, json!({"error": e}));
    }
    // `{"async":true}` : `202 {turnId}` tout de suite ; un fil de surveillance publie
    // l'issue du tour pour `GET /turn/<turnId>`.
    if !matches!(body.get("async"), Some(Value::Bool(true))) {
        return wait(ctx, dialect, &outcome, &cancelled, stream);
    }
    let id = turns.register(dialect);
    let (watch_ctx, watch_turns, watch_id) = (Arc::clone(ctx), Arc::clone(turns), id.clone());
    let surveille = std::thread::Builder::new()
        .name("swarm-bridge-async".into())
        .spawn(move || watch(watch_ctx, watch_turns, watch_id, outcome, cancelled));
    if let Err(e) = surveille {
        let e = format!("fil de surveillance impossible: {e}");
        turns.fail(&id, &e);
        return (500, json!({"error": e}));
    }
    (202, json!({"turnId": id}))
}

/// Attend la réponse du fil de travail. Si le client HTTP s'est déconnecté, le tour est
/// annulé tant qu'il n'a pas démarré : le routeur Discord ne lirait de toute façon personne.
fn wait(
    ctx: &Ctx,
    dialect: Dialect,
    outcome: &std::sync::mpsc::Receiver<TurnOutcome>,
    cancelled: &AtomicBool,
    stream: &mut TcpStream,
) -> (u16, Value) {
    let deadline = Instant::now() + WAIT;
    loop {
        match outcome.recv_timeout(POLL) {
            Ok(done) => return reply(done, dialect),
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

/// Surveille un tour asynchrone : son issue (ou l'arrêt de l'agent, ou le délai dépassé)
/// devient l'état publié par `GET /turn/<turnId>`. Personne n'attend sur la socket : la
/// déconnexion du demandeur n'annule rien, c'est le but de l'asynchrone.
fn watch(
    ctx: Arc<Ctx>,
    turns: Arc<AsyncTurns>,
    id: String,
    outcome: std::sync::mpsc::Receiver<TurnOutcome>,
    cancelled: Arc<AtomicBool>,
) {
    let deadline = Instant::now() + WAIT;
    loop {
        match outcome.recv_timeout(POLL) {
            Ok(done) => return turns.complete(&id, done),
            Err(RecvTimeoutError::Disconnected) => {
                return turns.fail(&id, "l'agent s'est arrêté avant de répondre");
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if Instant::now() >= deadline || ctx.stopping() {
            cancelled.store(true, Ordering::SeqCst);
            return turns.fail(&id, "pas de réponse de l'agent à temps");
        }
    }
}

fn reply(done: TurnOutcome, dialect: Dialect) -> (u16, Value) {
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
                "result": dialect.reply(&result.result),
                "session_id": result.session_id,
            }),
        ),
    }
}

/// `GET /turn/<turnId>` : l'état publié d'un envoi asynchrone.
fn turn_status(turns: &AsyncTurns, id: &str) -> (u16, Value) {
    match turns.status(id) {
        Some(state) => (200, state),
        None => (404, json!({"error": "tour inconnu ou expiré"})),
    }
}

/// L'état d'un tour asynchrone : `running` (en file ou en exécution — la machinerie des
/// tours ne signale que l'achèvement), puis `done` (avec la réponse façonnée selon le
/// dialecte inscrit) ou `error`. Une issue réelle peut remplacer un délai dépassé.
struct Entry {
    dialect: Dialect,
    state: State,
    answer: Option<Answer>,
    error: Option<String>,
    /// Inscription (ou dernière publication), pour l'oubli [`TURN_TTL`].
    at: Instant,
}

enum State {
    Running,
    Done,
    Error,
}

impl State {
    fn name(&self) -> &'static str {
        match self {
            State::Running => "running",
            State::Done => "done",
            State::Error => "error",
        }
    }
}

struct Answer {
    text: String,
    session_id: Option<String>,
}

/// Registre des envois asynchrones d'un pont : `POST /send {"async":true}` y inscrit chaque
/// tour sous un identifiant (`202 {turnId}`), un fil de surveillance y publie son issue, et
/// `GET /turn/<turnId>` la lit.
#[derive(Default)]
pub struct AsyncTurns {
    turns: Mutex<HashMap<String, Entry>>,
    next: AtomicU64,
}

impl AsyncTurns {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
        self.turns.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Oublie les tours jamais ramassés (mémoire bornée même sans `GET`).
    fn prune(&self) {
        self.lock().retain(|_, entry| entry.at.elapsed() < TURN_TTL);
    }

    /// Inscrit un tour sous un identifiant neuf, initialement `running`.
    fn register(&self, dialect: Dialect) -> String {
        let id = self.next.fetch_add(1, Ordering::SeqCst).to_string();
        let mut turns = self.lock();
        turns.retain(|_, entry| entry.at.elapsed() < TURN_TTL);
        turns.insert(
            id.clone(),
            Entry { dialect, state: State::Running, answer: None, error: None, at: Instant::now() },
        );
        id
    }

    /// Publie l'issue d'un tour ; la réponse est façonnée selon le dialecte inscrit.
    fn complete(&self, id: &str, done: TurnOutcome) {
        let mut turns = self.lock();
        let Some(entry) = turns.get_mut(id) else {
            return; // expiré : plus personne ne le lira
        };
        let result = done.result;
        match result.error {
            Some(error) => {
                entry.state = State::Error;
                entry.answer = None;
                entry.error = Some(error);
            }
            None => {
                entry.state = State::Done;
                entry.answer = Some(Answer {
                    text: entry.dialect.reply(&result.result),
                    session_id: result.session_id,
                });
                entry.error = None;
            }
        }
        entry.at = Instant::now();
    }

    /// Publie une erreur sans tour exécuté (arrêt de l'agent, délai dépassé, fil perdu).
    fn fail(&self, id: &str, message: &str) {
        let mut turns = self.lock();
        let Some(entry) = turns.get_mut(id) else { return };
        entry.state = State::Error;
        entry.answer = None;
        entry.error = Some(message.to_string());
        entry.at = Instant::now();
    }

    /// L'état publié ; `None` = identifiant inconnu ou expiré.
    fn status(&self, id: &str) -> Option<Value> {
        self.prune();
        let turns = self.lock();
        let entry = turns.get(id)?;
        let mut state = json!({"turnId": id, "state": entry.state.name()});
        match (&entry.state, &entry.answer, &entry.error) {
            (State::Done, Some(answer), _) => {
                state["result"] = answer.text.clone().into();
                state["session_id"] = answer.session_id.clone().into();
            }
            (State::Error, _, Some(error)) => {
                state["error"] = error.clone().into();
            }
            _ => {}
        }
        Some(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::Runner;
    use crate::runners::RunResult;
    use serde_json::json;
    use std::net::TcpListener;
    use std::sync::mpsc::channel;

    /// (ctx, extrémité « serveur » d'une connexion, extrémité client encore connectée).
    fn connected(name: &str) -> (Arc<Ctx>, TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        let ws =
            std::env::temp_dir().join(format!("swarm-bridge-routes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ws);
        (Arc::new(Ctx::new(name, Runner::Claude, ws, "test".into())), server, client)
    }

    fn request(method: &str, path: &str, body: &[u8]) -> Request {
        Request {
            method: method.into(),
            path: path.into(),
            headers: Vec::new(),
            body: body.to_vec(),
        }
    }

    /// La route `/status` publie la classe posée par le modèle dans la définition :
    /// `sniperbot_analyst` de classe `discord` reste `discord` (TODO §4.1) — le même
    /// document que l'outil `status` (il passe par `status_json`).
    #[test]
    fn la_route_status_du_pont_publie_la_classe_de_la_definition() {
        // `handle` passe par `server::token`, qui lit la config globale (init idempotent,
        // une fois par process de test — même approche que runners/claude::tests).
        let _ = crate::config::init(None);
        if crate::server::token().is_some() {
            return;
        }
        let (ctx, mut server, _client) = connected("sniperbot_analyst");
        let turns = Arc::new(AsyncTurns::default());
        crate::store::write_json_atomic(
            &crate::agents::claude_settings_path(&ctx.workspace, &ctx.agent),
            &json!({ "env": {}, "runner": "claude", "template": "discord", "class": "discord" }),
        )
        .unwrap();
        let requete = request("GET", "/status", b"");
        let (code, body) = handle(&ctx, &turns, Dialect::Discord, &requete, &mut server);
        assert_eq!(code, 200);
        assert_eq!(body["agent"], json!("sniperbot_analyst"));
        assert_eq!(body["class"], json!("discord"), "classe du modèle, jamais déduite du nom");
        let _ = std::fs::remove_dir_all(&ctx.workspace);
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
            let status = wait(&waiter_ctx, Dialect::Discord, &outcome, &cancelled, &mut server).0;
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

    /// `POST /send {"async":true}` répond `202 {turnId}` sans attendre ; le tour est
    /// lisible `running` par `GET /turn/<turnId>`, et l'arrêt de l'agent le publie `error`.
    #[test]
    fn un_envoi_asynchrone_repond_202_et_le_tour_devient_lisible() {
        let _ = crate::config::init(None);
        if crate::server::token().is_some() {
            return;
        }
        let (ctx, mut server, _client) = connected("async");
        let turns = Arc::new(AsyncTurns::default());
        let envoi = request("POST", "/send", br#"{"async":true,"message":"salut"}"#);
        let (code, body) = handle(&ctx, &turns, Dialect::Discord, &envoi, &mut server);
        assert_eq!(code, 202, "{body}");
        let id = body["turnId"].as_str().expect("turnId").to_string();

        // En file (aucun fil de travail ici) : `running`.
        let (code, etat) = handle(
            &ctx,
            &turns,
            Dialect::Discord,
            &request("GET", &format!("/turn/{id}"), b""),
            &mut server,
        );
        assert_eq!(code, 200, "{etat}");
        assert_eq!(etat["state"], json!("running"));

        // Mauvaise méthode et identifiant inconnu.
        let (code, _) = handle(
            &ctx,
            &turns,
            Dialect::Discord,
            &request("POST", &format!("/turn/{id}"), b"{}"),
            &mut server,
        );
        assert_eq!(code, 405);
        let (code, manquant) = handle(
            &ctx,
            &turns,
            Dialect::Discord,
            &request("GET", "/turn/inconnu", b""),
            &mut server,
        );
        assert_eq!(code, 404, "{manquant}");

        // L'arrêt de l'agent échoue le tour encore en file : l'état publié devient `error`.
        ctx.begin_stop();
        let echeance = Instant::now() + Duration::from_secs(5);
        let etat = loop {
            let (code, etat) = handle(
                &ctx,
                &turns,
                Dialect::Discord,
                &request("GET", &format!("/turn/{id}"), b""),
                &mut server,
            );
            assert_eq!(code, 200, "{etat}");
            match etat["state"].as_str() {
                Some("running") => {
                    assert!(Instant::now() < echeance, "le tour ne finit jamais : {etat}");
                    std::thread::sleep(Duration::from_millis(20));
                }
                Some("error") => break etat,
                other => panic!("état inattendu {other:?} : {etat}"),
            }
        };
        assert!(etat["error"].as_str().unwrap().contains("STOPPED"), "{etat}");
        let _ = std::fs::remove_dir_all(&ctx.workspace);
    }

    /// Le plafond de file : au-delà de [`QUEUE_CAP`] demandes en attente, `/send` répond
    /// `429` sans rien soumettre ; une place libérée, l'envoi passe (`202`).
    #[test]
    fn au_dela_du_plafond_de_file_send_repond_429_sans_soumettre() {
        let _ = crate::config::init(None);
        if crate::server::token().is_some() {
            return;
        }
        let (ctx, mut server, _client) = connected("plafond-file");
        let turns = Arc::new(AsyncTurns::default());
        for i in 0..QUEUE_CAP {
            ctx.submit(Turn::new(format!("t{i}"), None, None).0).unwrap();
        }
        let envoi = request("POST", "/send", br#"{"async":true,"message":"encore"}"#);
        let (code, body) = handle(&ctx, &turns, Dialect::Raw, &envoi, &mut server);
        assert_eq!(code, 429, "{body}");
        assert!(body["error"].as_str().unwrap().contains("file pleine"), "{body}");
        assert_eq!(ctx.inbox.len(), QUEUE_CAP, "rien de plus n'a été mis en file");

        // Une place libérée, l'envoi est accepté.
        assert_eq!(ctx.clear_queue("test"), QUEUE_CAP);
        let (code, body) = handle(&ctx, &turns, Dialect::Raw, &envoi, &mut server);
        assert_eq!(code, 202, "{body}");
        // Le fil de surveillance du tour accepté ne doit pas survivre au test.
        ctx.begin_stop();
        let _ = std::fs::remove_dir_all(&ctx.workspace);
    }

    /// Le registre publie les états et façonne la réponse selon le dialecte inscrit :
    /// vidée si déjà posté pour Discord/WhatsApp, jamais pour `raw`.
    #[test]
    fn le_registre_publie_les_etats_selon_le_dialecte() {
        const DEJA_POSTE: &str = "Message envoyé sur le canal, ID: 1531826331875147857";
        let outcome = |texte: &str| TurnOutcome {
            result: RunResult {
                result: texte.into(),
                session_id: Some("s1".into()),
                ..Default::default()
            },
            queued_ms: 3,
            duration_ms: 4,
        };

        let turns = AsyncTurns::default();
        let fait = turns.register(Dialect::Discord);
        turns.complete(&fait, outcome(DEJA_POSTE));
        let etat = turns.status(&fait).expect("le tour existe");
        assert_eq!(etat["state"], json!("done"));
        assert_eq!(etat["result"], json!(""), "déjà posté (discord) : réponse vidée");
        assert_eq!(etat["session_id"], json!("s1"));

        let brut = turns.register(Dialect::Raw);
        turns.complete(&brut, outcome(DEJA_POSTE));
        let etat = turns.status(&brut).unwrap();
        assert_eq!(etat["result"], json!(DEJA_POSTE), "raw : jamais vidée");

        let rate = turns.register(Dialect::Whatsapp);
        turns.fail(&rate, "boum");
        let etat = turns.status(&rate).unwrap();
        assert_eq!((etat["state"].as_str(), etat["error"].as_str()), (Some("error"), Some("boum")));

        assert!(turns.status("inconnu").is_none(), "identifiant inconnu");
    }

    /// Un tour vieilli au-delà du TTL n'est plus lisible (`404` côté route).
    #[test]
    fn un_tour_vieilli_n_est_plus_lisible() {
        let turns = AsyncTurns::default();
        let id = turns.register(Dialect::Raw);
        turns.lock().get_mut(&id).unwrap().at = Instant::now() - TURN_TTL - Duration::from_secs(1);
        assert!(turns.status(&id).is_none(), "expiré : oublié");
    }
}
