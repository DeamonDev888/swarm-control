# Changelog

Format : [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/) · versionnage [SemVer](https://semver.org/lang/fr/).

Le workflow de release extrait la section `## [X.Y.Z]` correspondant au tag pour
écrire les notes de la release GitHub : **chaque version publiée doit avoir sa section**.

## [Unreleased]

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
