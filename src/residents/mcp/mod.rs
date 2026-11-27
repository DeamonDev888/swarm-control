//! Outils MCP du serveur d'un agent résident (`swarm-<agent>`).
//!
//! | outil    | rôle                                                   |
//! |----------|--------------------------------------------------------|
//! | `ask`    | met un tour dans la file de l'agent et rend sa réponse |
//! | `status` | file, tour en cours, session, dernier tour             |
//! | `stop`   | tue le tour en cours (et vide la file si demandé)      |

mod ask;
mod chain;
mod status;
mod stop;

use super::ctx::Ctx;
use litemcp::prelude::ServerBuilder;
use std::sync::Arc;

pub fn register(builder: ServerBuilder, ctx: Arc<Ctx>) -> ServerBuilder {
    let builder = ask::register(builder, Arc::clone(&ctx));
    let builder = status::register(builder, Arc::clone(&ctx));
    stop::register(builder, ctx)
}
