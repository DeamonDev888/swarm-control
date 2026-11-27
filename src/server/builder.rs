//! Construction du serveur LiteMCP et enregistrement des outils.

use crate::tools;
use litemcp::prelude::ServerBuilder;

/// Serveur d'administration d'une instance, annoncé dans le registre sous son nom
/// (`swarm-control-N`). `anti_zombie` retire à l'ouverture les annonces d'instances mortes.
pub fn build(name: &str) -> ServerBuilder {
    let builder = ServerBuilder::new(name, env!("CARGO_PKG_VERSION"))
        // Un run d'agent dure des minutes : pas de plafond de 30 s sur les
        // handlers. Chaque outil borne lui-même son exécution (délai du run,
        // `timeoutMs` de wait).
        .no_tool_timeout()
        .announce()
        .anti_zombie();
    tools::register(builder)
}
