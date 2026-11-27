//! Agents **résidents** : un agent chargé en mémoire dans ce process.
//!
//! Un agent résident a :
//! - son **serveur MCP** propre (`mcp/` : `ask`, `status`, `stop`), annoncé dans le
//!   registre LiteMCP sous `swarm-<agent>` ;
//! - une **file** de demandes (`inbox.rs`) et **un fil de travail** (`worker.rs`) qui
//!   exécute les tours l'un après l'autre : une session n'est jamais reprise deux fois ;
//! - un **bail d'hébergement** (`owners`) : un seul process héberge un agent.
//!
//! En option (mode agent), un **pont HTTP** (`bridge/`) compatible avec l'`overmind-bridge`
//! Node (`/send`, `/reset`, `/status`, `/health`) met ses demandes dans la même file.
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
mod server;
mod start;
mod status;
mod stop;
mod table;
mod turn;
mod waiting;
mod worker;

pub use bridge::open as open_bridge;
pub use start::{start, start_on};
pub use stop::stop;
