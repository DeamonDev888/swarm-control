# Plan — swarm-control

*Mis à jour le 2026-11-27. Liste à cocher : [TODO.md](TODO.md). Ce qui est fait et vérifié
n'apparaît plus ici, sauf dans le tableau d'état du §2.*

## 1. Vision

Un binaire Rust léger (LiteMCP, sans runtime async ni base de données), pour **Windows, macOS et
Linux**, qui fabrique et pilote deux familles d'agents IA (celles du README) :

```
swarm-control                         serveur MCP d'administration (instance swarm-control-N)
 ├─ agents PERSISTANTS — 1 profil = 1 exécutable, session gardée 24/7
 │    discord · whatsapp · x · google (Gmail & Calendar) · custom
 │    └─ chacun : MCP riche · CLI · TUI · pont/connecteur · --debug
 └─ agents de l'ESSAIM — éphémères ou nommés, lancés en parallèle (run_agents_parallel)
      coder · browser · explorer · coordinator · monitor · verifier
```

- **Persistant** = un nom, une définition sur disque (persona, modèle, serveurs MCP), une session
  reprise à chaque tour, une file de demandes, un exécutable, un canal (pont ou connecteur MCP).
- **Essaim** = un archétype (persona et outils prêts à l'emploi) lancé pour une tâche : un tour
  sans rien garder (`run_agent` sans `agentName`) ou un agent nommé à durée de vie courte.
- Hors périmètre, assumé : audit, mémoire PostgreSQL, runtime async dans la lib.

## 2. État au 2026-11-27

| Pièce | État |
|---|---|
| Serveur MCP, 10 outils, schémas JSON validés ; runners Claude, Hermes, Antigravity | ✅ |
| Agents résidents, bail inter-process, instances numérotées, routage vers l'hôte | ✅ |
| Mode agent (binaire renommé / `--agent`), pont HTTP `--bridge` (dialecte overmind-bridge) | ✅ |
| Journal des tours, outils `history` / `session` / `logs` / `config`, CLI d'agent, `--debug` | ✅ |
| TUI (`ratatui`, feature `tui`), `create_agent {executable, unit}`, pairs automatiques | ✅ |
| Jeton `SWARM_TOKEN`, binaires 5 cibles, métadonnées `cargo binstall`, `main` protégée (force-push et suppression) | ✅ |
| **Agent Discord** en production sur le serveur | ⏳ §6 |
| Agents WhatsApp, X, Google, Custom ; archétypes de l'essaim | ⏳ §4 |

## 3. Décisions

1. **Pairs = outils MCP** : chaque résident est un serveur MCP annoncé ; ses pairs sont injectés
   dans sa config MCP (`bob.ask`).
2. **Un seul binaire** : `swarm-control` sert une instance ; copié sous le nom d'un agent, il sert
   cet agent (TUI dans un terminal, `--headless` sous systemd).
3. **Journal des tours propre à swarm-control**, indépendant du runner.
4. **Un canal = un pont HTTP ou un serveur MCP, jamais du code de plateforme dans la lib** :
   Discord, WhatsApp et X exigent des clients async et des SDK lourds ; ils vivent dans des
   process à part (routeur, serveur MCP) qui parlent au pont de l'agent. La lib ne connaît que
   le **dialecte** (bloc de contexte, réponse vide si l'agent a déjà posté).
5. **Modèles d'agents** (*templates*) livrés dans le binaire : persona, serveurs MCP attendus,
   variables requises, port de pont par défaut. `create_agent {template}` les instancie.
6. **grokbot** : plus tard (commande inconnue, aucun binaire).

## 4. Feuille de route

### 4.1 Modèles d'agents (socle commun aux §4.2-4.3)
- `create_agent {template: "discord" | "whatsapp" | "x" | "google" | "custom" | "coder" | …}` :
  persona (`SOUL.md` / prompt), serveurs MCP attendus (noms seulement, jamais de secret),
  variables d'environnement requises, `executable` / `unit` / port de pont par défaut.
- `swarm-control templates` (CLI) et `list_templates` (champ de `list_agents` ou outil) pour les
  lister ; `doctor --agent X` vérifie les variables et serveurs MCP attendus par son modèle.
- Modèles écrits en JSON/Markdown dans `assets/templates/`, inclus par `include_str!`, surchargeables
  par `<workspace>/templates/`.
- **11 agents préconfigurés alignés sur les logos** : `discord`, `whatsapp`, `x`, `google`,
  `custom`, `coder`, `browser`, `explorer`, `coordinator`, `monitor`, `verifier` — mêmes slugs que
  les logos de `assets/classes/` et que MCP Control (v0.8.12, onglet Swarm, `/swarm-logo/<classe>.svg`).
  MCP Control devine aujourd'hui la classe par regex sur le nom (`sniperbot_analyst` → *explorer*) :
  swarm-control publie un champ **`class`** (annonce LiteMCP, `status`, `list_agents`, `/status` du
  pont) que MCP Control lit en priorité.

### 4.2 Agents persistants (README : Discord, WhatsApp, X, Google, Custom)

| Agent | Entrée (qui le réveille) | Sortie (comment il répond) | À faire |
|---|---|---|---|
| **discord** | pont `--bridge` `/send` (routeur Discord) | `result` du pont, ou post direct via MCP `serveur_discord` | modèle ; migration du serveur §6 ; routeur Rust à part plus tard |
| **whatsapp** | pont `/send` avec dialecte `whatsapp` (bloc `[WHATSAPP CONTEXT]` : chat, expéditeur, groupe) | `result`, ou MCP WhatsApp | dialecte paramétrable du pont ; modèle ; passerelle externe documentée |
| **x** | sondage périodique (mentions, DM) via serveur MCP X | MCP X (post, réponse) | **tâches planifiées** (§4.4) ; garde-fou de publication (brouillon ou validation) |
| **google** (Gmail & Calendar) | sondage périodique (nouveaux mails, agenda du jour) | MCP Google (brouillons, événements) | tâches planifiées ; OAuth hors binaire, jetons par l'environnement |
| **custom** | pont `/send` générique (dialecte `raw` ou `json`) ou planification | tout serveur MCP branché | dialecte `raw` ; doc « brancher Telegram / Slack / Matrix / webhook » |

- **Dialectes du pont** : `--bridge-dialect discord|whatsapp|raw` (défaut `discord`, compatible
  overmind-bridge) ; règle « réponse vide si déjà posté » propre à chaque dialecte.
- **Accusé et suivi** : `POST /send {async:true}` → `202 {turnId}` puis `GET /turn/<id>` — pour
  les passerelles qui coupent à 30-60 s (WhatsApp, webhooks).

### 4.3 Agents de l'essaim (README : coder, browser, explorer, coordinator, monitor, verifier)

| Archétype | Rôle | Outils / MCP attendus |
|---|---|---|
| **coder** | écrit et modifie du code dans un dossier donné | runner Claude/Antigravity, accès fichiers |
| **browser** | navigue, lit et remplit des pages | serveur MCP navigateur (Playwright…) |
| **explorer** | cherche et résume (code, web, docs), lecture seule | outils en lecture ; permissions restreintes |
| **coordinator** | découpe une tâche et répartit sur les autres | `run_agents_parallel`, `agent_control` de l'instance |
| **monitor** | surveille (logs, URL, métriques) et alerte | tâches planifiées (§4.4), canal d'alerte (pair discord) |
| **verifier** | relit le travail d'un autre et tranche (OK / à reprendre) | lecture seule, sortie structurée |

- `run_agent {archetype: "explorer", prompt}` : un tour éphémère avec la persona de l'archétype.
- `run_agents_parallel` accepte `archetype` par entrée ; option `verify: true` qui enchaîne un
  `verifier` sur chaque résultat.
- **Permissions par archétype** : `explorer` et `verifier` sans `--dangerously-skip-permissions`
  (outils en lecture seule côté Claude : `--allowedTools`).

### 4.4 Tâches planifiées (monitor, x, google)
- `--every 5m "<prompt>"` sur un agent résident, ou `schedule` dans sa définition ; tours mis dans
  la **même file** que les demandes (pas de tour concurrent), marqués `from: "schedule"` au journal.
- Pas de cron complet : intervalle + fenêtre horaire optionnelle ; dérive rattrapée sans rafale
  (un tour manqué n'est pas rejoué deux fois).

### 4.5 Runner générique
- Runner décrit en JSON (commande, livraison du prompt, reprise, modèle, extraction de la
  session) : opencode, kilo, gemini, qwen d'abord ; cline, openclaw, grokbot ensuite.

## 5. Corrections, optimisations et tests

### Corrections

| # | Quoi | Où | Correction |
|---|---|---|---|
| K1 | **README corrompu dans l'arbre** : accents en double encodage (séquences d'octets C3 83 xx / C3 A2 E2 82 AC xx / C3 82 xx : l'UTF-8 relu en Windows-1252 puis réécrit) | `README.md` | ré-encodé dans l'arbre le 2026-11-27 ; contrôle en place : `scripts/check-encoding.sh` (verify.sh + job release-scripts) |
| K2 | **`main` local et `origin/main` divergent** (6 commits locaux, 7 distants dont `v0.1.2`) ; le force-push est désormais interdit par la règle GitHub | git | fusionner `origin/main` dans `main` (pas de rebase forcé), résoudre README / release.yml |
| K3 | Tag `v0.1.2` hors de `main` ; permaliens README en `/v0.1.2/` alors que la release « Latest » est `v0.1.1` | git, README | après K2 : publier la 0.1.2 (ou 0.1.3) depuis `main` |
| K4 | Exemples `sniper` partout alors que l'agent du README est **discord** | README, `help.rs`, `cli/mod.rs`, `lib.rs`, docs | remplacer par `discord` (exemple de pont : `--bridge 3011`, pas 3001 déjà pris par le worker Node) |
| K5 | README périmé : badge « ~2 Mio » (mesuré 3,02 Mio avec TUI) ; « À venir : pairs injectés » alors que c'est fait | README | mettre à jour |
| K6 | Fichiers de travail non suivis à la racine : `test-lanceur*.tmp.sh`, `lancer-profil.sh`, `post-discord-*.md`, `SKILL.autoclaw-profils.md` | racine | supprimer ou ranger (`work/`, ignoré) |
| K7 | Tags `v*` non protégés : un tag de release peut être déplacé | GitHub | règle de tag (création réservée, ni déplacement ni suppression) |
| K8 | **Test instable** `resident_hosting::agent_stop_closes_the_server_and_frees_everything` : échoue dans la suite complète (l. 27, « écoute encore »), passe seul 3 fois sur 3 — un autre test reprend le port libéré dans 23080-23099 | `tests/e2e/resident_hosting.rs` | vérifier que le port ne sert plus **cet** agent (`initialize` → nom `swarm-alice`) au lieu de « port fermé », ou lier les résidents de test sur port 0 |

### Optimisations
- `pids_alive` (Windows) lance un `tasklist` complet à chaque passage même si tous les PID sont
  en cache : servir le cache d'abord, ne requêter que les PID manquants.
- Pont : un fil par connexion et `/send` qui attend jusqu'à 1 h → 64 clients lents bloquent le
  pont ; plafonner la file par agent (`429` au-delà de N demandes en attente) et proposer
  `/send {async:true}` (§4.2).
- Démarrage de la TUI : la sonde des serveurs MCP (`tui/probe.rs`) est séquentielle ; la passer en
  parallèle bornée (un fil par serveur, délai court).
- Latence Hermes ≈ 10-18 s (démarrage Python) : inhérente au CLI ; mode serveur Hermes s'il existe.
- Mesurer taille et mémoire à chaque release (script `scripts/measure.sh`, résultat dans le
  CHANGELOG) ; objectif « quelques Mio » tenu.

### Tests unitaires à ajouter (fichiers de plus de 60 lignes sans `#[cfg(test)]`)
- **Process** : `alive.rs` (`csv_pid`, `valid_unix_pid`, cache TTL), `readers.rs`, `wait.rs`.
- **Résidents** : `ctx.rs` (drapeau `reset_after_turn`), `turn.rs`, `mcp/ask.rs` (cycle `via`,
  délai), `bridge/mod.rs` (`503` au-delà de `MAX_CONNECTIONS`).
- **Runners** : `execute.rs`, `claude/mod.rs` (construction des arguments, reprise).
- **Registre et baux** : `registry/index.rs`, `registry/live_run.rs`, `owners/takeover.rs`.
- **Outils** : `create_agent.rs` (options `executable`/`unit`/`template`), `run_args.rs`,
  `run_agent.rs`, `list_agents.rs`, `agent_control/disk.rs`, `run_agents_parallel/launch.rs`.
- **Agents** : `lookup.rs`, `list/claude.rs`, `list/antigravity.rs`, `hermes/create.rs`,
  `claude/settings_edit.rs`, `antigravity/update.rs`.
- **CLI / TUI** : `cli/ask.rs` (choix hébergé / pair / local), `cli/agent.rs`, `tui/mod.rs`
  (boucle d'événements sur un faux terminal `ratatui::backend::TestBackend`).
- **Nouveaux** : dialectes du pont (§4.2), modèles (§4.1 : chaque modèle intégré se charge et
  valide son schéma), planificateur (§4.4 : pas de rafale, pas de tour concurrent).

### Robustesse et sécurité
- Signature LiteMCP (`auth-sign`) / TLS avant toute écoute hors loopback.
- Smoke réel Claude avec un vrai jeton.
- Agents X et Google : aucune publication ni envoi de mail sans garde-fou (mode brouillon par
  défaut, `SWARM_ALLOW_PUBLISH=1` pour l'autoriser).

### Distribution
- Logo docs.rs figé sur le tag de la version.
- Homebrew / Scoop si la demande existe.

## 6. Agent Discord — migration du serveur Ubuntu 192.168.40.9

État constaté le 2026-11-27 (lecture seule) : Discord → `discord-master` (Node, un token, 8
préfixes) → `POST localhost:3001/send` → `overmind-bridge.js` (Node) → `overmind-mcp` :3099 →
`run_agent(hermes, sniperbot_analyst)` ; l'agent répond lui-même via `serveur_discord`. Les
agents `trade*` ont la même forme (≈ 57 Mio par worker Node).

1. Déposer le binaire Linux à côté (sans rien arrêter), le copier sous le nom de l'agent Discord
   et le lancer `--headless --port 23090 --bridge 3011` sur le workspace OverMind.
2. Tester au `curl` (`/health`, `/send` vers un salon de test, `/reset`).
3. **Mémoire** : garder overmind-mcp :3099 pour `memory_search` / `memory_store` au début.
4. Basculer le préfixe de `discord-master` vers :3011, observer, puis arrêter
   `discord-llm.service`. **Chaque modification du serveur validée avant.**
5. Répéter pour `trade2`…`trade7`, puis remplacer `discord-master` par un routeur Rust (crate à
   part : Discord exige un runtime async).

## 7. Limites assumées

- Le CLI du modèle est relancé à chaque tour : la conversation reste dans sa session.
- Pas de base de données ni de mémoire vectorielle : c'est le rôle d'un serveur MCP tiers.
- Les agents tournent avec les droits du compte qui les lance.
