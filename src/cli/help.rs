//! Texte d'aide.

pub fn text() -> String {
    format!(
        "swarm-control {} — agents persistants (Claude Code, Hermes) en MCP\n\n\
         USAGE (instance) : swarm-control [--port N] [--host H] [--workspace DIR] [--instance N] [--resident AGENT[,AGENT…]]\n\
         USAGE (agent)    : swarm-control --agent AGENT [--port N] [--bridge N] [--host H] [--workspace DIR]\n\
         \x20                  ou le binaire copié sous le nom de l'agent (`sniper`, `sniper.exe`)\n\n\
         --port N              port d'écoute (défaut: premier libre de 23080..=23099)\n\
         --host H              adresse d'écoute (défaut 127.0.0.1)\n\
         --workspace DIR       racine du workspace (.claude/, .mcp.json, bridge/)\n\
         --instance N          numéro d'instance → serveur `swarm-control-N` (défaut: premier libre)\n\
         --resident AGENT,…    rend ces agents résidents au démarrage (répétable)\n\
         --agent AGENT         mode agent : ce process ne sert que cet agent (serveur `swarm-AGENT`)\n\
         --bridge N            mode agent : pont HTTP compatible overmind-bridge sur le port N\n\
         \x20                     (POST /send, POST /reset, GET /status, GET /health)\n\
         --help, --version",
        env!("CARGO_PKG_VERSION")
    )
}
