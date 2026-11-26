# Plan — OverMind-rs (nom de travail)

*Réécrit le 2026-11-25 après vérification contre l'OverMind TS, le code de Hermes et des runs réels.*

## 1. But

Un binaire Rust (LiteMCP, ~2 Mio, sans runtime async ni base de données) qui rend des agents **persistants** : Claude Code et Hermes, pilotés par MCP. Pas d'audit, pas de mémoire PostgreSQL.

## 2. Ce qui existe (23 tests verts, clippy propre)

| Brique | Fichier | Rôle |
|---|---|---|
| Config | `src/config.rs` | workspace, `.env`, home Hermes natif, interpolation `$VAR` |
| Sessions | `src/sessions.rs` | `bridge/agents.json` (`runner:agent → id`), TTL 30 j, format identique au TS |
| Registre | `src/registry.rs` | runs vivants (PID, statut, sortie) + `bridge/process-registry.json` + balayeur |
| Processus | `src/process.rs` | lancement, lecture de flux, timeout, kill de l'arbre, annulation SSE |
| Agents | `src/agents.rs` | création / liste / maj / suppression, Claude (`.claude/`) et Hermes (profils) |
| Runners | `src/runners.rs` | cycle de run, reprise, session morte, jetons de secours |
| Outils | `src/tools.rs` | 8 outils MCP : `run_agent`, `run_agents_parallel`, `agent_control`, `create_agent`, `list_agents`, `get_agent_configs`, `update_agent_config`, `delete_agent` |

## 3. Vérifié

**Avec de vrais CLI**
- Hermes (v0.22.5) : création de profil (`profile create`, `config set model.default` puis `model.provider`), lancement avec prompt sur stdin (`--query-file -`), récupération de l'ID de session (`session_id: local-<hash>` sur **stderr**), suppression de profil. Un profil jetable a été créé puis supprimé ; il n'en reste aucun.
- Claude Code : forme réelle de l'enveloppe JSON (`type`, `subtype`, `is_error`, `api_error_status`, `result`, `session_id`). Un `--resume` sur un ID inexistant sort **« No conversation found… » sur stderr, code 1**, sans JSON : c'est ce que détecte notre retente.
- Sur une copie de tes données : 91 agents Claude et 16 profils Hermes lus, secrets masqués.

**Avec un faux CLI (tests de bout en bout)** : reprise de session, session périmée, kill, timeout, jeton de secours 401, Hermes complet, parallèle, `AGENT_BUSY`, masquage des secrets, erreurs fournisseur Hermes, noms et `cloneFrom`.

## 4. Écarts trouvés pendant la vérification (corrigés)

| # | Constat réel | Correction |
|---|---|---|
| 1 | Hermes sort en **code 1 avec le message d'erreur sur stdout** (crédits épuisés). Notre runner le prenait pour un succès (comportement hérité du TS). | Code non nul = erreur, avec le texte conservé. |
| 2 | **Les credentials Hermes sont par profil** (`auth.json` propre à chacun). Un profil neuf n'a qu'une clé Zai ; l'hypothèse « pool global » de l'ancien code TS est fausse. | `create_agent` avertit si aucune credential n'est détectée, et accepte `cloneFrom` (copie config, `.env`, SOUL, skills d'un profil existant). Les erreurs « No usable credentials » affichent la commande d'aide. |
| 3 | Les noms de profil Hermes sont `^[a-z0-9][a-z0-9_-]{0,63}$` ; notre règle générale acceptait majuscules et 128 caractères. | Validation Hermes dédiée à la création. |
| 4 | `hermes profile delete` sort en code non nul alors que le profil est bien supprimé. | Le disque fait foi ; plus de fausse erreur. |
| 5 | `--pass-session-id` injecte l'ID **dans le prompt système**, il ne l'imprime pas : l'ID sort via `-Q`. | Aucun changement ; le parseur lit stderr puis stdout, ce qui est correct. |

## 5. Pas encore vérifié (bloqué par des identifiants)

- Un run Hermes **réussi** avec un vrai modèle, puis sa reprise (le mot-code est-il retenu ?). Mon profil jetable n'avait aucune clé valide : Zai « crédits épuisés », DeepSeek et MiniMax absents du profil. Copier tes clés dans un autre profil m'a été refusé, à juste titre.
- Un run Claude **réussi** avec ton `ANTHROPIC_AUTH_TOKEN` (mon shell ne l'a pas : 401).

À faire côté toi, une fois (le smoke test lui-même est automatisable ensuite) :
1. Lancer le serveur : `cargo run --release -- --workspace C:\Users\Deamon\.overmind`.
2. Créer un agent Hermes avec `cloneFrom` vers un profil qui marche (ex. `explorateur`), le lancer deux fois, vérifier que le 2ᵉ run se souvient du 1ᵉʳ.
3. Idem pour un agent Claude déjà existant.

## 6. Limites assumées

- Latence Hermes : ~10 à 18 s par appel (démarrage de Python), car on passe par le CLI et non par la gateway HTTP.
- Verrou d'agent **en processus** : deux serveurs OverMind, ou un `hermes` lancé à la main, ne se voient pas. Hermes protège sa session par un bail en base (`session_turn_leases`) ; Claude non.
- Pas d'authentification sur le port (loopback) et `--dangerously-skip-permissions` par défaut : ne pas exposer au réseau.
- Un seul poste : pas de persistance des runs en cours si le serveur redémarre (les PID restent lisibles sur disque, pas leur sortie).

## 7. Prochaines étapes

1. **Smoke test réel** (section 5) puis le figer en test `#[ignore]` lancé à la demande.
2. **Choisir le nom** et renommer (dossier, crate, binaire, README, messages). Candidats libres sur crates.io : `cerbere`, `frelon`, `hydre`, `meute`, `swarm-control`.
3. **Premier commit** puis dépôt GitHub.
4. Optionnel : reprise Hermes par titre (`-c <agent> --create-if-missing`), qui supprimerait `agents.json` côté Hermes et suivrait seule la compression de contexte. Non testé ; seulement lu dans le code de Hermes.
5. Optionnel : `--format stream-json` de Hermes (renvoie `exit_code` et `text` structurés) pour remplacer l'analyse du texte.
6. Optionnel : auth LiteMCP (`auth-sign`, TLS) si le serveur doit sortir de la machine.
