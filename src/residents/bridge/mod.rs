//! Pont HTTP d'un agent résident (worker Discord/WhatsApp d'un agent, ou client `raw`).
//!
//! Un routeur Discord existant poste `POST /send {message, userId, username, channelId}` et
//! attend `{result, session_id}` : ce pont parle exactement ce dialecte, mais met chaque
//! demande dans la **file** de l'agent résident (même session, un tour à la fois). Avec
//! `{"async":true}`, `/send` répond `202 {turnId}` tout de suite et l'issue du tour devient
//! lisible par `GET /turn/<turnId>`.
//!
//! | route            | rôle                                                          |
//! |------------------|---------------------------------------------------------------|
//! | `POST /send`     | contexte + message → un tour → `{result, session_id}` ; avec `{"async":true}` : `202 {turnId}` |
//! | `GET /turn/<id>` | état d'un envoi asynchrone (`running`/`done`/`error` + réponse quand faite) |
//! | `POST /reset`    | oublie la session : le prochain tour repart à neuf            |
//! | `GET /status`    | état de l'agent (celui de l'outil MCP `status`)               |
//! | `GET /health`    | sonde de vie                                                  |
//!
//! Le dialecte ([`Dialect`], `--bridge-dialect`) façonne le prompt envoyé à l'agent et la
//! réponse rendue au client : `discord` (défaut), `whatsapp`, `raw`.
//!
//! HTTP/1.1 minimal en `std`, une requête par connexion, un fil par connexion (un tour dure
//! des minutes) borné par [`MAX_CONNECTIONS`] ; la file de l'agent est bornée par le
//! plafond de [`routes::QUEUE_CAP`] (`429`).

mod dialect;
mod discord;
mod http;
mod routes;

pub use dialect::Dialect;

use super::ctx::Ctx;
use super::table;
use crate::agents::Runner;
use routes::{AsyncTurns, handle};
use serde_json::json;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Connexions servies en même temps ; au-delà, `503` immédiat.
const MAX_CONNECTIONS: usize = 64;

/// Dialecte du pont demandé pour ce process (`--bridge-dialect`, posé une fois par la CLI
/// `serve` avant l'ouverture du pont ; défaut [`Dialect::Discord`]).
static REQUESTED: OnceLock<Dialect> = OnceLock::new();

/// Retient le dialecte du pont de ce process ; le premier appel gagne (un process ne sert
/// qu'un pont, mode agent).
pub fn request_dialect(dialect: Dialect) {
    let _ = REQUESTED.set(dialect);
}

/// Le dialecte demandé, ou `discord` si la CLI n'en a pas posé.
fn requested_dialect() -> Dialect {
    *REQUESTED.get().unwrap_or(&Dialect::Discord)
}

/// Ouvre le pont de l'agent résident `agent` sur `addr` (`host:port`) et rend l'adresse
/// réellement liée (port `0` = éphémère). Le dialecte est celui demandé pour le process
/// ([`request_dialect`], `discord` par défaut).
pub fn open(runner: Runner, agent: &str, addr: &str) -> Result<SocketAddr, String> {
    let dialect = requested_dialect();
    let token = crate::server::token();
    let host = addr.rsplit_once(':').map_or(addr, |(host, _)| host);
    crate::server::check_exposure(host, token.as_deref())?;
    let host = table::find(agent, Some(runner))
        .pop()
        .ok_or_else(|| format!("l'agent '{agent}' n'est pas résident dans ce process"))?;
    let ctx = Arc::clone(host.ctx());
    let listener =
        TcpListener::bind(addr).map_err(|e| format!("écoute impossible sur {addr}: {e}"))?;
    let local = listener.local_addr().map_err(|e| format!("adresse illisible: {e}"))?;
    let turns = Arc::new(AsyncTurns::default());
    std::thread::Builder::new()
        .name(format!("swarm-bridge-{agent}"))
        .spawn(move || accept_loop(&ctx, &turns, dialect, &listener))
        .map_err(|e| format!("fil du pont impossible: {e}"))?;
    Ok(local)
}

fn accept_loop(ctx: &Arc<Ctx>, turns: &Arc<AsyncTurns>, dialect: Dialect, listener: &TcpListener) {
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        if ctx.stopping() {
            return;
        }
        let Ok(mut stream) = stream else { continue };
        if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            active.fetch_sub(1, Ordering::SeqCst);
            http::respond(&mut stream, 503, &json!({"error": "trop de connexions"}));
            continue;
        }
        let (ctx, turns, done) = (Arc::clone(ctx), Arc::clone(turns), Arc::clone(&active));
        let spawned =
            std::thread::Builder::new().name("swarm-bridge-conn".into()).spawn(move || {
                serve_one(&ctx, &turns, dialect, stream);
                done.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn serve_one(ctx: &Arc<Ctx>, turns: &Arc<AsyncTurns>, dialect: Dialect, mut stream: TcpStream) {
    let (status, body) = match http::read(&mut stream) {
        Ok(request) => handle(ctx, turns, dialect, &request, &mut stream),
        Err((status, message)) => (status, json!({"error": message})),
    };
    http::respond(&mut stream, status, &body);
}

#[cfg(test)]
mod tests {
    use super::{AsyncTurns, Dialect, MAX_CONNECTIONS, http, open, serve_one};
    use crate::agents::Runner;
    use crate::residents::ctx::Ctx;
    use crate::residents::host::AgentHost;
    use crate::residents::table;
    use litemcp::prelude::ServerBuilder;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{Duration, Instant};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// Une requête `GET /health` complète (corps nul).
    const SANTE: &[u8] = b"GET /health HTTP/1.1\r\nHost: pont\r\nContent-Length: 0\r\n\r\n";

    /// Un agent résident « factice » de ce process : contexte sur un workspace
    /// temporaire, poignée d'un serveur LiteMCP jamais servi (port éphémère, donc jamais
    /// annoncé), fil de travail déjà terminé. Seul le pont HTTP de l'agent est réel.
    struct Factice {
        nom: String,
        ws: PathBuf,
    }

    impl Factice {
        fn nouveau(tag: &str) -> Self {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let nom = format!("pont-{tag}-{}-{n}", std::process::id());
            let ws =
                std::env::temp_dir().join(format!("swarm-pont-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&ws);
            let ctx = Arc::new(Ctx::new(&nom, Runner::Claude, ws.clone(), "test".into()));
            let lie = ServerBuilder::new(&ctx.name(), "test").bind("127.0.0.1:0").unwrap();
            let poignee = lie.handle();
            drop(lie);
            let travail = std::thread::Builder::new().spawn(|| {}).unwrap();
            table::insert(Arc::new(AgentHost::new(ctx, poignee, travail)));
            Factice { nom, ws }
        }
    }

    /// Retire l'hôte factice, l'arrête, puis réveille la boucle d'acceptation du pont :
    /// elle ne sort qu'en voyant l'arrêt, à la prochaine connexion.
    fn eteindre(factice: &Factice, adresse: SocketAddr) {
        if let Some(hote) = table::remove(Runner::Claude, &factice.nom) {
            hote.shutdown();
        }
        let _ = TcpStream::connect_timeout(&adresse, Duration::from_secs(2));
        let _ = std::fs::remove_dir_all(&factice.ws);
    }

    /// Lit toute la réponse d'un client (le pont ferme la connexion après avoir répondu).
    fn lire(client: &mut TcpStream) -> String {
        client.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut reponse = String::new();
        client.read_to_string(&mut reponse).unwrap();
        reponse
    }

    /// (ctx, registre des tours asynchrones, extrémité « serveur » d'une connexion,
    /// extrémité client encore connectée).
    fn paire() -> (Arc<Ctx>, Arc<AsyncTurns>, TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (serveur, _) = listener.accept().unwrap();
        let ws = std::env::temp_dir().join(format!("swarm-pont-ctx-{}", std::process::id()));
        (
            Arc::new(Ctx::new("alice", Runner::Claude, ws, "test".into())),
            Arc::new(AsyncTurns::default()),
            serveur,
            client,
        )
    }

    #[test]
    fn un_agent_non_resident_est_refuse() {
        let err = open(Runner::Claude, &format!("personne-{}", std::process::id()), "127.0.0.1:0")
            .expect_err("pas résident");
        assert!(err.contains("n'est pas résident"), "{err}");
    }

    #[test]
    fn une_ecoute_hors_loopback_est_refusee_sans_jeton() {
        // Avec un jeton configuré, l'écoute exposée est permise (règle couverte par
        // `server::auth`) : le cas « sans jeton » ne se teste que sans jeton.
        if crate::server::token().is_some() {
            return;
        }
        let err = open(Runner::Claude, "peu-importe", "0.0.0.0:0").expect_err("refusée");
        if crate::server::token().is_some() {
            return; // initialisation concurrente de la configuration : cas non pertinent
        }
        assert!(err.contains("refusée"), "{err}");
        assert!(err.contains("SWARM_TOKEN"), "{err}");
    }

    #[test]
    fn le_port_zero_rend_l_adresse_reellement_liée_et_le_pont_y_repond() {
        let factice = Factice::nouveau("port-zero");
        let adresse = open(Runner::Claude, &factice.nom, "127.0.0.1:0").unwrap();
        assert!(adresse.ip().is_loopback(), "{adresse}");
        assert_ne!(adresse.port(), 0, "port éphémère attribué");
        let mut client = TcpStream::connect(adresse).unwrap();
        client.write_all(SANTE).unwrap();
        let reponse = lire(&mut client);
        assert!(reponse.starts_with("HTTP/1.1 200"), "{reponse}");
        assert!(reponse.contains("\"status\":\"ok\""), "{reponse}");
        assert!(reponse.contains(&format!("\"agent\":\"{}\"", factice.nom)), "{reponse}");
        // Une adresse illisible est refusée explicitement, jamais remplacée en silence.
        let err = open(Runner::Claude, &factice.nom, "127.0.0.1:pasttt").expect_err("échec");
        assert!(err.contains("écoute impossible"), "{err}");
        eteindre(&factice, adresse);
    }

    #[test]
    fn au_dela_du_plafond_de_connexions_le_pont_repond_503_puis_reservit() {
        let factice = Factice::nouveau("plafond");
        let adresse = open(Runner::Claude, &factice.nom, "127.0.0.1:0").unwrap();
        // Autant de clients muets que le plafond : chacun tient un fil (lecture de
        // requête bloquée) jusqu'à sa fermeture.
        let mut muets = Vec::new();
        for _ in 0..MAX_CONNECTIONS {
            muets.push(TcpStream::connect(adresse).unwrap());
        }
        // Un de plus : refusé immédiatement.
        let mut surnumeraire = TcpStream::connect(adresse).unwrap();
        let reponse = lire(&mut surnumeraire);
        assert!(reponse.starts_with("HTTP/1.1 503"), "{reponse}");
        assert!(reponse.contains("trop de connexions"), "{reponse}");
        // Les clients partis, les fils se terminent et le pont sert de nouveau.
        drop(muets);
        let echeance = Instant::now() + Duration::from_secs(10);
        loop {
            let mut client = TcpStream::connect(adresse).unwrap();
            client.write_all(SANTE).unwrap();
            let reponse = lire(&mut client);
            if reponse.starts_with("HTTP/1.1 200") {
                break;
            }
            assert!(Instant::now() < echeance, "le pont ne sert plus : {reponse}");
            std::thread::sleep(Duration::from_millis(100));
        }
        eteindre(&factice, adresse);
    }

    #[test]
    fn une_requete_illisible_ou_incomplete_recoit_400() {
        // Ligne de requête invalide.
        let (ctx, turns, serveur, mut client) = paire();
        client.write_all(b"pas-une-requete\r\n\r\n").unwrap();
        serve_one(&ctx, &turns, Dialect::Discord, serveur);
        let reponse = lire(&mut client);
        assert!(reponse.starts_with("HTTP/1.1 400"), "{reponse}");
        assert!(reponse.contains("ligne de requête invalide"), "{reponse}");
        // Client parti avant d'envoyer quoi que ce soit.
        let (ctx, turns, serveur, client) = paire();
        drop(client);
        let echeance = Instant::now() + Duration::from_secs(5);
        let mut serveur = serveur;
        while !http::client_gone(&mut serveur) && Instant::now() < echeance {
            std::thread::sleep(Duration::from_millis(20));
        }
        serve_one(&ctx, &turns, Dialect::Discord, serveur);
    }

    #[test]
    fn une_route_inconnue_recoit_404_et_une_methode_fausse_405() {
        // Hors `/health`, le jeton partagé est exigé s'il est configuré.
        if crate::server::token().is_some() {
            return;
        }
        let (ctx, turns, serveur, mut client) = paire();
        client
            .write_all(b"GET /inconnue HTTP/1.1\r\nHost: pont\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        serve_one(&ctx, &turns, Dialect::Discord, serveur);
        let reponse = lire(&mut client);
        assert!(reponse.starts_with("HTTP/1.1 404"), "{reponse}");
        let (ctx, turns, serveur, mut client) = paire();
        client
            .write_all(b"POST /health HTTP/1.1\r\nHost: pont\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        serve_one(&ctx, &turns, Dialect::Discord, serveur);
        let reponse = lire(&mut client);
        assert!(reponse.starts_with("HTTP/1.1 405"), "{reponse}");
    }
}
