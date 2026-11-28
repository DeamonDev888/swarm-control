//! Agents **résidents** : un agent chargé en mémoire dans ce process.
//!
//! Un agent résident a :
//! - son **serveur MCP** propre (`mcp/` : `ask`, `status`, `stop`), annoncé dans le
//!   registre LiteMCP sous `swarm-<agent>` ;
//! - une **file** de demandes (`inbox.rs`) et **un fil de travail** (`worker.rs`) qui
//!   exécute les tours l'un après l'autre : une session n'est jamais reprise deux fois ;
//! - un **bail d'hébergement** (`owners`) : un seul process héberge un agent.
//!
//! En option (mode agent), un **pont HTTP** (`bridge/`) met ses demandes dans la même
//! file, dans un dialecte choisi (`--bridge-dialect` : `discord` par défaut, `whatsapp`,
//! `raw`) ; `/send` peut répondre `202 {turnId}` (envoi asynchrone), l'issue étant lisible
//! par `GET /turn/<turnId>`, et la file est plafonnée (`429`).
//!
//! Rien d'autre n'est gardé en RAM : la conversation reste dans la session du CLI
//! (`--resume`), relancé une fois par tour.

mod activity;
mod bridge;
mod control;
mod ctx;
mod host;
mod inbox;
mod info;
mod mcp;
mod reset;
mod schedule;
mod server;
mod start;
mod status;
mod stop;
mod table;
pub(crate) mod turn;
mod waiting;
mod worker;

pub use bridge::Dialect;
pub use bridge::open as open_bridge;
pub(crate) use bridge::request_dialect;
pub use ctx::Ctx;
pub use start::{start, start_on};
pub use stop::stop;

use crate::agents::Runner;
use std::sync::Arc;

/// Contexte d'un agent hébergé **par ce process** (TUI, outils internes).
pub fn ctx_of(agent: &str, runner: Runner) -> Option<Arc<Ctx>> {
    table::find(agent, Some(runner)).first().map(|host| Arc::clone(host.ctx()))
}
