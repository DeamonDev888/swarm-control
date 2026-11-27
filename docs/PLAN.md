# Plan — swarm-control

*Mis à jour le 2026-11-26.*

## 1. But

Un binaire Rust léger (LiteMCP, sans runtime async ni base de données) qui rend des agents
**persistants** (Claude Code, Hermes), les **orchestre** et les expose en **MCP natif**.
Pas d'audit, pas de mémoire PostgreSQL.

## 2. État (0.1.0)

- 10 outils MCP, schémas JSON publiés et validés (DSL zod de LiteMCP).
- Sessions reprises, registre de processus, parallèle, arrière-plan, `kill`, retente de session.
- Structure : une lib + un binaire de 4 lignes ; ~160 fichiers, aucun au-dessus de ~150 lignes
  ([ARCHITECTURE.md](ARCHITECTURE.md)).
- **Étape 2 faite : agents résidents** (serveur MCP propre `swarm-<agent>`, file, `ask` / `status` /
  `stop`, 10 outils côté instance). Mesuré : ≈ 80 Kio et 2 fils par agent résident au repos.
- **Étape 1 des agents résidents faite** : état par agent (`bridge/state/`), bail inter-process
  (`bridge/owners/`), ancien `agents.json` toujours lu. Deux instances sur un même workspace
  partagent les sessions et ne lancent jamais le même agent ensemble
  ([RESIDENT-AGENTS.md](RESIDENT-AGENTS.md)).
- **Étape 3 faite : instances numérotées** (`swarm-control-N`, `--instance`, annonce + anti-zombie)
  et **routage** : `run_agent` / `run_agents_parallel` d'un agent hébergé vont à son hôte, repli à
  la demande sinon.
- Qualité : 108 tests (73 unitaires, 35 de bout en bout avec un faux CLI), clippy `-D warnings`,
  MSRV 1.85 vérifiée, `cargo deny` propre.
- Publication : CI + `release.yml` (verify → crates.io → release GitHub → binaires)
  ([RELEASE.md](RELEASE.md)).
- Mesures (release, Windows 11) : binaire 1,93 Mio, ≈ 7 Mio de mémoire au repos.

## 3. Vérifié contre les vrais CLI

- **Hermes (v0.22.5)** : création de profil, prompt par stdin (`--query-file -`), ID de session sur
  stderr (`session_id: local-…`), suppression. Un profil jetable a été créé puis supprimé.
- **Claude Code** : forme réelle de l'enveloppe JSON ; `--resume` d'un ID inexistant →
  « No conversation found » sur stderr, code 1 (c'est ce que détecte la retente).
- Sur une copie de données réelles : 91 agents Claude et 16 profils Hermes lus, secrets masqués.

## 4. Pas encore vérifié (identifiants manquants)

- Un run Hermes **réussi** avec un vrai modèle, puis sa reprise.
- Un run Claude **réussi** avec un vrai jeton.

Smoke test à faire une fois : `create_agent(runner:"hermes", cloneFrom:"<profil qui marche>")`,
deux `run_agent`, le second doit se souvenir du premier ; idem pour un agent Claude.

## 5. Limites assumées

- Latence Hermes ≈ 10 à 18 s par appel (démarrage de Python), car on passe par le CLI.
- Verrou d'agent **en processus** : un second serveur, ou un `hermes` lancé à la main, ne se voit pas.
- Pas d'authentification sur le port (loopback) ; agents en `--dangerously-skip-permissions`.
- Les runs en cours sont perdus si le serveur redémarre (PID lisibles sur disque, pas la sortie).

## 6. Feuille de route (décidée le 2026-11-26 ; étapes 1 à 3 faites, 4 et 5 à venir)

Définition retenue : *chaque agent créé = 1 profil et 1 exécutable ; il vit, se souvient et
communique avec ses pairs ; compatible Hermes (natif), Claude Code, Antigravity, grokbot,
opencode, Kilo/Cline, OpenClaw, extensible à tout runner CLI.*

**Décisions prises**
- Pairs : **outils MCP**. Chaque agent est un serveur MCP annoncé dans le registre LiteMCP
  (`announce()`, `%TEMP%/litemcp-registry/<nom>.json`) ; ses pairs sont ajoutés à sa propre
  configuration MCP, son modèle les appelle comme des outils (`bob.ask`).
- Hébergement : **agents résidents**, sans fermer la porte à **plusieurs instances** (`swarm-control-1`,
  `swarm-control-2`…) — conception et payloads dans [RESIDENT-AGENTS.md](RESIDENT-AGENTS.md).
  Un agent = un serveur MCP propre (annonce `swarm-<agent>`), une file, un fil de travail ; le CLI
  reste lancé une fois par tour. Une seule instance héberge un agent (verrou de propriété).
- grokbot : **plus tard** (commande et options inconnues ; aucun binaire sur la machine).
- Construit : résidents, instances numérotées, routage. **Reste** : un exécutable par agent (étape 4),
  pairs injectés dans la config MCP (étape 5), autres runners, CLI complète.

**Chantiers, dans l'ordre proposé**
1. *Runner générique* : un runner décrit par un fichier JSON (commande, livraison du prompt, reprise,
   modèle, extraction de la session). Repères des CLI, tirés de l'ancien OverMind TS :
   `opencode run <prompt> --session <id>` · `kilo <prompt> --agent <mode> --session <id> --model <m> --auto
   --format json --dir <cwd>` (session dans les événements JSON) · `gemini -p <prompt> --model
   antigravity/<mode> --session-id <uuid> --approval-mode yolo --output-format json` · `cline [--mode m]
   [--resume id] <prompt>` · `qwen -p <prompt>` · `openclaw message send …`.
   **Vérifiables ici** : opencode, kilo, gemini/antigravity, qwen. **Absents** : cline, openclaw, grokbot.
2. *Un agent = 1 profil + 1 exécutable* : dossier de profil par agent et exécutable à son nom ;
   plusieurs agents peuvent partager un exécutable.
3. *Pairs* : annonce, découverte, injection des pairs dans la configuration MCP de chaque agent
   (Claude : `--mcp-config` ; Hermes : `mcp_servers` du profil ; autres : selon le runner).
4. *CLI complète* : sous-commandes, intro ASCII, mode debug, `schema <outil>`.

## 7. Suite

1. Smoke test réel (section 4).
2. Créer le dépôt GitHub `DeamonDev888/swarm-control`, renseigner `CARGO_REGISTRY_TOKEN`, publier `0.1.0`.
3. CLI (section 6).
4. Optionnel : reprise Hermes par titre (`-c <agent> --create-if-missing`), `--format stream-json`,
   authentification LiteMCP (`auth-sign`, TLS).
