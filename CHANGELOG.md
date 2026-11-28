# Changelog

Format : [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/) · versionnage [SemVer](https://semver.org/lang/fr/).

Le workflow de release extrait la section `## [X.Y.Z]` correspondant au tag pour
écrire les notes de la release GitHub : **chaque version publiée doit avoir sa section**.

## [Unreleased]

## [0.2.0] - 2026-11-28

Version à **ruptures** : lire « Modifié » et « Retiré » avant de mettre à jour.

### Ajouté

- **Modèles d'agents** intégrés (`assets/templates/`) : `create_agent {template}` pour les 11
  agents du README, `swarm-control templates` en CLI, `list_templates` en MCP — l'instance passe
  de 10 à **11 outils** ; champ `class` publié par `status`, `list_agents` et `/status` du pont
  (le logo ne se devine plus sur le nom).
- **Mémoire native** par agent résident, sans base de données : outils MCP `memory_store` /
  `memory_search` / `memory_forget` (un souvenir par ligne dans
  `bridge/memory/<runner>-<agent>.jsonl`), portée `swarm` en lecture seule, filtrage des jetons à
  l'écriture.
- **Archétypes de l'essaim** : `run_agent {archetype}` (persona prête à l'emploi), permissions
  restreintes pour `explorer` et `verifier` ; `verify:true` fait relire le résultat par
  l'archétype `verifier` (champ `verified` ; sans `background:true`).
- **Dialectes du pont** : `--bridge-dialect discord|whatsapp|raw` ; `POST /send {async:true}` →
  `202 {turnId}` puis `GET /turn/<id>`.
- **Tâches planifiées** : `schedule` dans la définition d'un résident (même file que les
  demandes, `from: "schedule"`, pas de rafale après une veille) ; `--every <durée> "<prompt>"`
  et `--window HH:MM-HH:MM` font de même depuis la ligne de commande (mode agent), en prime
  sur la tâche de la définition — un intervalle et une fenêtre, pas un cron complet.
- **Compilation native des agents persistants** : `create_agent {executable:true}` génère le
  crate de l'agent sous `agents/<nom>/build/` et lance `cargo build --release` en arrière-plan
  (journal `logs/build-<nom>.log`), binaire dans `bin/<nom>` ; `swarm-control build <agent>`
  recompile au premier plan ; entrée de lib `serve_compiled`, crate généré sans secret. Sans
  `cargo` : erreur claire avec le lien rustup, l'agent est créé et reste utilisable depuis
  l'instance.
- **Mémoire et rechargement en CLI** : `swarm-control memory search|add|forget` (même fichier
  que les outils `memory_*`), et `swarm-control reload <agent>` / outil MCP `reload` du
  résident : le `.env` du workspace et `SWARM_EXTERNAL_ENV_PATH` sont relus à chaud dès que
  leur date de modification change, l'environnement système gardant la priorité.
- **Suivi de la compilation** : `create_agent {executable:true}` rend `build.runId` ; le build
  est un run `build:<agent>` visible par `agent_control status|list|kill`, sans bail sur
  l'agent ; `status` publie `rebuildNeeded` (binaire compilé avec une autre version) ;
  `swarm-control build` régénère le crate avant de compiler ; icône Windows de la classe
  (`resources/icon.ico`, 16/32/48 px) posée dans les ressources du binaire ; `doctor` rend
  une ligne « compilation d'agents » (cargo, cible de l'hôte, dossier partagé).
- **Anti-régression** : contrats publics figés dans `tests/golden/` (outils, aide, bannière,
  clés, erreurs MCP, variables d'environnement), fichiers témoins de la 0.1.2 dans
  `tests/fixtures/`, garde-fou de publication écrit (`SWARM_ALLOW_PUBLISH`, agents x / google —
  branchement du marqueur de tour à venir).

### Modifié

- ⚠ **Rupture** : un binaire `swarm-control` copié ou renommé sous un nom d'agent n'est plus
  servi ni démarré (code 2, message de migration) — un agent persistant se lance par son binaire
  **compilé** (voir « Compilation native des agents persistants »). `--agent NAME` reste
  disponible pour le développement et les tests ; un binaire compilé le **refuse** (code 2), il ne
  sert que son agent.

### Retiré

- ⚠ **Rupture** : compatibilité OverMind supprimée (variables `OVERMIND_*`, `~/.overmind`,
  `bridge/agents.json`) ; le dialecte « overmind-bridge » s'appelle dialecte Discord.

### Corrigé

- Test instable `resident_hosting::agent_stop_closes_the_server_and_frees_everything` (condition
  « le serveur ne sert plus cet agent » au lieu de « port fermé »).
- Import `std::time` confiné sous Windows (`owners/tests.rs`) ; `paste` (RUSTSEC-2024-0436,
  transitif de `ratatui`) ignoré dans `deny.toml` ; README ré-encodé ; lints clippy réglés
  (`cargo clippy -D warnings` vert).

## [0.1.2] - 2026-11-27

### Ajouté
- **Un exécutable par agent** : le binaire copié sous le nom d'un agent (`sniper`, `sniper.exe`),
  ou lancé avec `--agent NAME`, ne sert que cet agent (serveur MCP `swarm-<agent>`, file,
  session), sans serveur d'administration. `--port` fixe le port de son serveur MCP.
  `swarm_control::serve_agent("nom")` sert de `main` à un crate d'agent, et `create_agent`
  peut matérialiser l'exécutable dédié.
- **Pont HTTP compatible `overmind-bridge`** (`--bridge N`, mode agent) : `POST /send`
  (contexte `[DISCORD CONTEXT]`, réponse `{result, session_id}`, vide si l'agent a posté via
  MCP), `POST /reset`, `GET /status`, `GET /health` — un routeur Discord Node existant n'a qu'à
  changer de port. Les demandes passent par la file de l'agent.
- **Runner Antigravity (`agy`)** : création, configuration, liste et exécution d'agents via le
  CLI Antigravity en complément de Claude Code et Hermes.
- **CLI complète & TUI (`ratatui`)** : sous-commandes `serve`, `ask`, `status`, `history`,
  `reset`, `config`, `schema`, `agents`, `run`, `runs`, `doctor`, mode `--debug`, et interface
  terminal interactive (feature `tui` activée par défaut, désactivable en bibliothèque via
  `default-features = false`).
- **Agents résidents enrichis, historique & découverte de pairs** : journal rotatif des tours
  (`bridge/history/…jsonl`), outils MCP d'inspection (`history`, `logs`, `config`, `session`),
  découverte des pairs `swarm-*` et authentification serveur optionnelle.
- **Identité visuelle SVG animée** : bannière frelon cybernétique animée, console MCP
  multi-agents en direct, 6 classes d'agents (`Coordinator`, `Coder`, `Explorer`, `Verifier`,
  `Browser`, `Monitor`) et classe d'agents persistants (`Hornet Discord`, `Hornet WhatsApp`,
  `Hornet Customisable`), compatibles GitHub, crates.io et docs.rs.
- **Installation en une ligne** depuis l'exécutable publié : `install.ps1` (Windows, sans droits
  administrateur, ajout au PATH utilisateur) et `install.sh` (Linux / macOS, `~/.local/bin`).
- README réécrit pour les débutants : installation, premiers pas, glossaire, interface terminal,
  dépannage.

### Corrigé
- Message d'erreur de `serve --bridge` sans agent : espaces parasites retirés.
- Test `control` instable : l'état disque d'un run tué est attendu au lieu d'être lu aussitôt.
- Durcissement de la gestion des baux (`owners`) et de la détection de processus vivants.

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
