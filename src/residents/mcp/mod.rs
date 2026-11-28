//! Outils MCP du serveur d'un agent résident (`swarm-<agent>`).
//!
//! | outil    | rôle                                                   |
//! |----------|--------------------------------------------------------|
//! | `ask`    | met un tour dans la file de l'agent et rend sa réponse |
//! | `status` | file, tour en cours, session, dernier tour             |
//! | `stop`   | tue le tour en cours (et vide la file si demandé)      |
//! | `history`| les derniers tours, lus dans le journal                |
//! | `session`| voir / oublier / forcer la session                     |
//! | `logs`   | fin de la sortie du dernier tour                       |
//! | `config` | fichiers de configuration (secrets masqués)            |
//! | `memory` | mémoire native : `memory_store` / `memory_search` / `memory_forget` |
//! | `reload` | relit à chaud les `.env` du workspace (PLAN §4.8)                |

mod ask;
mod chain;
mod config;
mod history;
mod logs;
mod memory;
mod reload;
mod session;
mod status;
mod stop;

use super::ctx::Ctx;
use litemcp::prelude::ServerBuilder;
use std::sync::Arc;

pub fn register(builder: ServerBuilder, ctx: Arc<Ctx>) -> ServerBuilder {
    let builder = ask::register(builder, Arc::clone(&ctx));
    let builder = status::register(builder, Arc::clone(&ctx));
    let builder = history::register(builder, Arc::clone(&ctx));
    let builder = session::register(builder, Arc::clone(&ctx));
    let builder = logs::register(builder, Arc::clone(&ctx));
    let builder = config::register(builder, Arc::clone(&ctx));
    let builder = memory::register(builder, Arc::clone(&ctx));
    let builder = reload::register(builder);
    stop::register(builder, ctx)
}
