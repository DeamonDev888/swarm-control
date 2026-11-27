#!/usr/bin/env bash
# Cohérence d'une version avant publication :
#   tag / argument  ↔  Cargo.toml  ↔  Cargo.lock  ↔  section du CHANGELOG.
#
# Usage : release-check.sh [X.Y.Z]
#   sans argument : le tag `vX.Y.Z` de GITHUB_REF_NAME, sinon la version de Cargo.toml.
# Sortie : `version=X.Y.Z` sur stdout (diagnostics sur stderr) ; code 1 si incohérent.
set -euo pipefail
cd "$(dirname "$0")/.."

fail() { echo "ERREUR: $*" >&2; exit 1; }

manifest=$(bash scripts/version.sh)
wanted="${1:-}"
if [ -z "$wanted" ] && [ "${GITHUB_REF_TYPE:-}" = "tag" ]; then
  wanted="${GITHUB_REF_NAME:-}"
fi
wanted="${wanted#v}"
wanted="${wanted:-$manifest}"

[[ "$wanted" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] \
  || fail "version '$wanted' invalide (attendu X.Y.Z)"
[ "$wanted" = "$manifest" ] \
  || fail "version demandée ($wanted) ≠ Cargo.toml ($manifest)"

lock=$(awk '/^name = "swarm-control"$/ {found=1; next} found && /^version/ {gsub(/"/, "", $3); print $3; exit}' Cargo.lock)
[ "$lock" = "$manifest" ] \
  || fail "Cargo.lock ($lock) ≠ Cargo.toml ($manifest) — lance « cargo update --workspace »"

grep -q "^## \[$wanted\]" CHANGELOG.md \
  || fail "section « ## [$wanted] » absente de CHANGELOG.md"

echo "cohérence OK pour $wanted" >&2
echo "version=$wanted"
