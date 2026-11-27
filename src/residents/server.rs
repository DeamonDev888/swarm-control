//! Le serveur MCP propre à un agent résident.

use super::ctx::Ctx;
use super::mcp;
use litemcp::http::{BoundServer, ServerHandle};
use litemcp::prelude::ServerBuilder;
use std::sync::Arc;

/// Construit et lie le serveur de l'agent : nom `swarm-<agent>`, annoncé dans le registre
/// LiteMCP, sur `listen` (`host:port`) ou, sans, sur le premier port libre de 23080..=23099
/// (repli éphémère si la tranche est pleine).
pub fn bind(ctx: &Arc<Ctx>, listen: Option<&str>) -> Result<(BoundServer, ServerHandle), String> {
    let builder = ServerBuilder::new(&ctx.name(), env!("CARGO_PKG_VERSION"))
        // Un tour dure des minutes ; `ask` borne lui-même son attente.
        .no_tool_timeout()
        .announce();
    let builder = mcp::register(builder, Arc::clone(ctx));
    let bound = match listen {
        Some(addr) => builder.bind(addr),
        None => builder.bind_default(),
    }
    .map_err(|e| format!("écoute impossible pour l'agent '{}': {e}", ctx.agent))?;
    let handle = bound.handle();
    Ok((bound, handle))
}
