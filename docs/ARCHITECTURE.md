# Architecture

Principe : **beaucoup de petits fichiers, peu de code par fichier**. Un fichier = une
responsabilité ; aucun ne dépasse ~150 lignes. `mod.rs` ne contient que des déclarations
et des ré-exports (plus, pour un module d'orchestration, une fonction de câblage de moins
de 30 lignes). Les tests unitaires vivent en bas du fichier testé, ou dans `tests.rs`
quand ils sont longs ; les tests de bout en bout sont dans `tests/e2e/`.

## Couches (du plus bas au plus haut)

Un module ne dépend que des couches **au-dessus** de lui dans ce tableau (plus bas niveau).

| # | Module | Rôle | Dossiers / fichiers clés |
|---|---|---|---|
| 1 | `store` | JSON atomique, horloge | `json_file.rs`, `clock.rs` |
| 2 | `config` | workspace, `.env`, interpolation `$VAR`, home Hermes, délais | `workspace.rs`, `dotenv.rs`, `env.rs`, `interpolate.rs`, `hermes_home.rs`, `timeouts.rs`, `state.rs` |
| 3 | `binaries` | localisation de `claude` et `hermes` | `claude.rs`, `hermes.rs`, `path_lookup.rs` |
| 4 | `state` | état persistant **par agent** : un fichier `bridge/state/<runner>-<agent>.json` | `doc.rs`, `path.rs` |
| 5 | `sessions` | session d'un agent, dans son état ; repli en lecture sur l'ancien `agents.json` | `get.rs`, `save.rs`, `delete.rs`, `legacy.rs`, `entry.rs` |
| 6 | `process` | lancer, capturer, attendre, tuer un arbre de processus | `run.rs`, `readers.rs`, `wait.rs`, `kill.rs`, `alive.rs`, `stdin.rs`, `utf8.rs` |
| 7 | `owners` | **bail** d'un agent : un seul process le fait tourner (inter-process) | `acquire.rs`, `takeover.rs`, `lease.rs`, `holder.rs`, `inspect.rs`, `sweep.rs` |
| 8 | `registry` | runs vivants (mémoire) + dernier run sur disque + bail + balayeur | `index.rs`, `live_run.rs`, `lifecycle.rs`, `disk.rs`, `sweeper.rs`, `output_buffer.rs` |
| 9 | `agents` | créer / lister / modifier / supprimer un agent | `claude/`, `hermes/`, `list/`, `lookup.rs`, `paths.rs`, `secrets.rs` |
| 10 | `runners` | cycle d'un run : reprise de session, lancement, retente, garde anti-panique | `execute.rs`, `prepare.rs`, `guard.rs`, `claude/`, `hermes/` |
| 10b | `peers` | client MCP minimal (std) vers un autre serveur LiteMCP | `call.rs`, `http.rs`, `url.rs` |
| 11 | `residents` | **agents résidents** : serveur MCP propre, file, fil de travail, hébergement | `start.rs`, `stop.rs`, `host.rs`, `ctx.rs`, `inbox.rs`, `worker.rs`, `waiting.rs`, `server.rs`, `reset.rs`, `mcp/`, `bridge/` (pont HTTP, dialecte Discord) |
| 11b | `routing` | un run d'agent hébergé est transmis à son hôte (`ask`) | `eligible.rs`, `forward.rs` |
| 11c | `instance` | numéro d'instance `swarm-control-N` (bail + battement) | `claim.rs` |
| 12 | `tools` | les 10 outils MCP de l'instance (schéma + handler) | un fichier ou dossier par outil |
| 13 | `server` | serveur LiteMCP de l'instance : construction, écoute, annonce | `builder.rs`, `bind.rs`, `banner.rs` |
| 14 | `cli` | ligne de commande (`--resident`, `--workspace`…) | `args.rs`, `help.rs`, `serve.rs` |

`main.rs` (4 lignes) appelle `swarm_control::run()` ; toute la logique est dans la lib.

## Flux d'un `run_agent`

```
tools/run_agent ──▶ routing::forward   agent hébergé (ici ou ailleurs) ? → `ask` chez l'hôte, fin.
                │                        sinon (ou hôte injoignable) :
                ├─▶ runners::prepare   valide l'agent, réserve le run :
                │                        · table en mémoire (AGENT_BUSY si déjà vivant ici)
                │                        · owners::acquire — bail sur disque (AGENT_BUSY si vivant ailleurs)
                └─▶ runners::execute   ├─ sessions::get      session à reprendre (autoResume)
                                       ├─ runners::claude|hermes::run
                                       │     ├─ plan / args / env    (claude)   args / command (hermes)
                                       │     └─ process::run         spawn, stdin, lecteurs, wait
                                       ├─ session morte ? → sessions::delete + relance à neuf
                                       ├─ sessions::save     (fichier d'état de l'agent)
                                       └─ registry::finish   statut + résultat (mémoire + `lastRun`) + bail libéré
```

Le balayeur (toutes les 30 s) renouvelle le bail des runs vivants : un bail sans battement
depuis 2 min, ou dont le PID est mort, est périmé et repris par le prochain demandeur.
```

## Flux d'un `ask` sur un agent résident

```
client MCP ─▶ swarm-<agent> (son propre serveur LiteMCP, annoncé dans le registre)
                └─ ask : chain::check (cycle ? chaîne trop longue ?)
                         ctx.submit ──▶ Inbox (file FIFO)          ← une demande par tour
                                          └─▶ worker (un seul fil par agent)
                                                ├─ waiting::prepare   attend le bail si un run « à la demande » tient l'agent
                                                ├─ runners::execute   (même cycle que `run_agent` : reprise de session…)
                                                └─ Turn::complete ──▶ l'appelant de `ask` reçoit la réponse
```

Chaque agent résident = 2 fils (serveur, travail) + sa file. Le bail d'**hébergement**
(`bridge/hosts/`) est détenu par le fil de travail, qui le renouvelle toutes les 30 s ; les baux de
**run** (`bridge/owners/`) restent pris par chaque tour, comme pour un `run_agent`.

## Fichiers d'état

Un fichier par agent : deux agents ne se réécrivent jamais l'un l'autre, même dans des
instances différentes. Pour un même agent, un seul process écrit à la fois (celui qui
détient son bail). Les fichiers de l'agent Claude / Hermes eux-mêmes sont ceux que leurs CLI
attendent (`.claude/`, profils Hermes).

| Fichier | Rôle |
|---|---|
| `<ws>/.claude/agents/<n>.md` | persona de l'agent Claude |
| `<ws>/.claude/settings_<n>.json`, `.mcp.<n>.json` | config et serveurs MCP de l'agent Claude |
| `<ws>/bridge/state/<runner>-<agent>.json` | **état de l'agent** : `session {id, ts}` (TTL 30 j) et `lastRun {runId, pid, status, …}` |
| `<ws>/bridge/owners/<runner>-<agent>.json` | **bail de run** de l'agent pendant un run : `{instance, pid, run_id, ts}` |
| `<ws>/bridge/hosts/<runner>-<agent>.json` | **bail d'hébergement** d'un agent résident : idem + `url`, `runner`, `agent` |
| `%TEMP%/swarm-control-instances/<N>.json` | **bail de numéro d'instance** (`swarm-control-N`) |
| `%TEMP%/litemcp-registry/swarm-control-<N>.json` | annonce LiteMCP du serveur d'administration de l'instance |
| `%TEMP%/litemcp-registry/swarm-<agent>.json` | annonce LiteMCP du serveur de l'agent résident (port, PID, instance) |
| `<ws>/bridge/agents.json` | ancien fichier de sessions hérité d'OverMind (plus supporté) : **lu** en repli, jamais écrit — retiré en 0.2.0 |
| `<hermes>/profiles/<n>/` | profil Hermes (géré par le CLI `hermes`) |
| `<ws>/model-aliases.json` | surnoms de modèles (optionnel) |

## Ajouter un outil MCP

1. Créer `src/tools/<nom>.rs` avec `pub fn register(builder) -> ServerBuilder` : schéma
   `zod::object()…` + handler `fn handle(args: Args) -> ToolResult`.
2. Déclarer `mod <nom>;` et l'appeler dans `tools/mod.rs`.
3. Ajouter un test dans `tests/e2e/` et mettre à jour `tools_list.rs`.

## Ajouter un runner (ex. Gemini)

1. `agents/runner.rs` : variante + `as_str` / `parse`.
2. `binaries/<runner>.rs` : localisation du CLI.
3. `agents/<runner>/` : `create`, `update`, `delete`, `configs` (même forme que `claude/`).
4. `runners/<runner>/` : `mod.rs` (`run`), `args.rs`, et l'analyse de sa sortie.
5. Aiguiller dans `agents/{create,update,delete,configs}.rs`, `runners/execute.rs`,
   `tools/schema.rs` (`RUNNERS`) et `registry/index.rs` (`RUNNERS`).
6. Étendre `examples/fake_cli/` et ajouter les tests `tests/e2e/<runner>_*.rs`.
