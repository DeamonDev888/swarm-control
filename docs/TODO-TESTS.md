# TODO tests — verrou anti-régression

*2026-11-27. But : qu'aucun changement (le nôtre, celui d'un agent, une montée de dépendance) ne
casse en silence ce qui marche. Point de départ : ≈ 377 tests unitaires et 65 e2e dans l'arbre, mais des
modules entiers sans test (`binaires`, `store`) ou presque (`sessions`, `routing`, `state`,
`instance`, `server`, `config`). Plan et feuille de route : [PLAN.md](PLAN.md) ·
[TODO.md](TODO.md).*

## État au 2026-11-28

Légende : `[ ]` à faire · `[ ] 🟡` **écrit dans l'arbre, vérifié vert globalement, à cocher item par
item** · `[x]` vérifié vert.

L'arbre compile de nouveau. Mesuré le 2026-11-28 : `cargo test --all-features` → **558 tests de la
bibliothèque + 81 tests e2e, 0 échec** (1 + 2 tests `#[ignore]` : vraie compilation, lente) ;
`cargo fmt --check` et `cargo clippy --all-targets --all-features -- -D warnings` propres. Les
références golden existent dans `tests/golden/`. Rien n'est commité : zwork travaille encore dans
l'arbre. Les 🟡 passent en `[x]` dès que le travail est commité et qu'on a relu chaque item contre
son test ; `tests/e2e/tools_merge.rs` et `publish_guard.rs` restent à remplir.

## Règles

- **Un contrat public = un test « golden »** : la sortie est comparée à un fichier de référence
  versionné dans `tests/golden/`. Changer le contrat oblige à mettre à jour le fichier **dans le
  même commit** : la revue voit la rupture. Mise à jour : `SWARM_BLESS=1 cargo test` réécrit les
  références.
- **Formats sur disque** : chaque format est relu depuis un **fichier témoin écrit par la version
  publiée** (`tests/fixtures/0.1.2/…`), pas seulement en aller-retour.
- Tests unitaires dans le fichier testé (`#[cfg(test)]`) ; e2e dans `tests/e2e/` avec le faux CLI
  du harnais. Aucun vrai `claude` / `hermes` / `agy`, aucun réseau hors `127.0.0.1`.
- Tout test qui attend un état doit utiliser `eventually`, jamais un `sleep` fixe.
- Un bogue corrigé = un test qui échouait avant la correction (nom de test qui décrit le bogue).

## 1. Contrats publics (golden)

- [ ] 🟡 `tools/list` de l'**instance** : noms, descriptions, JSON Schema de chaque outil
      (`tests/golden/tools-instance.json`).
- [ ] 🟡 `tools/list` d'un **résident** : `ask`, `status`, `stop`, `history`, `logs`, `config`,
      `session` (+ `memory_*`, `reload` quand ils arrivent) (`tests/golden/tools-resident.json`).
- [ ] 🟡 Clés de `status` (résident et `/status` du pont) et de `list_agents {details:true}` : MCP
      Control les lit (`class`, `agent`, `sessionId`, `queue`, `busy`…) — ensemble de clés figé.
- [ ] 🟡 Bannière de démarrage, instance et mode agent : format exact (le harnais e2e et les scripts
      la lisent : `MCP sur http://…/mcp`, `pont HTTP sur http://…`).
- [ ] 🟡 `--help` et `--version` : texte figé ; chaque commande et option listée dans l'aide existe
      dans le parseur, et inversement.
- [ ] 🟡 Codes de sortie CLI : `0` succès, `1` échec d'exécution, `2` ligne de commande invalide —
      un cas par commande.
- [ ] 🟡 Noms des variables d'environnement lues (`SWARM_*`, `CLAUDE_BIN`, `HERMES_BIN`, `AGY_BIN`,
      `HERMES_HOME`) : liste figée, et la doc README la reprend (test qui compare les deux).
      *(§1 : les 10 tests de `tests/e2e/contracts.rs` sont écrits ; références à générer.)*
- [ ] 🟡 Erreurs MCP : `-32602` avec la liste des problèmes pour des arguments invalides, `AGENT_BUSY`
      et son code, outil inconnu.

## 2. Formats sur disque (relecture de la 0.1.2)

*Fichiers témoins posés dans `tests/fixtures/0.1.2/` (state, hosts, annonces LiteMCP, profils
Claude / Hermes / Antigravity) + `0.1.2-files.txt` et `0.1.2-tools.json` ; **aucun test ne les
lit encore**. Manquent : `owners/`, `history/*.jsonl`.*

- [ ] `bridge/state/<runner>-<agent>.json` : session `{id, ts}` (TTL 30 j), `lastRun`.
- [ ] `bridge/owners/` (bail de run) et `bridge/hosts/` (bail d'hébergement) : lecture, bail
      périmé (âge testé avant le PID — C2), PID hors bornes Unix (`0`, `> i32::MAX`).
- [ ] `bridge/history/*.jsonl` : 11 champs, champs vides omis, rotation à 5 Mio, ligne corrompue
      ignorée sans faire échouer `history`.
- [ ] 🟡 `bridge/memory/*.jsonl` (mémoire native) : relecture, compaction, ligne corrompue ignorée.
- [ ] Annonce LiteMCP `%TEMP%/litemcp-registry/swarm-<agent>.json` : ce que `peers::discover`
      en lit (nom, adresse, PID, instance).
- [ ] Profils d'agents : `.claude/agents/<n>.md`, `settings_<n>.json`, `.mcp.<n>.json`, profil
      Hermes, `.antigravity/` — création puis relecture par `list_agents`.
- [ ] `.swarm-peers.json` (trace des pairs injectés) et serveurs MCP de l'utilisateur jamais
      écrasés.

## 3. Ce qui marche aujourd'hui (fonctionnel)

### Sessions et runners
- [ ] Agent nommé → session reprise au tour suivant ; session disparue → repart à neuf **une
      seule fois** ; sans `agentName` → rien d'écrit sur disque (éphémère).
- [ ] Claude : arguments construits (`--resume`, `--model`, permissions, `--mcp-config`), jetons
      de repli sur 401 / 429 / 5xx, enveloppe d'erreur lue.
- [ ] Hermes : réponse sur stdout, session sur stderr, profil `cloneFrom`, erreur de clé claire.
- [ ] Antigravity : enveloppe `agy` réelle et enveloppe d'erreur.
- [ ] Alias de modèles (`model-aliases.json`), délai `SWARM_TIMEOUT_MS`, `kill` de l'arbre de
      processus.

### Résidents, baux, instances
- [ ] Un seul hôte par agent, même entre instances ; second `agent_start` refusé avec l'URL de
      l'hôte ; l'hôte peut changer après arrêt.
- [ ] File : un tour à la fois, ordre FIFO, `stop {clearQueue}` rend une erreur à chaque demandeur.
- [ ] Routage : `run_agent` d'un résident → transmis à son hôte (`routedTo`) ; reste local avec
      `background`, `sessionId`, `path`, `provider`, `autoResume:false`.
- [ ] Cycle entre pairs (`via` contient déjà l'agent) refusé.
- [ ] Numéros d'instance `swarm-control-N` : premier libre, `--instance N` pris → refusé.
- [ ] 16 fils sur un bail mort ou frais → **un seul** gagnant (Windows inclus : test aujourd'hui
      rouge en CI Windows, à stabiliser avant de le figer).

### Pont HTTP
- [ ] Contexte Discord exact (`[DISCORD CONTEXT]`, `CHANNEL_ID`, `USER_NAME`, `USER_ID`,
      `MESSAGE`) et message brut si ni `channelId` ni `username` — golden.
- [ ] `result` vide si l'agent a déjà posté (identifiant Discord de 17 à 19 chiffres, lien
      `discord.com/channels/`), et pas vide sur un nombre quelconque.
- [ ] `/reset` pendant un tour → appliqué après le tour ; client déconnecté → tour en file annulé
      (C3) ; `411` en chunked, `413` au-delà de 1 Mio, `503` au-delà de 64 connexions, `405` / `404`.
- [ ] Jeton : écoute hors loopback refusée sans `SWARM_TOKEN` (pont et MCP) ; requête sans jeton
      → `401`.

### CLI et TUI
- [ ] `ask` choisit hébergé → pair → local, et le local écrit au journal.
- [ ] `status`, `history -n`, `reset`, `config --file`, `schema`, `agents`, `runs`, `doctor`
      (✓ / ✗ par runner).
- [ ] 🟡 TUI sur `ratatui::backend::TestBackend` : panneaux présents, saisie = un `ask`, raccourcis
      `r` `s` `d` `q` seulement quand le champ est vide, `Échap` efface, défilement.
- [ ] `--debug` : une ligne par requête MCP, **secrets masqués**.

### Sécurité (transversal)
- [ ] Aucun secret écrit : `config` masqué, journal, mémoire, `agent.json` généré, fichiers passés
      à Claude — un jeu de faux jetons (`sk-…`, `ghp_…`, Bearer, Discord) cherché dans tout ce que
      swarm-control écrit pendant la suite e2e.
- [ ] Noms d'agents validés (`[a-zA-Z0-9_-]`, minuscules pour Hermes) : aucun chemin construit à
      partir d'un nom refusé (`../`, `\`, vide, trop long).

## 4. Nos changements récents et à venir

- [ ] **Encodage** (le contrôle tourne en CI et dans `verify.sh`, mais le script n'a pas de
      test de lui-même) : `scripts/check-encoding.sh` détecte un fichier témoin doublement encodé et
      accepte un UTF-8 propre (test du script lui-même en CI).
- [ ] 🟡 **Classes et logos** : 11 classes ↔ 11 fichiers `assets/classes/hornet-pixel-<classe>.svg`
      ↔ slugs de MCP Control ; classe absente → `custom` ; `class` présent dans `status` et
      `list_agents` (+ `trading` si la classe est gardée).
- [ ] 🟡 **Modèles** : chaque modèle intégré se charge, valide son schéma, porte la classe de son nom ;
      surcharge `<workspace>/templates/` prioritaire ; variables requises manquantes → erreur.
- [ ] 🟡 **Archétypes** : `run_agent {archetype}` refuse `agentName` / `sessionId`, persona préfixée,
      `explorer` / `verifier` en lecture seule, `verify:true` remplit `verified`.
- [ ] 🟡 **Pairs automatiques** : un pair vivant entre dans la config MCP, un pair arrêté en sort,
      clé de pair = nom d'agent (`bob.ask`). *(e2e `peers.rs`, vert le 2026-11-27 avant les
      modifications en cours.)*
- [ ] 🟡 **Mémoire native** : `memory_store` / `memory_search` / `memory_forget`, recherche sans
      accents, score termes + récence, jetons filtrés, portée `swarm` en lecture seule.
      *(36 tests dans `src/memory/`, 5 dans `residents/mcp/memory.rs`, 6 dans `cli/memory.rs`.)*
- [ ] 🟡 **OverMind retiré** (0.2.0) : `OVERMIND_*` ignorées, `~/.overmind` et `bridge/agents.json`
      plus lus, aucune chaîne « overmind » dans le binaire hors exemple de doc. *(`sessions/legacy.rs`
      supprimé, plus aucune mention dans `src/` ; vérifs dans `contracts.rs` et `agent_cli.rs`.)*
- [ ] 🟡 **Outils regroupés** (0.2.0) : chaque ancien nom (alias) rend exactement la même chose que le
      nouvel outil ; `run_agent {tasks}` avec 1 et 10 tâches ; `waitAll:false`. *(`tools_merge.rs`
      créé, vide.)*
- [ ] 🟡 **Compilation native** (0.2.0) : crate généré, `agent.json` avec un jeton refusé, `--agent`
      refusé sur un binaire compilé, définition du workspace prioritaire, `.env` relu au tour
      suivant, `rebuildNeeded`, erreur claire sans `cargo` ; vraie compilation en `#[ignore]`,
      lancée en CI à chaque release. *(12 tests dans `agents/compile.rs`, 2 dans `cli/build.rs`,
      5 dans `cli/compiled.rs` ; e2e `compiled.rs` vide.)*
- [ ] 🟡 **Tâches planifiées** : pas de rafale après une veille, jamais de tour concurrent, `from:
      "schedule"` au journal. *(18 tests unitaires dans `residents/schedule.rs`.)*
- [ ] 🟡 **Dialectes du pont** : `whatsapp` et `raw` en golden, `/send {async:true}` → `202` puis
      `/turn/<id>`. *(4 tests dans `bridge/dialect.rs`, 4 dans `bridge/turns.rs`.)*

## 5. Modules sans test (unitaire, dans le fichier)

- [ ] 🟡 `binaries/` : résolution `CLAUDE_BIN` / `HERMES_BIN` / `AGY_BIN`, recherche dans le `PATH`,
      extensions Windows (`.exe`, `.cmd`). *(33 tests.)*
- [ ] 🟡 `store/` : `now_ms`, écriture JSON atomique (fichier temporaire + renommage, pas de fichier
      à moitié écrit après une erreur). *(11 tests.)*
- [ ] 🟡 `state/` (6 tests), `routing/` (11), `instance/claim.rs` (8).
- [ ] `sessions/` (get, save, delete, TTL — 1 seul test), `server/` (auth, bind, banner — 3),
      `config/` (dotenv sans override, interpolation `$VAR` / `${VAR}`, workspace, chemins,
      délais — 7).
- [ ] Fichiers listés dans [TODO.md](TODO.md) §5 « Tests unitaires à ajouter ».

## 6. Garde-fous en CI

- [ ] Suite complète **3 fois de suite** sur Windows, Linux et macOS dans un job nocturne : un test
      qui échoue une fois sur trois est instable → corrigé ou isolé, jamais ignoré en silence
      (cas connus : K8 `resident_hosting`, `owners` sous Windows).
- [ ] Couverture mesurée (`cargo llvm-cov`) et publiée par PR ; seuil qui ne peut que monter.
- [ ] Les tests golden tournent sur les 3 OS (fins de ligne, séparateurs de chemin).
- [ ] MSRV 1.85 et `--no-default-features` (lib sans TUI) testés à chaque PR.
- [ ] Avant chaque release : `scripts/verify.sh` complet + test de vraie compilation d'agent +
      relecture des fichiers témoins de la version précédente.
