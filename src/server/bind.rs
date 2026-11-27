//! Ouverture du socket d'écoute.

use litemcp::http::BoundServer;
use litemcp::prelude::ServerBuilder;

/// Écoute sur `host:port`, ou sur le premier port libre de 23080..=23099 sans `port`.
pub fn bind(builder: ServerBuilder, host: &str, port: Option<u16>) -> std::io::Result<BoundServer> {
    match port {
        Some(port) => builder.bind(&format!("{host}:{port}")),
        None => builder.bind_default(),
    }
}
