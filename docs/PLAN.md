# Plan — swarm-control

*Mis à jour le 2026-11-27. Liste à cocher : [TODO.md](TODO.md).*

## 1. Vision

Un binaire Rust léger (LiteMCP, sans runtime async ni base de données), pour **Windows, macOS et
Linux**, qui fabrique et pilote des agents IA :

```
swarm-control                       serveur MCP d'administration (instance swarm-control-N)
 ├─ create_agent(executable:true) ─▶ sniper(.exe)   agent PERSISTANT, son propre exécutable
 │                                    ├─ MCP riche   ask · status · stop · history · session · logs · config
 │                                    ├─ CLI         sniper ask "…" · status · history · reset · serve
 │                                    ├─ TUI         MCP branchés · conversation · session · file · logs
 │                                    ├─ pont HTTP   --bridge (Discord & co, compatible overmind-bridge)
 │                                    └─ --debug     requêtes MCP, commandes lancées, reprises, retentes
 └─ run_agent(sans agentName) ────▶ agent ÉPHÉMÈRE : un tour, aucune session gardée
```

- **Persistant** = un nom, une définition sur disque (persona, modèle, serveurs MCP), une session
  reprise à chaque tour, une file de demandes, un exécutable.
- **Éphémère** = un prompt, un tour, rien de sauvegardé (déjà possible : `run_agent` sans
  `agentName`).
- Hors périmètre, assumé : audit, mémoire PostgreSQL, runtime async.

## 2. État au 2026-11-27 (0.1.1 publiée, `main` plus loin)

| Pièce | État |
|---|---|
| Serveur MCP, 10 outils, schémas JSON validés | ✅ 0.1.0 |
| Runners Claude Code et Hermes, sessions reprises, retente à neuf | ✅ 0.1.0 |
| Agents résidents (serveur `swarm-<agent>`, file, `ask` / `status` / `stop`) | ✅ 0.1.0 |
| Bail inter-process, instances numérotées, routage vers l'hôte | ✅ 0.1.0 |
| Binaires Windows / macOS arm64 / Linux x86_64 par release | ✅ 0.1.0 |
| Logo crates.io / docs.rs ; PID hors bornes sous Unix | ✅ 0.1.1 |
| **Mode agent** : binaire renommé ou `--agent`, `--port`, `serve_agent()` | ✅ `main` (4a2d1cf) |
| **Pont HTTP** `--bridge` compatible overmind-bridge (`/send` `/reset` `/status` `/health`) | ✅ `main` (4a2d1cf) |
| Runner **Antigravity** | 🟡 en cours dans l'arbre de travail, non commité (autre chantier) |
| Journal des tours, MCP riche, CLI d'agent, TUI, `--debug` | ❌ §4 |
| `create_agent(executable:true)` | ❌ §4 |

Qualité : 117 tests sur `main` (79 unitaires, 38 de bout en bout avec un faux CLI), clippy
`-D warnings`, MSRV 1.85, `cargo deny`, CI sur les 3 OS.

## 3. Décisions

1. **Pairs = outils MCP** : chaque agent résident est un serveur MCP annoncé dans le registre
   LiteMCP ; ses pairs sont injectés dans sa config MCP, son modèle les appelle (`bob.ask`).
2. **Résidents + plusieurs instances** : un agent = un serveur, une file, un fil de travail ; le CLI
   du modèle est relancé à chaque tour. Une seule instance héberge un agent (bail).
3. **Un seul binaire** : `swarm-control` sert une instance ; copié sous le nom d'un agent, il sert
   cet agent. Lancé dans un terminal → TUI ; sous systemd ou `--headless` → MCP + pont, sans
   interface. `sniper ask …` passe par le serveur MCP de l'agent s'il tourne déjà.
4. **Journal des tours propre à swarm-control** (§4.1) : la conversation vit aussi dans la session
   du CLI (SQLite chez Hermes, JSONL chez Claude), mais chacun dans son format ; un journal à nous,
   indépendant du runner, nourrit `history`, la CLI et la TUI.
5. **TUI** : `ratatui` + `crossterm` (synchrones, pas de runtime async), derrière une feature `tui`
   active par défaut dans les binaires publiés ; la lib reste légère pour qui l'importe.
6. **grokbot** : plus tard (commande inconnue, aucun binaire).

## 4. Feuille de route (dans l'ordre)

### 4.1 Journal des tours et MCP riche *(socle commun)*
- `bridge/history/<runner>-<agent>.jsonl` : une ligne par tour `{ts, from, via, prompt, reply,
  error, sessionId, durationMs, queuedMs}`, écrite par le fil de travail ; rotation par taille
  (ex. 5 Mio) ; secrets jamais journalisés (prompts déjà sans `env`).
- Outils de l'agent : `history {limit, before}`, `session {action: show|reset|set}`, `logs` (fin
  de sortie du tour en cours / du dernier), `config` (lecture masquée).
- Le pont `/reset` et l'outil `session reset` partagent `Ctx::reset_session`.

### 4.2 CLI de l'agent et `--debug`
- Sous-commandes : `serve` (défaut), `ask "<prompt>"`, `status`, `history [-n N]`, `reset`,
  `config`, `schema <outil>` ; côté instance : `agents`, `run`, `runs`, `doctor`.
- Client MCP minimal réutilisé (`peers`) : la CLI parle au serveur de l'agent s'il tourne, sinon
  exécute le tour elle-même (même bail, même session).
- `--debug` / `SWARM_DEBUG=1` : une ligne par requête MCP, commande lancée (secrets masqués),
  session reprise ou oubliée, retente, bail pris ou repris.

### 4.3 TUI de l'agent
- Panneaux : en-tête (agent, runner, modèle, session, ports MCP et pont) · serveurs MCP branchés
  et leur état (sonde `/health` ou `initialize`) · conversation (journal §4.1, défilement) · file
  et tour en cours (qui, depuis quand, sortie en direct) · logs / debug.
- Saisie : écrire un message = un `ask` ; raccourcis `r` reset, `s` stop, `d` debug, `q` quitter
  (l'agent continue sous systemd s'il n'a pas été lancé par la TUI).

### 4.4 Fabrication d'agents
- `create_agent {executable:true}` : copie le binaire dans `<workspace>/bin/<agent>(.exe)`, rend
  son chemin ; `delete_agent` le retire. Option `unit:true` (Linux) : écrit une unit systemd
  `--headless`, sans l'activer.
- `run_agent` sans `agentName` documenté comme **éphémère** (aucune session gardée, aucun
  fichier d'état).
- Dossier de profil par agent (aujourd'hui : workspace partagé) ; plusieurs agents peuvent
  partager un exécutable.

### 4.5 Migration de Sniper (serveur 192.168.40.9) — voir §6
### 4.6 Pairs automatiques
- Découverte des `swarm-*` du registre, injection dans la config MCP de chaque agent (Claude :
  `--mcp-config` ; Hermes : `mcp_servers` du profil ; Antigravity : selon le runner).
### 4.7 Runner générique
- Un runner décrit en JSON (commande, livraison du prompt, reprise, modèle, extraction de la
  session). Vérifiables ici : opencode, kilo, gemini, qwen. Absents : cline, openclaw, grokbot.

## 5. Corrections et optimisations à faire

### À corriger vite
| # | Quoi | Où | Pourquoi |
|---|---|---|---|
| C1 | **CI rouge sur `main`** : 2 tests de `cli::exe` utilisent des chemins Windows, où `\` n'est un séparateur que sous Windows | `src/cli/exe.rs` | corrigé dans l'arbre (cas Windows sous `cfg(windows)`), **à committer et pousser** |
| C2 | `Holder::is_stale` teste le PID **avant** l'âge du bail : sous Windows chaque test lance `tasklist` (≈ 50–100 ms) même pour un bail déjà périmé | `src/owners/holder.rs` | inverser : âge d'abord, PID ensuite |
| C3 | Pont `/send` : si le client HTTP se déconnecte, le tour en file n'est pas annulé | `src/residents/bridge/routes.rs` | sonder la socket pendant l'attente (comme `ask` avec le jeton d'annulation) |
| C4 | Action `softprops/action-gh-release` sur Node 20 (déprécié par GitHub) | `.github/workflows/release.yml` | monter de version (Dependabot le proposera) |
| C5 | Docs en retard : PLAN §5 (fait ici), `RESIDENT-AGENTS.md` ne connaît pas le mode agent ni le pont | `docs/` | mettre à jour |

### Robustesse
- **Runs perdus au redémarrage** : seuls PID et statut sont sur disque, pas la sortie. Écrire la fin
  de sortie dans le journal (§4.1) suffit pour la consultation.
- **Pont et serveurs sans authentification** : écoute en `127.0.0.1` par défaut et agents en
  `--dangerously-skip-permissions`. Avant toute écoute hors loopback : signature LiteMCP
  (`auth-sign`) / TLS côté MCP, jeton partagé côté pont.
- **Tests sensibles au temps** : plusieurs tests e2e attendent un état disque juste après une action
  (un cas corrigé dans `control`). Passer en revue `resident_queue` et `multi_instance` avec
  `eventually`.
- **Couverture** : le pont n'est testé qu'avec un agent Claude ; ajouter un agent Hermes (sortie
  sur stdout, session sur stderr) et un `/reset` pendant un tour en cours.
- **Smoke tests réels jamais faits** : un run Hermes réussi avec un vrai modèle puis sa reprise ;
  idem Claude avec un vrai jeton.

### Performance et légèreté
- `pid_alive` sous Windows lance un process `tasklist` par appel : regrouper (un `tasklist` pour
  tous les PID d'un passage du balayeur), ou cache court par PID.
- Pont : un fil par connexion (plafond 64) ; suffisant pour un routeur Discord, à revoir si un
  agent sert beaucoup de clients (file déjà bornée par le fil de travail unique).
- Latence Hermes ≈ 10–18 s par tour (démarrage Python) : inhérente au CLI ; piste future, le mode
  serveur de Hermes s'il en expose un.
- Garder l'objectif « quelques Mio » : mesurer mémoire et taille du binaire à chaque release
  (la TUI les fera grossir, d'où la feature `tui`).

### Distribution
- Binaires manquants : **macOS x86_64** et **Linux aarch64** (serveurs ARM, Raspberry Pi).
- Logo docs.rs pointé sur `main` : le figer sur le tag de la version pour qu'une refonte d'image
  ne change pas les docs déjà publiées.
- Paquets : `cargo binstall` (métadonnées), puis Homebrew / Scoop si la demande existe.

## 6. Migration de Sniper (serveur Ubuntu 192.168.40.9)

État constaté le 2026-11-27 (lecture seule) : Discord → `discord-master` (Node, un token, 8
préfixes) → `POST localhost:3001/send` → `overmind-bridge.js` (Node, `discord_llm`) →
`overmind-mcp` :3099 → `run_agent(hermes, sniperbot_analyst)` ; profil Hermes MiniMax-M3.1-Flash,
7 serveurs MCP ; l'agent répond lui-même sur Discord via `serveur_discord`. Les 7 agents
`trade*` ont la même forme (≈ 57 Mio par worker Node).

1. Construire le binaire Linux, le déposer à côté (sans rien arrêter) ; lancer
   `sniperbot_analyst --headless --port 23090 --bridge 3011` sur le workspace OverMind.
2. Tester au `curl` (`/health`, `/send` vers un salon de test, `/reset`).
3. **Mémoire** : la SOUL appelle `memory_search` / `memory_store` d'overmind-mcp ; garder
   overmind-mcp pour la mémoire au début (déjà dans la config MCP du profil).
4. Basculer `!sniper` de `discord-master` vers le port 3011, observer, puis arrêter le worker Node
   (`discord-llm.service`). **Modifications du serveur : chacune validée avant.**
5. Répéter pour `trade2`…`trade7`, puis remplacer `discord-master` par un routeur Rust (crate à
   part : Discord exige un runtime async, que la lib n'embarque pas).

## 7. Limites assumées

- Le CLI du modèle est relancé à chaque tour : la conversation reste dans sa session.
- Pas de base de données ni de mémoire vectorielle : c'est le rôle d'un serveur MCP tiers.
- Les agents tournent avec les droits du compte qui les lance.
