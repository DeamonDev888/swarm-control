//! Les 10 outils MCP de l'instance. Un outil = un fichier (ou dossier) avec sa fonction
//! `register`, qui déclare à la fois son schéma et son handler.
//!
//! | outil                 | rôle                                                        |
//! |-----------------------|-------------------------------------------------------------|
//! | `run_agent`           | lance un agent, reprise de session automatique              |
//! | `run_agents_parallel` | plusieurs agents en parallèle                               |
//! | `agent_control`       | status / stream / wait / kill / list des runs               |
//! | `agent_start`         | rend un agent **résident** (serveur MCP propre, file)       |
//! | `agent_stop`          | arrête un agent résident                                    |
//! | `create_agent`        | crée un agent persistant (`resident:true` pour l'héberger)  |
//! | `list_agents`         | liste les agents (et ceux qui sont résidents)               |
//! | `get_agent_configs`   | lit les fichiers d'un agent (secrets masqués)               |
//! | `update_agent_config` | modifie modèle / MCP / env / prompt / fichier               |
//! | `delete_agent`        | supprime un agent et sa session                             |
//!
//! Les outils d'un agent résident (`ask`, `status`, `stop`) sont dans `residents/mcp/`.

mod agent_control;
mod agent_start;
mod agent_stop;
mod create_agent;
mod delete_agent;
mod errors;
mod get_agent_configs;
mod list_agents;
mod run_agent;
mod run_agents_parallel;
mod run_args;
mod schema;
mod update_agent_config;

use litemcp::prelude::ServerBuilder;

/// Enregistre les 10 outils sur le serveur de l'instance.
pub fn register(builder: ServerBuilder) -> ServerBuilder {
    let builder = run_agent::register(builder);
    let builder = run_agents_parallel::register(builder);
    let builder = agent_control::register(builder);
    let builder = agent_start::register(builder);
    let builder = agent_stop::register(builder);
    let builder = create_agent::register(builder);
    let builder = list_agents::register(builder);
    let builder = get_agent_configs::register(builder);
    let builder = update_agent_config::register(builder);
    delete_agent::register(builder)
}
