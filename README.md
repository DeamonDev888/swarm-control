# overmind-rs

OverMind réécrit en Rust sur [LiteMCP](https://crates.io/crates/litemcp) : **des agents persistants** (Claude Code, Hermes) pilotés par MCP. Un binaire de ~2 Mio, zéro runtime async, pas de base de données, pas d'audit.

*(Nom de travail : renomme le dossier et le champ `name` de `Cargo.toml` quand tu auras choisi.)*

## Ce qui rend un agent persistant

1. **Définition sur disque** : un agent est un prompt (persona) + une config, créés une fois par `create_agent`.
2. **Session reprise** : chaque run d'un agent nommé reprend sa dernière session (`--resume`), puis sauvegarde la nouvelle dans `bridge/agents.json` (TTL 30 jours). L'agent se souvient d'un appel à l'autre.
3. **Registre de processus** : chaque run est suivi en mémoire (PID, statut, sortie en direct) et sur disque (`bridge/process-registry.json`). On peut le lire, l'attendre, le tuer ; un balayeur marque les PID morts `orphaned`.
4. **Un seul run vivant par agent** : deux reprises simultanées corrompraient la session → `AGENT_BUSY`.
5. **Session périmée** : si `--resume` échoue (« No conversation found »…), la session est oubliée et le run repart à neuf, une fois.

## Lancer

```powershell
cargo run --release -- --workspace C:\Users\Deamon\.overmind
```

Port : premier libre de `23080..=23099` (ou `--port N`). Côté client MCP :

```json
{ "mcpServers": { "overmind": { "type": "http", "url": "http://127.0.0.1:23080/mcp" } } }
```

Workspace (même résolution que l'OverMind TS) : `--workspace` → `OVERMIND_WORKSPACE` → `~/.overmind/.env` → dossier contenant `.mcp.json` en remontant → `~/.overmind`.

## Outils (8)

| Outil | Rôle |
|---|---|
| `run_agent` | Lance un agent. `agentName` ⇒ reprise auto de session (`autoResume`, défaut `true`). `background:true` rend un `runId`. |
| `run_agents_parallel` | Jusqu'à 10 agents en parallèle ; `waitAll:false` rend la main au premier succès. |
| `agent_control` | `status` · `stream` (sortie en direct) · `wait` · `kill` (arbre de processus) · `list`. |
| `create_agent` | Claude : `.claude/agents/<n>.md` + `settings_<n>.json` + `.mcp.<n>.json`. Hermes : profil natif (`hermes profile create`) + `SOUL.md`. |
| `list_agents` | Agents Claude + Hermes, état de session, run en cours. |
| `get_agent_configs` | Lit les fichiers d'un agent, **secrets masqués**. |
| `update_agent_config` | `model`, `mcpServers`, `env`, `prompt`, ou réécriture d'un fichier. |
| `delete_agent` | Supprime l'agent et sa session ; refusé si un run est en cours. |

## Compatibilité avec l'OverMind TS

Mêmes fichiers, mêmes formats : `.claude/agents/*.md`, `.claude/settings_<n>.json`, `.claude/.mcp.<n>.json`, `bridge/agents.json`, `bridge/process-registry.json`, profils Hermes natifs. Tes agents et sessions existants fonctionnent tels quels.

**Retiré volontairement** : audits (Loi 25), mémoire PostgreSQL (`memory_*`, `memory_runs`), `a2a_hub`, `get_metrics`, `config_example`, `create_prompt`/`edit_prompt` (remplacés par `update_agent_config`), les 6 autres runners (Gemini, Kilo, Qwen, OpenClaw, Cline, OpenCode), le bridge Discord/webhooks, la gateway HTTP Hermes, la télémétrie.

## Différences de comportement

- **`autoResume` vaut `true` par défaut** dès qu'`agentName` est fourni (l'ancien défaut était `false`).
- **Pas de secret sur disque** : le fichier de settings passé à Claude ne contient plus `env` ; les variables (jetons) passent par l'environnement du process. Plus de `settings_<n>_tmp.json` avec jetons.
- **MCP isolés** : un agent Claude reçoit `--strict-mcp-config` avec *seulement* ses serveurs.
- **Persona** : le prompt de l'agent est transmis à Claude via `--append-system-prompt` (l'ancien code ne le passait pas au CLI). Au-delà de 20 000 caractères, il est préfixé au prompt de la première session.
- **Hermes** : prompt envoyé par stdin (`--query-file -`) : pas de souci de guillemets ni de longueur. Un code de sortie non nul est une **erreur** (Hermes y met le message du fournisseur sur stdout), avec un indice actionnable.
- **Credentials Hermes par profil** : un profil neuf n'hérite pas de tes clés. `create_agent` avertit, et `cloneFrom:<profil>` copie config, `.env`, SOUL et skills d'un profil qui a déjà les siennes. Noms Hermes : minuscules, chiffres, `_`, `-`, 64 max.
- **Alias de modèles** : `<workspace>/model-aliases.json` (`{"surnom": "vrai-modèle"}`), au lieu de la table codée en dur.
- Retente de jeton sur 401/429/5xx via `AUTH_FALLBACK_1..3` / `ANTHROPIC_AUTH_TOKEN_FALLBACK` (Claude).

## Variables

`CLAUDE_BIN`, `HERMES_BIN` (chemins des CLI), `HERMES_HOME`, `OVERMIND_TIMEOUT_MS` (défaut 15 min ; un agent Claude peut surcharger via `AGENT_TIMEOUT_MS` dans son `env`), `OVERMIND_CLAUDE_PERMISSIONS` (défaut `--dangerously-skip-permissions`).

## Tests

```powershell
cargo test
```

14 tests unitaires + 11 tests de bout en bout : le vrai serveur sur port éphémère, un faux CLI (`examples/fake_cli.rs`) à la place de `claude`/`hermes`. Ils couvrent création, reprise de session, session périmée, kill, timeout, jeton de secours, Hermes complet (dont erreurs fournisseur, `cloneFrom`, noms), parallèle, masquage des secrets.

Voir [PLAN.md](PLAN.md) pour l'état de la vérification et la suite.

## Sécurité

Le serveur écoute en `127.0.0.1` sans authentification (comportement LiteMCP par défaut) et lance des agents en `--dangerously-skip-permissions` : **ne l'expose pas sur le réseau**. LiteMCP sait signer les requêtes (feature `auth-sign`) et faire du TLS si tu veux l'ouvrir.
