//! Le serveur MCP propre à un agent résident.

use super::ctx::Ctx;
use super::mcp;
use litemcp::http::{BoundServer, ServerHandle};
use litemcp::prelude::ServerBuilder;
use std::sync::Arc;

/// Construit et lie le serveur de l'agent : nom `swarm-<agent>`, annoncé dans le registre
/// LiteMCP, sur `listen` (`host:port`) ou, sans, sur `SWARM_AGENT_PORT` s'il est défini
/// (`127.0.0.1:<port>`, `0` = port éphémère attribué par l'OS — override du harnais e2e,
/// pour qu'aucun serveur ne se dispute la tranche fixe), sinon sur le premier port libre de
/// 23080..=23099 (repli éphémère si la tranche est pleine).
pub fn bind(ctx: &Arc<Ctx>, listen: Option<&str>) -> Result<(BoundServer, ServerHandle), String> {
    let token = crate::server::token();
    let exposed = listen.and_then(|addr| addr.rsplit_once(':').map(|(host, _)| host));
    crate::server::check_exposure(exposed.unwrap_or("127.0.0.1"), token.as_deref())?;
    let builder = ServerBuilder::new(&ctx.name(), env!("CARGO_PKG_VERSION"))
        // Un tour dure des minutes ; `ask` borne lui-même son attente.
        .no_tool_timeout()
        // Une ligne par requête MCP reçue (`--debug`, PLAN §4.2).
        .on_call(|event| crate::debug!("mcp ← {} {}", event.method, event.name.unwrap_or("-")))
        .announce();
    let builder = crate::server::guard(builder, token.as_deref());
    let builder = mcp::register(builder, Arc::clone(ctx));
    let bound = match listen {
        Some(addr) => builder.bind(addr),
        None => match env_listen()? {
            Some(addr) => builder.bind(&addr),
            None => builder.bind_default(),
        },
    }
    .map_err(|e| format!("écoute impossible pour l'agent '{}': {e}", ctx.agent))?;
    let handle = bound.handle();
    Ok((bound, handle))
}

/// Adresse d'écoute demandée par `SWARM_AGENT_PORT` (override du harnais e2e, l'équivalent
/// de `--port 0` pour l'instance) : `127.0.0.1:<port>`, `0` = port éphémère. Absente ou
/// vide = pas d'override ; valeur non numérique = erreur claire, jamais un silence.
fn env_listen() -> Result<Option<String>, String> {
    match std::env::var("SWARM_AGENT_PORT") {
        Ok(raw) if !raw.is_empty() => {
            let port: u16 = raw.parse().map_err(|_| {
                format!("SWARM_AGENT_PORT : nombre 0-65535 attendu (reçu « {raw} »)")
            })?;
            Ok(Some(format!("127.0.0.1:{port}")))
        }
        _ => Ok(None),
    }
}
