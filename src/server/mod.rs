//! Serveur MCP LiteMCP : construction, écoute, annonce.

mod auth;
mod banner;
mod bind;
mod builder;

pub(crate) use auth::{authorized, check_exposure, guard, token};

use std::path::Path;

/// Construit le serveur, l'annonce sur stderr, puis sert jusqu'à l'arrêt.
pub fn serve(name: &str, host: &str, port: Option<u16>, workspace: &Path) -> std::io::Result<()> {
    let token = auth::token();
    // Sans port explicite, `bind_default` écoute en loopback : seule une écoute explicite est
    // regardée ici.
    let exposed = if port.is_some() { host } else { "127.0.0.1" };
    auth::check_exposure(exposed, token.as_deref()).map_err(std::io::Error::other)?;
    let builder = auth::guard(builder::build(name), token.as_deref());
    let bound = bind::bind(builder, host, port)?;
    banner::announce(name, &bound.local_addr()?.to_string(), workspace);
    bound.serve()
}
