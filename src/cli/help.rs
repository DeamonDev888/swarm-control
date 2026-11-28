//! Texte d'aide.

pub fn text() -> String {
    format!(
        "swarm-control {} — agents persistants (Claude Code, Hermes, Antigravity) en MCP\n\n\
         USAGE (instance) : swarm-control [--port N] [--host H] [--workspace DIR] [--instance N] [--resident AGENT[,AGENT…]]\n\
         \x20                  | swarm-control <commande>\n\
         USAGE (agent)    : swarm-control --agent AGENT [--port N] [--bridge N] [--host H] [--workspace DIR]\n\
         \x20                  [--every DURÉE \"<prompt>\" [--window HH:MM-HH:MM]]\n\
         \x20                  (développement et tests) ou le binaire compilé de l'agent (`discord`,\n\
         \x20                  `discord.exe` : create_agent {{executable:true}} ou `build <agent>`)\n\
         \x20                  | discord <commande>\n\n\
         Commandes d'agent (binaire compilé ou --agent) :\n\
         \x20 serve                sert l'agent (comportement par défaut sans commande)\n\
         \x20 ask \"<prompt>\"       un tour : via son serveur s'il tourne, sinon exécuté ici\n\
         \x20                      (même bail de run, même session reprise)\n\
         \x20 status               état : hébergé ici, annoncé ailleurs, ou connu du disque\n\
         \x20 history [-n N]       les N derniers tours du journal (défaut 20) ; --before MS\n\
         \x20                      remonte page par page\n\
         \x20 reset                oublie la session (le prochain tour repart à neuf)\n\
         \x20 config [--file NOM]  fichiers de l'agent, secrets masqués (filtre par sous-chaîne)\n\
         \x20 schema <outil>       schéma d'entrée d'un outil servi par un serveur qui tourne\n\
         \x20                      (`--port N`, le serveur de l'agent, ou une instance vivante)\n\
         \x20 memory search \"…\"    mémoire native : search [-n N] [--swarm] · add [--tags a,b] ·\n\
         \x20                      forget <id> (le même fichier que les outils memory_*)\n\
         \x20 reload               relit à chaud les .env du workspace (hébergé ici, ou pair vivant)\n\n\
         Commandes d'instance :\n\
         \x20 agents               agents du workspace et leur résidence\n\
         \x20 run <agent> \"<prompt>\"  un tour d'agent (à son hôte s'il est résident ailleurs)\n\
         \x20 runs                 dernier run de chaque agent (fichiers d'état)\n\
         \x20 build <agent>        recompile le binaire dédié de l'agent (cargo build --release ;\n\
         \x20                      journal dans <workspace>/logs/build-<agent>.log)\n\
         \x20 doctor               diagnostics non destructifs du workspace\n\n\
         --port N              port d'écoute (défaut: premier libre de 23080..=23099)\n\
         --host H              adresse d'écoute (défaut 127.0.0.1)\n\
         --workspace DIR       racine du workspace (.claude/, .mcp.json, bridge/)\n\
         --instance N          numéro d'instance → serveur `swarm-control-N` (défaut: premier libre)\n\
         --resident AGENT,…    rend ces agents résidents au démarrage (répétable)\n\
         --agent AGENT         vise cet agent (mode agent pour `serve`, cible des commandes ;\n\
         \x20                     refusé par un binaire compilé, qui ne sert que son agent)\n\
         --bridge N            mode agent : pont HTTP parlant le dialecte Discord sur le port N\n\
         \x20                     (POST /send, POST /reset, GET /status, GET /health)\n\
         --runner R            claude | hermes | antigravity (si l'agent existe sous plusieurs)\n\
         --model M             modèle imposé au tour (`ask`, `run`)\n\
         --timeout-ms MS       attente maximale d'un tour (défaut 3600000)\n\
         --every DURÉE        tâche planifiée de l'agent (mode agent) : un tour \"<prompt>\" (positionnel\n\
         \x20                     de serve) toutes les DURÉE (30s, 5m, 1h30), dans la même file que les\n\
         \x20                     demandes ; prime sur le schedule de la définition de l'agent\n\
         --window HH:MM-HH:MM  fenêtre horaire locale de la tâche planifiée (ex. 09:00-18:00)\n\
         --offline             build : `cargo build --offline` (registre local déjà complet)\n\
         --json                sortie JSON brute au lieu du résumé lisible\n\
         --debug               une ligne par requête MCP, commande lancée (secrets masqués),\n\
         \x20                     session reprise ou oubliée, retente, bail pris ou repris —\n\
         \x20                     idem SWARM_DEBUG=1\n\
         --headless            sans terminal (systemd) ; TUI par défaut dans un terminal\n\
         --help, --version",
        env!("CARGO_PKG_VERSION")
    )
}
