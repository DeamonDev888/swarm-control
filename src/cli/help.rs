//! Texte d'aide.

pub fn text() -> String {
    format!(
        "swarm-control {} — agents persistants (Claude Code, Hermes) en MCP\n\n\
         USAGE: swarm-control [--port N] [--host H] [--workspace DIR] [--instance N] [--resident AGENT[,AGENT…]]\n\n\
         --port N              port d'écoute (défaut: premier libre de 23080..=23099)\n\
         --host H              adresse d'écoute (défaut 127.0.0.1)\n\
         --workspace DIR       racine du workspace (.claude/, .mcp.json, bridge/)\n\
         --instance N          numéro d'instance → serveur `swarm-control-N` (défaut: premier libre)\n\
         --resident AGENT,…    rend ces agents résidents au démarrage (répétable)\n\
         --help, --version",
        env!("CARGO_PKG_VERSION")
    )
}
