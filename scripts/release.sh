#!/usr/bin/env bash
# Prépare une release LOCALE : bump → CHANGELOG daté → verify → commit → tag.
# Ne pousse RIEN : tu relis, puis tu pousses toi-même (le tag déclenche release.yml).
#
# Usage : scripts/release.sh X.Y.Z      (depuis un arbre propre, sur `main`)
# Nécessite GNU sed (Git Bash sous Windows, Linux).
set -euo pipefail
cd "$(dirname "$0")/.."

version="${1:?usage: release.sh X.Y.Z}"
version="${version#v}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || { echo "version invalide: $version" >&2; exit 1; }

[ -z "$(git status --porcelain)" ] || { echo "arbre de travail non propre" >&2; exit 1; }
[ "$(git rev-parse --abbrev-ref HEAD)" = "main" ] || { echo "release depuis main uniquement" >&2; exit 1; }
git rev-parse -q --verify "refs/tags/v${version}" >/dev/null && { echo "le tag v${version} existe déjà" >&2; exit 1; }
grep -q '^## \[Unreleased\]' CHANGELOG.md || { echo "CHANGELOG.md sans section [Unreleased]" >&2; exit 1; }

# 1. version dans Cargo.toml (section [package] uniquement) puis Cargo.lock
sed -i '/^\[package\]/,/^\[/{s/^version *= *".*"/version = "'"${version}"'"/}' Cargo.toml
cargo update --workspace

# 2. [Unreleased] devient la nouvelle version, datée d'aujourd'hui
sed -i "s/^## \[Unreleased\]/## [Unreleased]\n\n## [${version}] - $(date +%F)/" CHANGELOG.md

# 3. cohérence + gate qualité complet
bash scripts/release-check.sh "$version" >/dev/null
ALLOW_DIRTY=1 bash scripts/verify.sh

# 4. commit + tag annoté (aucun trailer d'attribution : la CI le refuse)
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -q -m "release: v${version}"
git tag -a "v${version}" -m "swarm-control ${version}"

cat <<EOF

Release v${version} prête en local. Relis puis pousse :

  git push origin main
  git push origin v${version}     # déclenche .github/workflows/release.yml
EOF
