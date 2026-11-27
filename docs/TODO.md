# TODO — swarm-control

*État au 2026-11-27. Mêmes numéros que [PLAN.md](PLAN.md). Ce qui était fait et vérifié a été
retiré (voir PLAN §2). Gate : 214 tests unitaires et 52/53 e2e verts dans l'arbre ; 1 e2e instable
(K8).*

## À faire en premier

- [ ] **K1** Contrôle CI qui refuse le double encodage dans les `*.md` (motifs définis par
      `scripts/check-encoding.sh` ; le README de l'arbre a été réparé le 2026-11-27).
- [ ] **K2** Fusionner `origin/main` (7 commits, dont `v0.1.2` et les visuels) dans `main`
      (6 commits locaux) — sans force-push, interdit par la règle « Protéger main ».
- [ ] **K8** Stabiliser `resident_hosting::agent_stop_closes_the_server_and_frees_everything`
      (port libéré repris par un autre test).
- [ ] **K6** Ranger les fichiers de travail de la racine (`test-lanceur*.tmp.sh`,
      `lancer-profil.sh`, `post-discord-*.md`, `SKILL.autoclaw-profils.md`).
- [ ] Committer l'arbre : pairs automatiques (`src/agents/peers.rs`, `src/peers/pairs.rs`,
      `tests/e2e/peers.rs`), `tests/e2e/agent_cli.rs`, refonte CLI/TUI (fichiers supprimés).

## 4.1 Modèles d'agents

### Agents préconfigurés, alignés sur les logos du README et de MCP Control

MCP Control (`mcp-command-center` v0.8.12, onglet Swarm) embarque les 11 logos animés officiels
(`src/ui/swarm-logos/<classe>.svg`, servis sur `/swarm-logo/<classe>.svg`) — les mêmes que
`assets/classes/hornet-pixel-<classe>.svg` ici. Il **devine** la classe d'après le nom
(`swarmLogoClassFor`, regex dans `src/ui/app.js`) : un agent mal nommé reçoit le mauvais logo
(`sniperbot_analyst` → *explorer* à cause de `analy` ; `trade2` → *custom* ; `wa` et `cal`
attrapent des noms sans rapport).

- [ ] Créer les 11 agents préconfigurés, **nommés d'après leur classe** pour que le logo tombe
      juste même sans métadonnée :
  - [ ] persistants : `discord`, `whatsapp`, `x`, `google`, `custom` — `executable:true`, pont ou
        planification selon §4.2 ;
  - [ ] essaim : `coder`, `browser`, `explorer`, `coordinator`, `monitor`, `verifier` — persona et
        permissions selon §4.3.
- [ ] Champ **`class`** dans la définition de l'agent (défaut : la classe de son modèle), exposé
      par l'annonce LiteMCP du résident, `status`, `list_agents` et `/status` du pont — un agent
      `sniperbot_analyst` de classe `discord` garde ainsi le logo Discord.
- [ ] Chaque modèle référence son logo (`assets/classes/hornet-pixel-<classe>.svg`) ; test unitaire
      qui vérifie qu'à chaque classe correspond un fichier de logo et un des 11 slugs de MCP
      Control (`logo`, `coordinator`, `coder`, `explorer`, `verifier`, `monitor`, `browser`,
      `discord`, `google`, `whatsapp`, `x`, `custom`).
- [ ] Côté MCP Control (dépôt à part) : lire `class` dans l'annonce / `status` en priorité, la
      regex sur le nom ne servant plus que de repli.
- [ ] README : chaque image de classe placée à côté de l'agent préconfiguré qui lui correspond
      (tableau classe → logo → commande `create_agent {template}`).

### Moteur de modèles

- [ ] `create_agent {template}` : persona, serveurs MCP attendus, variables requises, défauts
      `executable` / `unit` / port de pont.
- [ ] Modèles intégrés (`assets/templates/`, `include_str!`) surchargeables par
      `<workspace>/templates/`.
- [ ] `swarm-control templates` et liste des modèles côté MCP ; `doctor --agent X` vérifie les
      prérequis du modèle.

## 4.2 Agents persistants

- [ ] **discord** : modèle (pont `discord`, MCP `serveur_discord`, port 3011) — migration §6.
- [ ] **whatsapp** : dialecte `whatsapp` du pont (`[WHATSAPP CONTEXT]`), modèle, doc de la
      passerelle externe.
- [ ] **x** : modèle sur serveur MCP X, sondage planifié (§4.4), publication en brouillon par défaut.
- [ ] **google** (Gmail & Calendar) : modèle sur serveur MCP Google, sondage planifié, OAuth hors
      binaire.
- [ ] **custom** : dialecte `raw`/`json` du pont ; doc Telegram / Slack / Matrix / webhook.
- [ ] `--bridge-dialect discord|whatsapp|raw` (défaut `discord`, compatible overmind-bridge).
- [ ] `POST /send {async:true}` → `202 {turnId}` + `GET /turn/<id>`.

## 4.3 Agents de l'essaim

- [ ] Modèles **coder**, **browser**, **explorer**, **coordinator**, **monitor**, **verifier**.
- [ ] `run_agent {archetype}` (tour éphémère avec la persona) ; `archetype` par entrée de
      `run_agents_parallel`.
- [ ] Option `verify: true` : un `verifier` relit chaque résultat.
- [ ] Permissions restreintes pour `explorer` et `verifier` (`--allowedTools`, pas de
      `--dangerously-skip-permissions`).

## 4.4 Tâches planifiées

- [ ] `--every <durée> "<prompt>"` / `schedule` dans la définition ; tours dans la même file,
      `from: "schedule"` au journal ; pas de rafale après une veille.

## 4.5 Runner générique

- [ ] Runner décrit en JSON : opencode, kilo, gemini, qwen ; puis cline, openclaw, grokbot.

## 5. Corrections

- [ ] **K3** Publier la 0.1.2/0.1.3 depuis `main` après K2 (permaliens README `/v0.1.2/`, « Latest »
      encore `v0.1.1`).
- [ ] **K4** Remplacer les exemples `sniper` par `discord` : README, `src/cli/help.rs`,
      `src/cli/mod.rs`, `src/lib.rs`, `docs/RESIDENT-AGENTS.md` ; exemple de pont en 3011.
- [ ] **K5** README : badge binaire (3,02 Mio mesurés avec TUI), retirer « pairs injectés » de
      « À venir », ajouter modèles et tâches planifiées.
- [ ] **K7** Règle GitHub sur les tags `v*` (ni déplacement ni suppression).

## 5. Optimisations

- [ ] `pids_alive` (Windows) : servir le cache d'abord, `tasklist` seulement pour les PID manquants
      (`src/process/alive.rs`).
- [ ] Pont : plafond de file par agent (`429`) en plus des 64 connexions.
- [ ] Sonde MCP de la TUI en parallèle bornée (`src/tui/probe.rs`).
- [ ] `scripts/measure.sh` : taille du binaire et mémoire au repos, consignées au CHANGELOG à
      chaque release.

## 5. Tests unitaires à ajouter

- [ ] `process/alive.rs` (`csv_pid`, `valid_unix_pid`, expiration du cache), `process/readers.rs`,
      `process/wait.rs`.
- [ ] `residents/ctx.rs` (reset différé), `residents/turn.rs`, `residents/mcp/ask.rs` (cycle `via`,
      délai), `residents/bridge/mod.rs` (`503` au-delà de 64).
- [ ] `runners/execute.rs`, `runners/claude/mod.rs` (arguments, reprise).
- [ ] `registry/index.rs`, `registry/live_run.rs`, `owners/takeover.rs`.
- [ ] `tools/create_agent.rs`, `tools/run_args.rs`, `tools/run_agent.rs`, `tools/list_agents.rs`,
      `tools/agent_control/disk.rs`, `tools/run_agents_parallel/launch.rs`.
- [ ] `agents/lookup.rs`, `agents/list/claude.rs`, `agents/list/antigravity.rs`,
      `agents/hermes/create.rs`, `agents/claude/settings_edit.rs`, `agents/antigravity/update.rs`.
- [ ] `cli/ask.rs` (hébergé / pair / local), `cli/agent.rs`, `tui/mod.rs` (`TestBackend`).
- [ ] Nouveaux modules : dialectes du pont, chargement de chaque modèle, planificateur.

## 5. Robustesse, sécurité, distribution

- [ ] Signature LiteMCP (`auth-sign`) / TLS avant toute écoute hors loopback.
- [ ] Smoke réel Claude avec un vrai jeton (bloqué : jeton requis).
- [ ] Garde-fou de publication pour x et google (`SWARM_ALLOW_PUBLISH=1`).
- [ ] Logo docs.rs figé sur le tag de la version (réglage docs.rs).
- [ ] Homebrew / Scoop si la demande existe.

## 6. Agent Discord — serveur 192.168.40.9

*(aucune modification du serveur sans validation préalable)*

- [ ] Binaire Linux x86_64 déposé à côté, copié sous le nom de l'agent Discord, lancé
      `--headless --port 23090 --bridge 3011` sur le workspace OverMind.
- [ ] Tests au `curl` : `/health`, `/send` vers un salon de test, `/reset`.
- [ ] Garder overmind-mcp :3099 pour la mémoire au début.
- [ ] Basculer le préfixe de `discord-master` vers :3011, observer, arrêter `discord-llm.service`.
- [ ] Puis `trade2`…`trade7` ; plus tard, routeur Discord en Rust (crate à part).
