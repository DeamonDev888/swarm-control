# TODO — swarm-control

*État au 2026-11-27. Le pourquoi et le détail sont dans [PLAN.md](PLAN.md) (les numéros §
renvoient à ses sections).*

## Fait

- [x] Refonte en couches commitée ; dépôt GitHub créé ; CI sur les 3 OS.
- [x] 0.1.0 et 0.1.1 publiées (crates.io, release GitHub, binaires) ; logo sur crates.io et docs.rs.
- [x] Mode agent (binaire renommé, `--agent`, `--port`, `serve_agent()`) et pont HTTP `--bridge`
      compatible overmind-bridge — `main` 4a2d1cf.

## 1. Corrections urgentes (PLAN §5)

- [ ] **C1** CI rouge : committer et pousser le correctif des tests `cli::exe` (chemins Windows sous `cfg(windows)`).
- [ ] **C2** `Holder::is_stale` : tester l'âge du bail avant le PID (évite un `tasklist` inutile sous Windows).
- [ ] **C3** Pont `/send` : annuler le tour en file si le client HTTP se déconnecte.
- [ ] **C4** `release.yml` : monter `softprops/action-gh-release` (Node 20 déprécié).
- [ ] **C5** `RESIDENT-AGENTS.md` : documenter le mode agent et le pont.
- [ ] Runner Antigravity (en cours dans l'arbre, autre chantier) : terminer, tester, committer à part.

## 2. Journal des tours et MCP riche (PLAN §4.1)

- [ ] `bridge/history/<runner>-<agent>.jsonl` écrit par le fil de travail, avec rotation.
- [ ] Outils d'agent `history`, `session` (show / reset / set), `logs`, `config`.
- [ ] Tests e2e : historique après 3 tours, rotation, `session reset`.

## 3. CLI de l'agent et `--debug` (PLAN §4.2)

- [ ] Sous-commandes d'agent : `serve`, `ask`, `status`, `history`, `reset`, `config`, `schema <outil>`.
- [ ] Sous-commandes d'instance : `agents`, `run`, `runs`, `doctor`.
- [ ] `--debug` / `SWARM_DEBUG=1` : requêtes MCP, commandes lancées (secrets masqués), sessions, retentes, baux.
- [ ] `--headless` explicite (systemd) ; TUI par défaut dans un terminal (après §4).

## 4. TUI de l'agent (PLAN §4.3)

- [ ] Feature `tui` (`ratatui` + `crossterm`), active dans les binaires publiés.
- [ ] Panneaux : en-tête · serveurs MCP et leur état · conversation · file et tour en cours · logs.
- [ ] Saisie = `ask` ; raccourcis reset / stop / debug / quitter.
- [ ] Mesurer taille du binaire et mémoire avant/après.

## 5. Fabrication d'agents (PLAN §4.4)

- [ ] `create_agent {executable:true}` → `<workspace>/bin/<agent>(.exe)` ; `delete_agent` le retire.
- [ ] Option `unit:true` (Linux) : unit systemd `--headless`, non activée.
- [ ] Documenter `run_agent` sans `agentName` comme agent éphémère.
- [ ] Dossier de profil par agent ; exécutable partagé entre plusieurs agents.

## 6. Migration de Sniper — serveur 192.168.40.9 (PLAN §6)

- [ ] Binaire Linux déposé à côté ; `sniperbot_analyst --headless --port 23090 --bridge 3011`.
- [ ] Tests au `curl` : `/health`, `/send` vers un salon de test, `/reset`.
- [ ] Garder overmind-mcp :3099 pour la mémoire (`memory_search` / `memory_store`) au début.
- [ ] Basculer `!sniper` de `discord-master` vers :3011, puis arrêter `discord-llm.service` *(validé avant)*.
- [ ] Puis `trade2`…`trade7` ; plus tard un routeur Discord en Rust (crate à part).

## 7. Robustesse, perfs, distribution (PLAN §5)

- [ ] Smoke tests réels : Hermes (vrai modèle, reprise) et Claude (vrai jeton).
- [ ] Tests e2e sensibles au temps : passer `resident_queue` et `multi_instance` en `eventually`.
- [ ] Pont testé avec un agent Hermes, et `/reset` pendant un tour.
- [ ] `pid_alive` Windows : un `tasklist` groupé par passage du balayeur, ou cache court.
- [ ] Authentification avant toute écoute hors loopback (LiteMCP `auth-sign` / TLS ; jeton pour le pont).
- [ ] Binaires macOS x86_64 et Linux aarch64 ; métadonnées `cargo binstall`.
- [ ] Logo docs.rs figé sur le tag de version.
- [ ] GitHub : image d'aperçu `assets/social-preview.jpg`, protection de `main` et des tags `v*`.

## 8. Plus tard (PLAN §4.6, §4.7)

- [ ] Pairs automatiques : découverte des `swarm-*`, injection dans la config MCP de chaque agent.
- [ ] Runner générique décrit en JSON : opencode, kilo, gemini, qwen ; puis cline, openclaw, grokbot.
- [ ] Reprise Hermes par titre (`-c <agent> --create-if-missing`), sortie `--format stream-json`.
