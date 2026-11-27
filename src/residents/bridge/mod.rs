//! Pont HTTP compatible avec l'`overmind-bridge` Node (worker Discord d'un agent).
//!
//! Un routeur Discord existant poste `POST /send {message, userId, username, channelId}` et
//! attend `{result, session_id}` : ce pont parle exactement ce dialecte, mais met chaque
//! demande dans la **file** de l'agent résident (même session, un tour à la fois).
//!
//! | route          | rôle                                                         |
//! |----------------|--------------------------------------------------------------|
//! | `POST /send`   | contexte Discord + message → un tour → `{result, session_id}` |
//! | `POST /reset`  | oublie la session : le prochain tour repart à neuf           |
//! | `GET /status`  | état de l'agent (celui de l'outil MCP `status`)              |
//! | `GET /health`  | sonde de vie                                                 |
//!
//! HTTP/1.1 minimal en `std`, une requête par connexion, un fil par connexion (un tour dure
//! des minutes) borné par [`MAX_CONNECTIONS`].

mod discord;
mod http;
mod routes;

use super::ctx::Ctx;
use super::table;
use crate::agents::Runner;
use serde_json::json;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Connexions servies en même temps ; au-delà, `503` immédiat.
const MAX_CONNECTIONS: usize = 64;

/// Ouvre le pont de l'agent résident `agent` sur `addr` (`host:port`) et rend l'adresse
/// réellement liée (port `0` = éphémère).
pub fn open(runner: Runner, agent: &str, addr: &str) -> Result<SocketAddr, String> {
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
    std::thread::Builder::new()
        .name(format!("swarm-bridge-{agent}"))
        .spawn(move || accept_loop(&ctx, &listener))
        .map_err(|e| format!("fil du pont impossible: {e}"))?;
    Ok(local)
}

fn accept_loop(ctx: &Arc<Ctx>, listener: &TcpListener) {
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
        let (ctx, done) = (Arc::clone(ctx), Arc::clone(&active));
        let spawned =
            std::thread::Builder::new().name("swarm-bridge-conn".into()).spawn(move || {
                serve_one(&ctx, stream);
                done.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn serve_one(ctx: &Ctx, mut stream: TcpStream) {
    let (status, body) = match http::read(&mut stream) {
        Ok(request) => routes::handle(ctx, &request, &mut stream),
        Err((status, message)) => (status, json!({"error": message})),
    };
    http::respond(&mut stream, status, &body);
}
