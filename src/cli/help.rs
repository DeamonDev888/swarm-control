//! Texte d'aide.

pub fn text() -> String {
    format!(
        "swarm-control {} — agents persistants (Claude Code, Hermes, Antigravity) en MCP\n\n\
         USAGE (instance) : swarm-control [--port N] [--host H] [--workspace DIR] [--instance N] [--resident AGENT[,AGENT…]]\n\
         \x20                  | swarm-control <sous-commande d'instance>\n\
         USAGE (agent)    : swarm-control --agent AGENT [--port N] [--bridge N] [--host H] [--workspace DIR]\n\
         \x20                  ou le binaire copié sous le nom de l'agent (`sniper`, `sniper.exe`)\n\
         \x20                  | sniper <sous-commande d'agent>\n\n\
         Sous-commandes d'agent (binaire renommé ou --agent) :\n\
         \x20 serve                sert l'agent (comportement par défaut sans sous-commande)\n\
         \x20 ask \"<prompt>\"       un tour : via son serveur s'il tourne, sinon exécuté ici\n\
         \x20                      (même bail de run, même session reprise)\n\
         \x20 status               état de l'agent (file, tour en cours, session)\n\
         \x20 history [-n N]       les N derniers tours du journal (défaut 20)\n\
         \x20 reset                oublie la session (le prochain tour repart à neuf)\n\
         \x20 config               fichiers de l'agent (secrets masqués)\n\
         \x20 schema <outil>       schéma MCP d'un outil de l'agent (ask, status, …)\n\n\
         Sous-commandes d'instance :\n\
         \x20 agents               liste les agents et les résidents annoncés\n\
         \x20 run <agent> \"<prompt>\"  un tour d'agent (à son hôte s'il est résident ailleurs)\n\
         \x20 runs                 runs vivants ici et derniers runs sur disque\n\
         \x20 doctor               diagnostic du workspace (annonces, baux, runs orphelins)\n\n\
         --port N              port d'écoute (défaut: premier libre de 23080..=23099)\n\
         --host H              adresse d'écoute (défaut 127.0.0.1)\n\
         --workspace DIR       racine du workspace (.claude/, .mcp.json, bridge/)\n\
         --instance N          numéro d'instance → serveur `swarm-control-N` (défaut: premier libre)\n\
         --resident AGENT,…    rend ces agents résidents au démarrage (répétable)\n\
         --agent AGENT         mode agent : ce process ne sert que cet agent (serveur `swarm-AGENT`)\n\
         --bridge N            mode agent : pont HTTP compatible overmind-bridge sur le port N\n\
         \x20                     (POST /send, POST /reset, GET /status, GET /health)\n\
         --runner R            sous-commandes : claude | hermes | antigravity (si l'agent est ambigu)\n\
         --debug               diagnostic : une ligne par requête MCP, commande lancée\n\
         \x20                     (secrets masqués), session reprise ou oubliée, retente,\n\
         \x20                     bail pris ou repris — idem SWARM_DEBUG=1\n\
         --headless            sans terminal (systemd) ; TUI par défaut dans un terminal (à venir)\n\
         --help, --version",
        env!("CARGO_PKG_VERSION")
    )
}
