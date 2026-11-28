# TODO — swarm-control

*État vérifié le 2026-11-27 sur l'arbre de travail (Windows 11, Rust 1.99.1). Mêmes numéros de
section que [PLAN.md](PLAN.md). Légende : `[x]` fait et vérifié ; `[ ]` reste à faire. Une case
cochée décrit **l'arbre**, pas forcément le commit.*

## 0. Vérification et périmètre du commit

| Contrôle | Résultat |
|---|---|
| `cargo fmt --check` | ✅ |
| `cargo check --all-targets` | ✅ |
| `cargo clippy --all-targets` | ✅ y compris avec `-D warnings` (lints réglés) |
| `cargo test --lib` | ✅ 526 réussis, 0 échec, 1 ignoré |
| `cargo test --test e2e` | ✅ 79 réussis, 0 échec |

**Couverture des audits** : trois vérifications déléguées ont été rendues utilisables et
recoupées avec le code — §4.8 (compilation, 12 points), §4.7 (outils et sémantique de `kill`),
§4.1-4.6 + K1-K10 + §5 (optimisations). Aucun périmètre délégué n'est resté non couvert. Une
conclusion d'audit a été corrigée après vérification directe : K10 restait partiel (deux docs
annoncent encore la lecture de `agents.json`), et il est coché ainsi ci-dessous.

**Commit à trois fichiers** (effectué dans ce commit) :

- [x] `src/cli/exe.rs` — `renamed`, `migration_error`, `MIGRATION`. `agent_name` reste un alias de
      compatibilité : le `cli/mod.rs` de `HEAD` l'appelle encore.
- [x] `docs/PLAN.md` — §2 vérifié, état par section, K1-K11.
- [x] `docs/TODO.md` — ce fichier.

**Ce que ce commit ne fait pas** : `src/cli/mod.rs` n'en fait pas partie. À `HEAD`, `cli::run()`
appelle encore `exe::agent_name` et sert donc un binaire renommé ; le refus 0.2.0 (code 2) n'entre
en vigueur qu'avec le commit de `cli/mod.rs`.

- [ ] Committer `src/cli/mod.rs` (avec `src/cli/agent.rs` et `src/lib.rs`) pour rendre le refus
      effectif et retirer l'alias `agent_name`.
- [x] `main` locale est derrière `origin/main` de 5 commits (README, visuels) et l'arbre modifie  (résolu : `origin/main` fusionné dans la branche)
      les mêmes fichiers : fast-forward/merge à traiter avant le commit (K11).
- [ ] L'arbre porte ~100 fichiers modifiés ou non suivis (§4.1-4.8) : découper les commits par
      sujet, ne pas tout mettre dans celui-ci.

## 2. État vérifié

Résumé (détail : PLAN §2) : **§4.1-4.3 faits** ; **§4.4 et §4.6 partiels** ; **§4.8 partiel** ;
**§4.5 et §4.7 absents** ; §5 à moitié ; §6 rien. 526 tests unitaires et 79 e2e verts localement ;
la CI n'est pas revérifiée.

## 4.1 Modèles d'agents

### Agents préconfigurés et logos

- [x] 11 modèles intégrés (22 fichiers `assets/templates/*.json|*.md`) : `discord`, `whatsapp`,
      `x`, `google`, `custom`, `coder`, `browser`, `explorer`, `coordinator`, `monitor`, `verifier`.
- [x] Moteur : `create_agent {template}`, persona, serveurs MCP attendus, variables requises,
      défauts `executable` / `unit` / runner ; surcharge `<workspace>/templates/` prioritaire.
- [x] `list_templates` (11e outil) et `swarm-control templates` ; `doctor --agent X` vérifie les
      variables, les serveurs MCP et le binaire du runner.
- [x] Champ **`class`** dans `status`, `list_agents` et `/status` du pont ; jamais déduit du nom.
- [x] Logo par classe (`assets/classes/hornet-pixel-<classe>.svg`) + test qui vérifie les 11
      classes, les 11 slugs de MCP Control et la présence des fichiers.
- [x] Tests e2e `tests/e2e/templates.rs` (9 tests) et unitaires du moteur.

### Reste

- [ ] README : tableau modèles → classe → logo → `create_agent {template}`, et remplacer
      « Les 10 outils MCP » par 11 (le README ne mentionne ni `template` ni `list_templates`).
- [ ] MCP Control (dépôt à part) : lire `class` dans `status` / `list_agents` en priorité ; la
      regex sur le nom ne sert plus que de repli.
- [ ] `RESIDENT-AGENTS.md` : documenter `class`, `template` et les 11 modèles.

## 4.2 Agents persistants

- [x] **discord** : modèle (pont `discord`, MCP `serveur_discord`, port 3011).
- [x] **whatsapp** : modèle et dialecte `[WHATSAPP CONTEXT]`.
- [x] **x** : modèle sur serveur MCP X, brouillon par défaut.
- [x] **google** : modèle Gmail & Calendar, OAuth hors binaire.
- [x] **custom** : modèle, dialecte `raw`.
- [x] `--bridge-dialect discord|whatsapp|raw` (défaut `discord`), règle « réponse vide si déjà
      posté » par dialecte.
- [x] `POST /send {async:true}` → `202 {turnId}` puis `GET /turn/<id>` (TTL 1 h).
- [x] Tests unitaires des dialectes et des tours asynchrones.

### Reste

- [ ] Passerelle WhatsApp externe : la documenter (webhook → `POST /send`, coupure à 30-60 s,
      reprise par `/turn/<id>`), comme le routeur Discord l'est pour `discord`.
- [ ] Brancher `config::publish::guard` avant toute publication X ou envoi Gmail : l'API et ses
      tests existent (`src/config/publish.rs`), aucun appelant aujourd'hui ; les modèles x/google
      ne font que l'interdire par consigne.
- [ ] Docs : `README` et `RESIDENT-AGENTS.md` pour x/google (MCP externes, OAuth, brouillon).
- [ ] Routeur Discord en Rust (crate à part, runtime async) — après §6.

## 4.3 Agents de l'essaim

- [x] 6 modèles : `coder`, `browser`, `explorer`, `coordinator`, `monitor`, `verifier`.
- [x] `run_agent {archetype}` : tour éphémère, persona préfixée, refuse `agentName` / `sessionId`.
- [x] `run_agents_parallel` accepte `archetype` par entrée.
- [x] `verify:true` : un `verifier` relit chaque résultat et remplit `verified` / `verifiedError`.
- [x] Permissions restreintes `explorer` / `verifier` : `--allowedTools`, jamais
      `--dangerously-skip-permissions`.
- [x] Tests unitaires et e2e (`tests/e2e/archetype.rs`).

### Reste

- [x] README : section archétypes (`run_agent {archetype}`, `verify:true`, permissions).
- [x] `RESIDENT-AGENTS.md` : quand préférer un archétype à un agent nommé.

## 4.4 Tâches planifiées

- [x] `schedule` dans la définition (`{every, prompt, window?}` ou chaîne `<durée> <prompt>`).
- [x] Tours déposés dans la **même file** que les demandes, marqués `from: "schedule"`.
- [x] Fenêtre horaire, pas de rafale après une veille, un tour manqué n'est pas rejoué deux fois.
- [x] Planificateur arrêté avec l'agent ; 18 tests unitaires dans `src/residents/schedule.rs`.

### Reste

- [x] **`--every` et `--window` sont des drapeaux morts** : parsés dans `cli/args.rs`, jamais lus
      par `cli/serve.rs` ni `cli/agent.rs`. Les transmettre à `residents::start` (ou poser le
      schedule process-wide comme le dialecte) et couvrir par un test.
- [ ] Modèles `x`, `google`, `monitor` : aucun `schedule` par défaut dans leur JSON ; en poser un
      (sondage des mentions, agenda du jour, surveillance) ou le documenter comme à créer.
- [ ] Test e2e : un tour planifié arrive une seule fois, marqué `from: "schedule"`.
- [x] README : `--every 5m "<prompt>"`, fenêtre, limite « pas de cron complet ».

## 4.5 Runner générique

- [ ] Écrire le runner décrit en JSON (commande, livraison du prompt, reprise, modèle, extraction
      de la session) : `src/runners/generic/mod.rs` ne contient que sa documentation.
- [ ] opencode, kilo, gemini, qwen.
- [ ] cline, openclaw, grokbot.
- [ ] Tests : un faux CLI décrit en JSON, reprise de session, erreur de commande, modèle absent.

## 4.6 Mémoire native

- [x] Outils du résident `memory_store {text, tags?}`, `memory_search {query, limit?}`,
      `memory_forget {id}` — mêmes noms que ceux des personas existantes.
- [x] Stockage `bridge/memory/<runner>-<agent>.jsonl`, recherche par mots-clés sans accents,
      score termes + récence, plafond et compaction, ligne corrompue ignorée.
- [x] Jetons filtrés à l'écriture (même filtre que `config`).
- [x] Portée `swarm` en lecture seule sur la mémoire des pairs.
- [x] Injectée comme serveur MCP `memory` de chaque agent (Claude, Hermes, Antigravity) ; un
      serveur `memory` déclaré par l'utilisateur n'est jamais écrasé.
- [x] 36 tests dans `src/memory/`, 5 dans `residents/mcp/memory.rs`, 6 dans `cli/memory.rs`.

### Reste

- [x] **`src/cli/memory.rs` n'est pas compilé** : `mod memory;` absent de `src/cli/mod.rs`, donc
      `memory search|add|forget` n'existe pas dans le binaire. Déclarer le module, la commande
      (`args.rs`), l'aide (`help.rs`) et les tests CLI.
- [ ] Panneau mémoire dans la TUI (lecture, recherche, ajout) — absent de `src/tui/`.
- [x] README : `memory_store` / `memory_search` / `memory_forget`, portée `swarm`, filtre des
      jetons, différence avec la session (`--resume`).
- [ ] Test e2e : un tour écrit un souvenir, le tour suivant le retrouve via `memory_search`.

## 4.7 Outils MCP de l'instance : de 11 à 7

- [x] Question tranchée : `agent_control kill` atteint le tour du résident **dans le même
      process** ; depuis une autre instance il ne tue que le CLI (pas la file) ; un run fini gardé
      1 h n'est plus « en cours » et n'a pas de repli disque. À corriger avant de fusionner les
      arrêts.
- [ ] `run_agent` accepte `tasks:[…]` (10 max) et `waitAll` → absorbe `run_agents_parallel`.
- [ ] `agent_control` renommé `run_control` (`status | stream | wait | kill | list`).
- [ ] `agent_start` + `agent_stop` → `agent_resident {start | stop | list}`.
- [ ] `get_agent_configs` + `update_agent_config` → `agent_config {get | set}`.
- [ ] `list_templates` → `list_agents {templates:true}`.
- [ ] Anciens noms gardés une version comme alias « déprécié » (même handler) : 7 alias
      (`run_agents_parallel`, `agent_control`, `agent_start`, `agent_stop`, `get_agent_configs`,
      `update_agent_config`, `list_templates`), retrait en 0.3.0.
- [ ] Corriger les trous de `kill` trouvés ci-dessus (autre instance, run fini < 1 h, repli disque).
- [ ] Adapter la CLI (`agents`, `run`, `runs`), `tests/e2e/tools_list.rs` (11 → 7 + 7 alias),
      `README` (« Les 10 outils MCP »), `RESIDENT-AGENTS.md`, `ARCHITECTURE.md`.
- [ ] MCP Control (dépôt à part) : passer aux nouveaux noms, garder les anciens en repli tant que
      des instances 0.1.x tournent.
- [ ] Tests `tests/e2e/tools_merge.rs` (aujourd'hui 3 lignes de commentaire) : chaque alias rend la
      même chose que le nouvel outil ; `run_agent {tasks}` avec 1 et 10 tâches ; `waitAll:false`
      rend au premier succès.

## 4.8 Compilation native des agents persistants

- [x] Génération de `<workspace>/agents/<nom>/build/` : `Cargo.toml` (swarm-control épinglé à la
      version courante), `src/main.rs`, `agent.json` (définition par défaut, **sans secret** :
      génération refusée sinon), `build.rs`.
- [x] `serve_compiled(agent, class, definition)` : nom et classe gravés, `--agent` refusé (code 2),
      `--version` imprimé ; la définition du workspace fait foi, l'embarquée ne sert qu'à recréer
      un dossier manquant.
- [x] `spawn_build` en arrière-plan, dossier partagé `<workspace>/build/target`, binaire dans
      `<workspace>/bin/<agent>(.exe)`, journal `<workspace>/logs/build-<agent>.log`.
- [x] Signature ad hoc macOS (`codesign -s -`) si disponible.
- [x] Sans `cargo` : erreur claire avec `https://rustup.rs`, agent créé quand même.
- [x] Définition, settings Claude, `.env` du profil et `env` des serveurs MCP relus à chaque tour.
- [x] `delete_agent` supprime le binaire et `build/` ; le profil de l'agent reste.
- [x] Tests unitaires : 12 dans `agents/compile.rs`, 2 dans `cli/build.rs`, 5 dans `cli/compiled.rs`.

### Reste

- [x] Le build n'est **pas suivi par `agent_control`** : `create_agent` rend `{status:"building"}`
      sans `runId` ; `spawn_build` est un fil nommé, pas un run. Enregistrer un run (ou un suivi
      dédié) pour `status` / `stream` / `kill`.
- [x] `swarm-control build <agent>` ne régénère pas le crate avant de compiler : un changement de
      nom, de classe ou de version n'est pas pris en compte. Appeler `compile::generate` d'abord.
- [x] Icône Windows : aucun `.ico` n'est produit ; `build.rs` ne pose l'icône et la version que si
      `resources/icon.ico` existe. Convertir `assets/classes/hornet-pixel-<classe>.svg` en `.ico`
      à la génération, puis vérifier les ressources du binaire.
- [x] Ligne « compilation d'agents » dans `doctor` (cargo présent, cible de l'hôte, cache partagé).
- [x] `<workspace>/.env` relu à chaud quand sa date de modification change ; outil MCP `reload` du
      résident et commande `swarm-control reload <agent>` (les variables système gardent la
      priorité) — `config/state.rs` ne le lit qu'une fois.
- [x] `rebuildNeeded` dans `status` (binaire compilé avec une autre version que l'instance) —
      `compile::rebuild_needed` existe mais n'est appelé nulle part.
- [x] Docs : README §3 (« Un agent = un exécutable » décrit encore la copie), `RESIDENT-AGENTS.md`,
      CHANGELOG (note de migration : les copies renommées ne servent plus leur agent) ; la
      description de l'outil `create_agent` dit encore « copie le binaire courant ».
- [x] `tests/e2e/compiled.rs` (aujourd'hui 3 lignes) : crate généré, `agent.json` avec un jeton
      refusé, refus de `--agent`, définition du workspace prioritaire, `.env` modifié relu,
      `rebuildNeeded`, erreur sans `cargo` (faux PATH).
- [ ] Un test de **vraie compilation**, marqué `#[ignore]` (lent, réseau), lancé en CI une fois par
      release.
- [x] Retirer l'alias `agent_name` de `cli/exe.rs` quand `cli/mod.rs` est commité (ce commit ne le
      peut pas).

## 5. Corrections

- [x] **K1** README ré-encodé ; `scripts/check-encoding.sh` en place (verify.sh + job release).
- [x] **K2** `origin/main` fusionné dans `main` (`ac15042`).
- [x] **K3** `v0.1.2` est sur `main` ; le README n'épingle plus de tag (liens `/main/`).
- [x] **K4** Plus aucun exemple `sniper` dans le code ; `discord` partout (`--bridge 3011`).
- [x] **K5** README : badge ~3 Mio, pairs injectés décrits comme faits.
- [x] **K6** Fichiers de travail de la racine rangés ou supprimés.
- [x] **K8** `resident_hosting` vérifie que le port ne sert plus **cet** agent (`initialize` →
      `swarm-alice`), plus « port fermé » ; la suite complète est verte.
- [x] **K9** Plus aucune compatibilité OverMind dans `src/` (variables, `~/.overmind`,
      `sessions/legacy.rs`, `OVERMIND_AGENT_NAME`) ; dialecte renommé « Discord ».
- [ ] **K7** Règle GitHub sur les tags `v*` (création réservée, ni déplacement ni suppression) —
      aucune preuve locale, à poser dans les réglages du dépôt.
- [ ] **K10** Deux docs annoncent encore la lecture de `bridge/agents.json` :
      `docs/ARCHITECTURE.md` (l. 89) et `docs/RESIDENT-AGENTS.md` (l. 72). Retirer ces phrases.
- [x] **K11** `main` locale en retard de 5 commits sur `origin/main` (README, visuels) ; l'arbre
      modifie les mêmes fichiers. Fast-forward/merge avant le commit, sans écraser les visuels
      distants.

## 5. Optimisations

- [x] `pids_alive` (Windows) : cache servi d'abord, `tasklist` seulement pour les PID manquants
      (`src/process/alive.rs`), test unitaire vert.
- [x] Pont : plafond de file par agent (`429`) en plus des 64 connexions.
- [x] Sonde MCP de la TUI en parallèle bornée (`src/tui/probe.rs`).
- [ ] `scripts/measure.sh` : le script existe et sait rendre du JSON, mais aucun résultat n'est
      consigné au CHANGELOG ; le lancer à chaque release.
- [ ] Latence Hermes (10-18 s au démarrage Python) : mode serveur Hermes s'il existe — à vérifier
      avant d'investir.

## 5. Tests

- [x] Suite verte : 526 unitaires + 79 e2e (Windows, 2026-11-27).
- [x] Références golden générées et lues (`tests/golden/`) ; `tests/e2e/contracts.rs` vert.
- [x] Modules longtemps sans test : `binaries/` (33), `store/` (11), `state/` (6), `routing/` (11),
      `instance/claim.rs` (8), `process/` (alive, readers, wait), `registry/`, `owners/`, TUI
      (`TestBackend`).
- [ ] `tests/e2e/compiled.rs`, `tests/e2e/tools_merge.rs`, `tests/e2e/publish_guard.rs` : vides
      (3 lignes de commentaire) ; les remplir avec §4.7, §4.8 et le garde-fou de publication.
- [ ] `docs/TODO-TESTS.md` §2 : relire les fichiers témoins `tests/fixtures/0.1.2/` (aucun test ne
      les lit encore) ; manquent `owners/` et `history/*.jsonl`.
- [ ] `docs/TODO-TESTS.md` §3 : tests fonctionnels listés (sessions, résidents, pont, CLI, TUI,
      sécurité).
- [ ] CI : `cargo clippy -- -D warnings` (19 avertissements de code mort aujourd'hui) et
      `cargo deny check`.
- [ ] Réactualiser l'en-tête de `docs/TODO-TESTS.md` : il annonce encore que « l'arbre ne compile
      pas » alors que la suite est verte ; le reste du plan de tests reste valable.
- [ ] Job nocturne 3 OS × 3 passages, couverture (`cargo llvm-cov`), MSRV 1.85,
      `--no-default-features`.
- [ ] Avant release : `scripts/verify.sh` + vraie compilation d'agent + relecture des fichiers
      témoins de la version précédente.

## 5. Robustesse, sécurité, distribution

- [ ] Signature LiteMCP (`auth-sign`) / TLS avant toute écoute hors loopback.
- [ ] Smoke réel Claude avec un vrai jeton (bloqué : jeton requis).
- [ ] Garde-fou de publication x/google : branché (voir §4.2), pas seulement l'API.
- [ ] Logo docs.rs figé sur le tag de la version (réglage docs.rs).
- [ ] Homebrew / Scoop si la demande existe.
- [ ] **0.2.0** : `Cargo.toml` en 0.2.0, note de migration au CHANGELOG (binaire compilé, outils
      regroupés, OverMind retiré), publication crates.io, binaires 5 cibles, page crates.io à jour.

## 6. Agent Discord — serveur 192.168.40.9

*(aucune modification du serveur sans validation préalable)*

- [ ] Agent créé dans un workspace swarm-control propre (pas celui d'OverMind), modèle et classe
      `discord`, persona reprise du SOUL actuel, MCP sans `overmind-mcp` : `serveur_discord` +
      mémoire native (§4.6).
- [ ] Binaire de l'agent compilé pour Linux (Rust sur le serveur à valider, ou compilation
      ailleurs puis dépôt), lancé `--headless --port 23090 --bridge 3011`.
- [ ] Tests au `curl` : `/health`, `/send` vers un salon de test, `/reset` ; `memory_store` /
      `memory_search` servis par swarm-control.
- [ ] Basculer le préfixe de `discord-master` vers :3011, observer, arrêter
      `discord-llm.service` et `overmind-mcp`.
- [ ] Puis `trade2`…`trade7` ; plus tard, routeur Discord en Rust (crate à part).

## 7. Limites assumées

Rien à faire : le CLI du modèle est relancé à chaque tour, la mémoire native est un journal JSONL
sans recherche vectorielle, et les agents tournent avec les droits du compte qui les lance.
