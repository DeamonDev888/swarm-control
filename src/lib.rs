//! # swarm-control
//!
//! ![swarm-control](https://raw.githubusercontent.com/DeamonDev888/swarm-control/main/assets/pixel/hornet-pixel-banner.svg)
//!
//! Agents IA persistants (Claude Code, Hermes, Antigravity) pilotés par MCP, servis par
//! [LiteMCP](https://crates.io/crates/litemcp).
//!
//! ![Archétypes d'agents](https://raw.githubusercontent.com/DeamonDev888/swarm-control/main/assets/pixel/swarm-live-strip.svg)
//!
//! ![Agents persistants Discord, WhatsApp et Custom](https://raw.githubusercontent.com/DeamonDev888/swarm-control/main/assets/pixel/swarm-persistent-bridges.svg)
//!
//! Carte des modules (du plus bas au plus haut niveau) :
//!
//! | module      | rôle                                                          |
//! |-------------|---------------------------------------------------------------|
//! | `store`     | JSON atomique, horloge                                        |
//! | `config`    | workspace, `.env`, interpolation, home Hermes                 |
//! | `binaries`  | localisation de `claude` et `hermes`                          |
//! | `state`     | état persistant par agent (`bridge/state/`)                   |
//! | `sessions`  | session d'un agent (état + repli sur l'ancien `agents.json`)  |
//! | `history`   | journal des tours (`bridge/history/…jsonl`), rotation         |
//! | `process`   | lancement, capture, timeout, kill d'un process                |
//! | `instance`  | numéro d'instance `swarm-control-N`                           |
//! | `owners`    | baux : un seul process fait tourner / héberge un agent        |
//! | `peers`     | client MCP minimal vers un autre serveur (agent, instance)    |
//! | `registry`  | runs vivants + dernier run sur disque + balayeur              |
//! | `agents`    | création / liste / maj / suppression (Claude, Hermes)         |
//! | `runners`   | cycle de run : reprise de session, lancement, retente         |
//! | `residents` | agents résidents : serveur MCP propre, file, fil de travail   |
//! | `routing`   | run d'un agent hébergé → transmis à son hôte                  |
//! | `tools`     | les 10 outils MCP de l'instance                               |
//! | `server`    | serveur LiteMCP de l'instance                                 |
//! | `cli`       | ligne de commande                                             |

#![forbid(unsafe_code)]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/DeamonDev888/swarm-control/main/assets/logo-256.png",
    html_favicon_url = "https://raw.githubusercontent.com/DeamonDev888/swarm-control/main/assets/favicon-64.png"
)]

mod agents;
mod binaries;
mod cli;
mod config;
mod debug;
mod history;
mod instance;
mod memory;
mod owners;
mod peers;
mod process;
mod registry;
mod residents;
mod routing;
mod runners;
mod server;
mod sessions;
mod state;
mod store;
mod tools;

#[cfg(feature = "tui")]
mod tui;

pub use cli::{run, serve_agent, serve_compiled};
