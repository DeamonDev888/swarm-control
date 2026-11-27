# Publier une version

Pipeline : **lint → compile → test → publication crates.io → release GitHub → binaires.**

```
 scripts/release.sh X.Y.Z          (local)   bump + CHANGELOG daté + verify + commit + tag
        │  git push origin main && git push origin vX.Y.Z
        ▼
 .github/workflows/release.yml     (tag v*)
   1. verify          cohérence (tag ↔ Cargo.toml ↔ Cargo.lock ↔ CHANGELOG)
                      + fmt · clippy -D warnings · tests · doc stricte · paquet · cargo-deny
   2. publish         crates.io, idempotent (200 déjà publié · 404 publier · autre = échec)
   3. github-release  notes = section « ## [X.Y.Z] » du CHANGELOG
   4. binaries        Linux · macOS · Windows, archives + SHA-256 joints à la release
```

La release GitHub n'existe **que si** le crate est publié : une publication crates.io est
irréversible (on ne peut que `yank`), une release GitHub se corrige.

## Au quotidien

| Besoin | Commande |
|---|---|
| Vérifier comme la CI | `bash scripts/verify.sh` (`ALLOW_DIRTY=1` si l'arbre n'est pas commité) |
| Cohérence des versions | `bash scripts/release-check.sh [X.Y.Z]` |
| Simuler la publication | `DRY_RUN=1 bash scripts/publish.sh X.Y.Z` |
| Préparer une release | `bash scripts/release.sh X.Y.Z` (arbre propre, branche `main`) |

CI (`ci.yml`, sur chaque push et PR) : `lint`, `test` (Linux, macOS, Windows), `msrv` (1.85),
`package`, `deny`, `release-scripts`, `attribution` (aucun trailer `Co-Authored-By: Claude`).

## Mise en place unique

1. **Dépôt GitHub** : `DeamonDev888/swarm-control` (l'URL est déjà dans `Cargo.toml`).
2. **Jeton crates.io** : créer un jeton limité à la publication de `swarm-control`, puis le
   stocker dans *Settings → Secrets → Actions* sous le nom `CARGO_REGISTRY_TOKEN`.
   Sans secret, le job `publish` tente le *trusted publishing* (OIDC) : à configurer côté
   crates.io, et à ma connaissance possible seulement une fois le crate créé — la toute
   première publication passe donc par le jeton (à vérifier sur crates.io).
3. **Protection** : protéger la branche `main` (CI requise) et le motif de tag `v*`.
4. **Dependabot** est déjà configuré (`.github/dependabot.yml`) : crates et actions.

## Publier `0.2.0`, pas à pas

```bash
# 1. Remplir « ## [Unreleased] » dans CHANGELOG.md, committer.
# 2. Préparer (bump, CHANGELOG daté, verify, commit « release: v0.2.0 », tag annoté) :
bash scripts/release.sh 0.2.0
# 3. Relire (git show), puis pousser — le tag déclenche release.yml :
git push origin main
git push origin v0.2.0
```

## Incidents

| Situation | Que faire |
|---|---|
| `verify` échoue | rien n'a été publié : corriger, puis supprimer et recréer le tag |
| `publish` OK, `binaries` échoue | relancer le job (idempotent) ; ou publier à la main les binaires |
| Version cassée déjà publiée | `cargo yank --version X.Y.Z`, corriger, publier `X.Y.(Z+1)` |
| Republier sans nouveau tag | *Actions → Release → Run workflow* (champ `version`) : vérifie et publie, sans créer de release |
