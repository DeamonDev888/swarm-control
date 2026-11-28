# Plan — swarm-control

*Mis à jour le 2026-11-27. Liste à cocher : [TODO.md](TODO.md). Le §2 est l'état **vérifié** de
l'arbre de travail ; le §4 garde la conception de chaque pièce et signale ce qui reste.*

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
| Serveur MCP, **11 outils** (10 + `list_templates`), schémas JSON validés ; runners Claude, Hermes, Antigravity | ✅ |
| Agents résidents, bail inter-process, instances numérotées, routage vers l'hôte, pairs automatiques | ✅ |
| Pont HTTP `--bridge`, dialectes `discord` / `whatsapp` / `raw`, `/send {async:true}` + `/turn/<id>` | ✅ |
| Journal des tours, outils `history` / `session` / `logs` / `config`, CLI d'agent, `--debug`, TUI | ✅ |
| 11 modèles d'agents (`assets/templates/`), `create_agent {template}`, `list_templates`, classe + logos | ✅ |
| Archétypes de l'essaim (`run_agent {archetype}`, `verify:true`, permissions lecture seule) | ✅ |
| Jeton `SWARM_TOKEN`, binaires 5 cibles, métadonnées `cargo binstall`, `main` protégée (force-push et suppression) | ✅ |
| Mémoire native (`memory_*`, JSONL, portée `swarm`) | 🟡 MCP et stockage faits ; CLI et panneau TUI à brancher |
| Tâches planifiées (`schedule` de la définition, même file, `from:"schedule"`) | 🟡 `--every` / `--window` parsés mais sans effet |
| Compilation native (`serve_compiled`, crate généré, `build/`) | 🟡 reload, `rebuildNeeded`, icône Windows et e2e manquent |
| Outils MCP regroupés de 11 à 7 (§4.7) | ⏳ |
| Runner générique JSON (§4.5) | ⏳ |
| **Agent Discord** en production sur le serveur | ⏳ §6 |
| Publication 0.2.0 (version, notes de migration, crates.io, binaires) | ⏳ |

Tests locaux du 2026-11-27 (Windows 11, Rust 1.99.1) : `cargo fmt --check`, `cargo check
--all-targets`, `cargo clippy --all-targets`, `cargo test --lib` (**526 réussis, 1 ignoré**) et
`cargo test --test e2e` (**79 réussis**) verts. La CI n'est pas revérifiée.

## 3. Décisions

1. **Pairs = outils MCP** : chaque résident est un serveur MCP annoncé ; ses pairs sont injectés
   dans sa config MCP (`bob.ask`).
2. **Un agent persistant = un binaire compilé** (décision du 2026-11-27, remplace la copie
   renommée) : à la création, swarm-control génère un petit crate qui dépend de lui-même et le
   compile ; l'identité (nom, classe) est **gravée dans le binaire**, pas déduite du nom de
   fichier. La définition (persona, modèle, serveurs MCP) et **tous les identifiants** restent
   dans le workspace, relus à chaque tour : aucun secret dans un binaire, aucune recompilation
   pour les modifier. Pas de repli par copie : sans Rust, la création de l'exécutable
   échoue avec un message clair. `swarm-control` reste le binaire d'instance ; `--agent NAME`
   reste pour le développement et les tests (TUI dans un terminal, `--headless` sous systemd).
3. **Journal des tours propre à swarm-control**, indépendant du runner.
4. **Un canal = un pont HTTP ou un serveur MCP, jamais du code de plateforme dans la lib** :
   Discord, WhatsApp et X exigent des clients async et des SDK lourds ; ils vivent dans des
   process à part (routeur, serveur MCP) qui parlent au pont de l'agent. La lib ne connaît que
   le **dialecte** (bloc de contexte, réponse vide si l'agent a déjà posté).
5. **Modèles d'agents** (*templates*) livrés dans le binaire : persona, serveurs MCP attendus,
   variables requises, port de pont par défaut. `create_agent {template}` les instancie.
6. **grokbot** : plus tard (commande inconnue, aucun binaire).
7. **OverMind n'est plus supporté** (ni l'OverMind TS, ni `overmind-mcp`, ni `overmind-bridge`) :
   aucune dépendance, aucune compatibilité de fichiers ou de variables, pas même pour la mémoire.
   Le nom n'apparaît plus que comme **exemple** d'un routeur existant qui parle le dialecte
   Discord du pont. La mémoire des agents est fournie par swarm-control lui-même (§4.6).

## 4. Feuille de route

### 4.1 Modèles d'agents (socle commun aux §4.2-4.3)

**État** : fait dans l'arbre — 11 modèles (22 fichiers `assets/templates/`), moteur, `class` et
logos. Reste la doc README et la lecture de `class` par MCP Control (dépôt à part).

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
  swarm-control publie un champ **`class`** (`status`, `list_agents`, `/status` du pont — l'annonce
  LiteMCP a un format figé) que MCP Control lit en priorité.

### 4.2 Agents persistants (README : Discord, WhatsApp, X, Google, Custom)

**État** : pont et 5 modèles faits (dialectes, `async`, `/turn`). Reste la passerelle WhatsApp
documentée, le garde-fou de publication câblé dans les agents x/google, et la migration §6.

| Agent | Entrée (qui le réveille) | Sortie (comment il répond) | À faire |
|---|---|---|---|
| **discord** | pont `--bridge` `/send` (routeur Discord) | `result` du pont, ou post direct via MCP `serveur_discord` | modèle ; migration du serveur §6 ; routeur Rust à part plus tard |
| **whatsapp** | pont `/send` avec dialecte `whatsapp` (bloc `[WHATSAPP CONTEXT]` : chat, expéditeur, groupe) | `result`, ou MCP WhatsApp | dialecte paramétrable du pont ; modèle ; passerelle externe documentée |
| **x** | sondage périodique (mentions, DM) via serveur MCP X | MCP X (post, réponse) | **tâches planifiées** (§4.4) ; garde-fou de publication (brouillon ou validation) |
| **google** (Gmail & Calendar) | sondage périodique (nouveaux mails, agenda du jour) | MCP Google (brouillons, événements) | tâches planifiées ; OAuth hors binaire, jetons par l'environnement |
| **custom** | pont `/send` générique (dialecte `raw` ou `json`) ou planification | tout serveur MCP branché | dialecte `raw` ; doc « brancher Telegram / Slack / Matrix / webhook » |

- **Dialectes du pont** : `--bridge-dialect discord|whatsapp|raw` (défaut `discord` — le dialecte
  qu'un routeur existant comme l'ancien overmind-bridge parle déjà) ; règle « réponse vide si déjà posté » propre à chaque dialecte.
- **Accusé et suivi** : `POST /send {async:true}` → `202 {turnId}` puis `GET /turn/<id>` — pour
  les passerelles qui coupent à 30-60 s (WhatsApp, webhooks).

### 4.3 Agents de l'essaim (README : coder, browser, explorer, coordinator, monitor, verifier)

**État** : fait — 6 modèles, `archetype` par tour et par entrée parallèle, `verify:true`,
permissions lecture seule. Reste la doc README des archétypes.

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

**État** : partiel — le `schedule` de la définition fonctionne (même file, `from:"schedule"`,
pas de rafale) ; `--every` / `--window` sont parsés mais jamais transmis au résident.

- `--every 5m "<prompt>"` sur un agent résident, ou `schedule` dans sa définition ; tours mis dans
  la **même file** que les demandes (pas de tour concurrent), marqués `from: "schedule"` au journal.
- Pas de cron complet : intervalle + fenêtre horaire optionnelle ; dérive rattrapée sans rafale
  (un tour manqué n'est pas rejoué deux fois).

### 4.5 Runner générique

**État** : absent — `src/runners/generic/mod.rs` ne contient que sa documentation.

- Runner décrit en JSON (commande, livraison du prompt, reprise, modèle, extraction de la
  session) : opencode, kilo, gemini, qwen d'abord ; cline, openclaw, grokbot ensuite.

### 4.6 Mémoire native des agents *(remplace `overmind-mcp`)*

**État** : partiel — outils MCP, stockage JSONL, score, compaction, filtrage des jetons, portée
`swarm` et injection MCP faits et testés ; `src/cli/memory.rs` n'est pas déclaré dans `cli/mod.rs`
(donc absent du binaire) et la TUI n'a pas de panneau mémoire.

- Outils MCP du résident **`memory_store {text, tags?}`**, **`memory_search {query, limit?}`**,
  **`memory_forget {id}`** — mêmes noms que ceux qu'appellent les personas existantes, qui
  n'ont donc rien à changer d'autre que leur config MCP.
- Stockage par agent : `bridge/memory/<runner>-<agent>.jsonl` (une ligne par souvenir : id, ts,
  texte, tags, source), sans base de données ; recherche par mots-clés normalisés (minuscules,
  sans accents) et score simple (termes trouvés, récence) ; plafond de taille avec compaction.
- Exposés aussi en CLI (`discord memory search "…"`, `memory add`) et dans la TUI (panneau
  mémoire) ; injectés comme serveur MCP de l'agent au même titre que ses pairs (§ pairs
  automatiques), donc disponibles pour Claude, Hermes et Antigravity.
- Partage optionnel : `memory_search {scope: "swarm"}` interroge la mémoire des pairs (lecture
  seule), pour qu'un `coordinator` retrouve ce que les autres ont appris.
- Aucun secret mémorisé : filtrage des motifs de jetons à l'écriture (même règle que `config`).

### 4.7 Outils MCP de l'instance : de 11 à 7 *(0.2.0, avec K9)*

**État** : absent — les 11 outils gardent leurs noms, aucun alias, `tools_merge.rs` est vide.

Audit du 2026-11-27 : plusieurs outils font la même chose sous des noms différents, et deux noms
trompent le modèle (`agent_control` agit sur les **runs**, pas sur les agents ; `agent_start` ne
lance pas un tour, il rend l'agent **résident** — confusion fréquente avec `run_agent`).

| Besoin | Aujourd'hui | Doublon |
|---|---|---|
| lancer | `run_agent`, `run_agents_parallel` | mêmes arguments (`run_object`) : le parallèle n'est qu'une liste de `run_agent` |
| attendre | `waitAll` du parallèle, ou `run_agent {background}` + `agent_control wait` | deux chemins |
| arrêter un tour | `agent_control kill`, `agent_stop`, `stop` du résident | trois portes |
| lister | `list_agents` (run en cours), `agent_control list` (runs), `list_templates` | recouvrement |
| état | `agent_control status`, `status` du résident, `list_agents` | trois sources |
| configuration | `get_agent_configs`, `update_agent_config` | paire lire / écrire |

Cible (7 outils) :

| Outil | Remplace |
|---|---|
| `create_agent` | inchangé (`resident:true` reste un raccourci) |
| `delete_agent` | inchangé |
| `list_agents` | + `list_templates` (`{templates:true}`) |
| `agent_config {action: get\|set}` | `get_agent_configs`, `update_agent_config` |
| `run_agent` | + `run_agents_parallel` (`tasks:[…]`, 10 max, `waitAll`) |
| `run_control {status\|stream\|wait\|kill\|list}` | `agent_control` (renommé : il agit sur les runs) |
| `agent_resident {start\|stop\|list}` | `agent_start`, `agent_stop` |

- Les outils **propres à un résident** (`ask`, `status`, `stop`, `history`, `logs`, `config`,
  `session`) ne changent pas : ce sont ceux de l'agent, pas de l'instance.
- **Compatibilité** : les anciens noms restent une version comme alias (même handler, description
  « déprécié : utiliser … »), retirés en 0.3.0.
- **Arrêts, vérifié le 2026-11-27** : dans le même process, `kill` atteint le tour en cours du
  résident ; depuis une autre instance, il ne tue que le CLI, pas la file ; un run fini gardé 1 h
  n'est plus « en cours » et n'a pas de repli disque. À corriger avant de fusionner les arrêts.
- **Clients à adapter** : MCP Control appelle `agent_control`, `agent_start`, `agent_stop`,
  `list_agents`, `run_agent`, `run_agents_parallel`, `get_agent_configs`, `update_agent_config`,
  `create_agent`, `delete_agent` ; la CLI (`agents`, `run`, `runs`) et les tests e2e
  (`tools_list.rs` compte les outils).

### 4.8 Compilation native des agents persistants *(remplace `executable:true` par copie)*

**État** : partiel — génération du crate, `serve_compiled`, build partagé, binaire, signature
macOS et `delete_agent` faits. Manquent : suivi du build par `agent_control`, régénération par
`swarm-control build`, icône Windows, `.env` à chaud + `reload`, `rebuildNeeded`, ligne `doctor`,
docs et e2e.

- `create_agent {executable:true}` (et `swarm-control build <agent>` en CLI) génère
  `<workspace>/agents/<nom>/build/` : `Cargo.toml` (dépendance `swarm-control` **à la version du
  binaire qui génère**, `default-features` selon la TUI), `src/main.rs` et `agent.json` :
  ```rust
  const AGENT: &str = "discord";
  const CLASS: &str = "discord";
  const DEFINITION: &str = include_str!("../agent.json"); // définition PAR DÉFAUT, sans secret
  fn main() -> std::process::ExitCode { swarm_control::serve_compiled(AGENT, CLASS, DEFINITION) }
  ```
- Nouvelle entrée de lib **`serve_compiled(agent, class, definition)`** : nom et classe gravés ;
  `--agent` sur la ligne de commande est **refusé** (un binaire compilé ne sert que son agent).
- **Ce qui fait foi** :

  | Élément | Où | Prise en compte |
  |---|---|---|
  | nom, classe | gravés dans le binaire | recompiler (`swarm-control build <agent>`) |
  | persona, modèle, serveurs MCP | définition du workspace | tour suivant |
  | identifiants de l'agent (settings Claude `env`, `.env` du profil Hermes, `env` des serveurs MCP) | fichiers de l'agent | tour suivant (le CLI est relancé à chaque tour) |
  | `<workspace>/.env` | workspace | tour suivant (relu si modifié, voir plus bas) |
  | version de swarm-control | dépendance épinglée | recompiler |

  La définition embarquée n'est qu'un **défaut** : elle est réécrite dans le workspace au premier
  démarrage si le dossier de l'agent manque, jamais utilisée sinon. Elle ne contient **aucun
  secret** (la génération refuse un `agent.json` qui en contient, même filtre que `config`).
- **`.env` du workspace relu à chaud** : aujourd'hui chargé une seule fois au démarrage
  (`config::get().dotenv`) ; il sera relu au début d'un tour quand sa date de modification
  change, et `swarm-control reload <agent>` (outil MCP `reload` du résident) force la relecture.
  Les variables de l'environnement système gardent la priorité.
- `cargo build --release` en **arrière-plan** (run suivi par `agent_control`, journal de build
  dans `logs`) ; dossier de compilation **partagé** `<workspace>/build/target` : le premier agent
  coûte 1 à 3 min et quelques centaines de Mio, les suivants 5 à 20 s (seuls leur `main` et
  l'édition de liens changent). Binaire final dans `<workspace>/bin/<agent>(.exe)`.
- **Icône et métadonnées** : `build.rs` pose sous Windows l'icône de la classe
  (`assets/classes/hornet-pixel-<classe>`, convertie en `.ico` à la génération) et la version ;
  rien sous Linux ; macOS : signature ad hoc (`codesign -s -`) si disponible.
- **Prérequis vérifiés avant** : `cargo` dans le `PATH` (ou `CARGO`), cible de l'hôte installée ;
  `doctor` ajoute une ligne « compilation d'agents ». Sans Rust : erreur explicite avec le lien
  `rustup`, l'agent reste créé (utilisable via l'instance), seul l'exécutable manque.
- **Recompilation** : seulement pour changer de nom, de classe ou de version de swarm-control
  (`swarm-control build <agent>`) ; `status` signale `rebuildNeeded` quand la version du binaire
  diffère de celle de l'instance. Modifier persona, modèle, MCP ou identifiants ne demande rien.
- **Hors ligne** : `cargo build --offline` si le registre local a déjà les dépendances ; sinon la
  première compilation a besoin du réseau (crates.io).
- **Retrait** : copie de `current_exe()` (`src/agents/executable.rs`), identité par nom de fichier
  (`src/cli/exe.rs::agent_name`) et `run()` qui la consulte — changement cassant, 0.2.0 ;
  `delete_agent` supprime aussi `build/`.

## 5. Corrections, optimisations et tests

### Corrections

| # | Quoi | Où | Correction |
|---|---|---|---|
| K1 ✅ | **README corrompu dans l'arbre** : accents en double encodage (séquences d'octets C3 83 xx / C3 A2 E2 82 AC xx / C3 82 xx : l'UTF-8 relu en Windows-1252 puis réécrit) | `README.md` | ré-encodé dans l'arbre le 2026-11-27 ; contrôle en place : `scripts/check-encoding.sh` (verify.sh + job release-scripts) |
| K2 ✅ | **`main` local et `origin/main` divergent** (6 commits locaux, 7 distants dont `v0.1.2`) ; le force-push est désormais interdit par la règle GitHub | git | fusionner `origin/main` dans `main` (pas de rebase forcé), résoudre README / release.yml |
| K3 ✅ | Tag `v0.1.2` hors de `main` et release « Latest » en retard | git, README | `v0.1.2` rattaché à `main` ; README en liens `/main/` ; la 0.2.0 reste à publier (§5 Distribution) |
| K4 ✅ | Exemples `sniper` partout alors que l'agent du README est **discord** | README, `help.rs`, `cli/mod.rs`, `lib.rs`, docs | remplacer par `discord` (exemple de pont : `--bridge 3011`, pas 3001 déjà pris par le worker Node) |
| K5 ✅ | README périmé : badge « ~2 Mio » (mesuré 3,02 Mio avec TUI) ; « À venir : pairs injectés » alors que c'est fait | README | mettre à jour |
| K6 ✅ | Fichiers de travail non suivis à la racine : `test-lanceur*.tmp.sh`, `lancer-profil.sh`, `post-discord-*.md`, `SKILL.autoclaw-profils.md` | racine | supprimer ou ranger (`work/`, ignoré) |
| K7 ⏳ | Tags `v*` non protégés : un tag de release peut être déplacé | GitHub | règle de tag (création réservée, ni déplacement ni suppression) |
| K9 ✅ | **Compatibilité OverMind à retirer** (non supporté) : variables `OVERMIND_*` (`config/env.rs`, `server/auth.rs`, `config/timeouts.rs`), dossier `~/.overmind` (`config/paths.rs`), lecture de `bridge/agents.json` (`sessions/legacy.rs`), `OVERMIND_AGENT_NAME` passé aux runners (`runners/claude`, `hermes/command.rs`, `antigravity`), commentaires « comme l'OverMind TS » | `src/`, tests e2e | supprimer ; renommer le dialecte « overmind-bridge » en **dialecte Discord** dans le code, l'aide (`help.rs`, `args.rs`), les tests (`the_bridge_speaks_the_overmind_bridge_dialect`, `context_matches_the_overmind_format`, `multi_instance.rs` l. 99) ; changement cassant → version 0.2.0 et note de migration au CHANGELOG |
| K10 🟡 | Docs qui promettent encore la compatibilité OverMind | `docs/ARCHITECTURE.md` (l. 89), `docs/RESIDENT-AGENTS.md` (l. 72) | retirer les deux phrases qui annoncent la lecture de `bridge/agents.json` : le fichier n'est plus lu depuis K9 |
| K8 ✅ | **Test instable** `resident_hosting::agent_stop_closes_the_server_and_frees_everything` : échoue dans la suite complète (l. 27, « écoute encore »), passe seul 3 fois sur 3 — un autre test reprend le port libéré dans 23080-23099 | `tests/e2e/resident_hosting.rs` | vérifier que le port ne sert plus **cet** agent (`initialize` → nom `swarm-alice`) au lieu de « port fermé », ou lier les résidents de test sur port 0 |
| K11 ⏳ | **`main` locale en retard de 5 commits** sur `origin/main` (README et visuels) ; l'arbre local modifie les mêmes fichiers | git | fast-forward puis merge avant le commit ; ne pas écraser les visuels distants |

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
- **Compilation** (§4.8) : génération du crate (Cargo.toml, main.rs, agent.json sans secret), refus
  de `--agent` sur un binaire compilé, la définition du workspace l'emporte sur l'embarquée,
  `.env` relu quand sa date change, erreur claire sans `cargo`.
- **Outils** : `create_agent.rs` (options `executable`/`unit`/`template`), `run_args.rs`,
  `run_agent.rs`, `list_agents.rs`, `agent_control/disk.rs`, `run_agents_parallel/launch.rs`.
- **Agents** : `lookup.rs`, `list/claude.rs`, `list/antigravity.rs`, `hermes/create.rs`,
  `claude/settings_edit.rs`, `antigravity/update.rs`.
- **CLI / TUI** : `cli/ask.rs` (choix hébergé / pair / local), `cli/agent.rs`, `tui/mod.rs`
  (boucle d'événements sur un faux terminal `ratatui::backend::TestBackend`).
- **Nouveaux** : mémoire native (§4.6 : stockage, recherche sans accents, compaction, filtrage des jetons, portée `swarm`), dialectes du pont (§4.2), modèles (§4.1 : chaque modèle intégré se charge et
  valide son schéma), planificateur (§4.4 : pas de rafale, pas de tour concurrent).

### Robustesse et sécurité
- Signature LiteMCP (`auth-sign`) / TLS avant toute écoute hors loopback.
- Smoke réel Claude avec un vrai jeton.
- Agents X et Google : aucune publication ni envoi de mail sans garde-fou (mode brouillon par
  défaut, `SWARM_ALLOW_PUBLISH=1` pour l'autoriser).

### Distribution
- **0.2.0** : `Cargo.toml` en 0.2.0, note de migration au CHANGELOG (binaire compilé, outils
  regroupés, OverMind retiré), publication crates.io et binaires 5 cibles.
- Logo docs.rs figé sur le tag de la version.
- Homebrew / Scoop si la demande existe.

## 6. Agent Discord — migration du serveur Ubuntu 192.168.40.9

État constaté le 2026-11-27 (lecture seule) : Discord → `discord-master` (Node, un token, 8
préfixes) → `POST localhost:3001/send` → `overmind-bridge.js` (Node) → `overmind-mcp` :3099 →
`run_agent(hermes, sniperbot_analyst)` ; l'agent répond lui-même via `serveur_discord`. Les
agents `trade*` ont la même forme (≈ 57 Mio par worker Node). **Toute la chaîne OverMind
disparaît** : seul `discord-master` (le routeur) reste, le temps d'avoir le routeur Rust.

1. Créer l'agent dans un **workspace swarm-control propre** (pas le workspace OverMind) avec le
   modèle `discord` (classe `discord`), sa persona reprise du SOUL actuel, et sa config MCP
   **sans `overmind-mcp`** : `serveur_discord` + la mémoire native (§4.6).
2. Compiler le binaire de l'agent pour Linux : sur le serveur (Rust à installer avec `rustup`
   dans le compte `demon` — **à valider avant**), ou ailleurs pour `x86_64-unknown-linux-gnu`
   puis déposer le fichier ; le lancer `--headless --port 23090 --bridge 3011` sans rien arrêter.
3. Tester au `curl` (`/health`, `/send` vers un salon de test, `/reset`) et vérifier que
   `memory_store` / `memory_search` répondent depuis swarm-control.
4. Basculer le préfixe de `discord-master` vers :3011, observer, puis arrêter
   `discord-llm.service` **et `overmind-mcp`**. **Chaque modification du serveur validée avant.**
5. Répéter pour `trade2`…`trade7`, puis remplacer `discord-master` par un routeur Rust (crate à
   part : Discord exige un runtime async).

## 7. Limites assumées

- Le CLI du modèle est relancé à chaque tour : la conversation reste dans sa session.
- Pas de base de données ni de mémoire vectorielle : la mémoire native (§4.6) est un journal
  JSONL avec recherche par mots-clés ; une recherche sémantique reste le rôle d'un serveur MCP tiers.
- Les agents tournent avec les droits du compte qui les lance.
