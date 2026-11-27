# Changelog

Format : [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/) · versionnage [SemVer](https://semver.org/lang/fr/).

Le workflow de release extrait la section `## [X.Y.Z]` correspondant au tag pour
écrire les notes de la release GitHub : **chaque version publiée doit avoir sa section**.

## [Unreleased]

### Ajouté
- **Un exécutable par agent** : le binaire copié sous le nom d'un agent (`sniper`, `sniper.exe`),
  ou lancé avec `--agent NAME`, ne sert que cet agent (serveur MCP `swarm-<agent>`, file,
  session), sans serveur d'administration. `--port` fixe le port de son serveur MCP.
  `swarm_control::serve_agent("nom")` sert de `main` à un crate d'agent.
- **Pont HTTP compatible `overmind-bridge`** (`--bridge N`, mode agent) : `POST /send`
  (contexte `[DISCORD CONTEXT]`, réponse `{result, session_id}`, vide si l'agent a posté via
  MCP), `POST /reset`, `GET /status`, `GET /health` — un routeur Discord Node existant n'a qu'à
  changer de port. Les demandes passent par la file de l'agent.
- **Runner Antigravity (`agy`)** : création, configuration, liste et exécution d'agents via le
  CLI Antigravity en complément de Claude Code et Hermes.
- **Identité visuelle SVG animée** : bannière principale animée, console MCP multi-agents
  (`Coordinator`, `Coder`, `Explorer`, `Verifier`, `Browser`, `Monitor`) et classe d'agents
  persistants (`Hornet Discord`, `Hornet WhatsApp`, `Hornet Customisable` prêt pour tout bridge
  CLI & serveur MCP), visibles sur GitHub, crates.io et docs.rs.

### Corrigé
- Test `control` instable : l'état disque d'un run tué est attendu au lieu d'être lu aussitôt.

## [0.1.1] - 2026-11-27

### Ajouté
- Logo en pied du README (visible sur crates.io), logo et icône d'onglet sur docs.rs,
  bannière en tête de la documentation.

### Corrigé
- Unix : un PID hors bornes (0 ou au-delà de `i32::MAX`, relu par `kill` comme `-1`) était vu
  vivant par `pid_alive` et pouvait être visé par `kill_tree` ; il est désormais ignoré.

## [0.1.0] - 2026-11-26

Première version.

### Ajouté
- Serveur MCP natif (LiteMCP, Streamable HTTP stateless) avec 10 outils : `run_agent`,
  `run_agents_parallel`, `agent_control`, `agent_start`, `agent_stop`, `create_agent`,
  `list_agents`, `get_agent_configs`, `update_agent_config`, `delete_agent`.
- **Agents résidents** : `agent_start` / `create_agent resident:true` / `--resident` chargent un
  agent en mémoire avec son propre serveur MCP (`swarm-<agent>`, annoncé dans le registre
  LiteMCP), une file de demandes et un fil de travail. Outils de l'agent : `ask` (file FIFO, même
  session, refus des cycles via `via`), `status`, `stop`. Un seul hôte par agent entre instances
  (bail d'hébergement).
- **Instances numérotées** : `swarm-control-N` (premier numéro libre ou `--instance N`, réservé par
  un bail), annoncée dans le registre LiteMCP avec balayage des annonces mortes.
- **Routage** : `run_agent` / `run_agents_parallel` d'un agent résident sont transmis à son hôte
  (`routedTo` dans la réponse) ; repli à la demande sans hôte ou si l'hôte est injoignable.
- Garde anti-panique d'un run : un run qui panique se termine en échec et libère son bail.
- Agents persistants Claude Code et Hermes : reprise automatique de session
  (`bridge/state/<runner>-<agent>.json`, un fichier par agent ; l'ancien `bridge/agents.json`
  reste lu), retente à neuf si la session est périmée.
- Bail inter-process par agent (`bridge/owners/`, création exclusive, battement de cœur,
  reprise sérialisée d'un bail périmé) : plusieurs instances `swarm-control` peuvent partager
  un workspace sans jamais lancer le même agent en même temps.
- Registre de processus (mémoire + dernier run dans l'état de l'agent) : statut, PID, sortie
  en direct, `wait`, `kill` de l'arbre de processus, balayeur d'orphelins.
- Runs en arrière-plan (`background:true`) et en parallèle (`run_agents_parallel`).
- Jetons de secours Claude sur 401/429/5xx, alias de modèles, `cloneFrom` pour Hermes.
- Agents Claude (`.claude/`) et Hermes (profils) compatibles avec l'OverMind TS.
