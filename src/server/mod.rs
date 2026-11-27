//! Serveur MCP LiteMCP : construction, écoute, annonce.

mod banner;
mod bind;
mod builder;

use std::path::Path;

/// Construit le serveur, l'annonce sur stderr, puis sert jusqu'à l'arrêt.
pub fn serve(name: &str, host: &str, port: Option<u16>, workspace: &Path) -> std::io::Result<()> {
    let bound = bind::bind(builder::build(name), host, port)?;
    banner::announce(name, &bound.local_addr()?.to_string(), workspace);
    bound.serve()
}
