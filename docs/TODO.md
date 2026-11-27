# TODO — swarm-control

*État au 2026-11-27. Source : [PLAN.md](PLAN.md), [RESIDENT-AGENTS.md](RESIDENT-AGENTS.md), README.*

## 0. Urgent — sécuriser le travail fait

- [ ] **Committer la refonte** : tout le code découpé (`src/**`, `tests/e2e/**`, `examples/fake_cli/`),
      la doc, la CI, les scripts et les licences ne sont **pas encore dans git** (seul le premier commit,
      avec l'ancienne structure à plat, existe).
- [ ] Créer le dépôt GitHub `DeamonDev888/swarm-control` et ajouter le remote (aucun remote configuré).
- [ ] Pousser `main`, vérifier que `ci.yml` passe sur GitHub.

## 1. Valider contre les vrais CLI (PLAN §4)

- [ ] Smoke test Hermes : `create_agent(runner:"hermes", cloneFrom:"<profil qui marche>")`, puis deux
      `run_agent` — le second doit se souvenir du premier.
- [ ] Smoke test Claude : même chose avec un vrai jeton.
- [ ] Smoke test résident : `agent_start` + `ask` réel, puis routage depuis une 2ᵉ instance.

## 2. Publier la 0.1.0 (docs/RELEASE.md)

- [ ] Renseigner le secret `CARGO_REGISTRY_TOKEN` sur GitHub.
- [ ] `bash scripts/release-check.sh`, puis tag `v0.1.0` → `release.yml` (crates.io + release + binaires).
- [ ] Vérifier la page crates.io, docs.rs et les badges du README.
- [ ] Mettre l'image `assets/social-preview.jpg` comme aperçu du dépôt GitHub.

## 3. Ménage de la doc

- [ ] PLAN §5 est périmé : « verrou d'agent en processus » n'est plus vrai (bail inter-process
      `bridge/owners/` construit). À corriger.
- [ ] Mettre à jour les compteurs (108 tests) à chaque ajout.

## 4. Feuille de route (dans l'ordre décidé)

### 4.1 Runner générique
- [ ] Format JSON de description d'un runner (commande, livraison du prompt, reprise, modèle,
      extraction de session).
- [ ] Brancher les runners vérifiables ici : **opencode**, **kilo**, **gemini/antigravity**, **qwen**.
- [ ] Plus tard (binaires absents) : cline, openclaw, grokbot.
- [ ] Tests e2e avec le faux CLI pour un runner décrit en JSON.

### 4.2 Un agent = 1 profil + 1 exécutable (étape 4)
- [ ] Dossier de profil par agent.
- [ ] `alice.exe` = le binaire sous un autre nom ; le nom du fichier donne l'identité.
- [ ] Champ `executable` dans `create_agent` ; plusieurs agents peuvent partager un exécutable.

### 4.3 Pairs automatiques (étape 5)
- [ ] Outil / fonction `peers` : liste des `swarm-*` du registre LiteMCP.
- [ ] Injection des pairs dans la config MCP de chaque agent
      (Claude : `--mcp-config` ; Hermes : `mcp_servers` du profil ; autres : selon le runner).
- [ ] Test e2e : alice appelle `bob.ask`, refus de cycle via `via`.

### 4.4 CLI complète
- [ ] Sous-commandes : `serve`, `agents`, `run`, `runs`, `doctor`.
- [ ] Intro ASCII + affichage de ce que fait le serveur.
- [ ] Mode debug : traces MCP, commandes lancées, sessions reprises, retentes.
- [ ] `schema <outil>` : exposer les JSON Schema en ligne de commande.

## 5. Optionnel / plus tard

- [ ] Reprise Hermes par titre (`-c <agent> --create-if-missing`).
- [ ] Sortie `--format stream-json`.
- [ ] Authentification LiteMCP (`auth-sign`, TLS) — aujourd'hui loopback sans auth +
      `--dangerously-skip-permissions`.
- [ ] Survivre au redémarrage : les runs en cours perdent leur sortie (seuls PID/statut sont sur disque).
- [ ] Latence Hermes (10–18 s, démarrage Python) : piste à étudier si gênant.
