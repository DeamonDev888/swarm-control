#!/usr/bin/env bash
# Publication crates.io IDEMPOTENTE.
#   200 → déjà publié (rien à faire) ; 404 → à publier ; autre → échec franc
#   (une erreur réseau n'est JAMAIS une preuve d'absence).
#
# Usage : publish.sh X.Y.Z        DRY_RUN=1 pour simuler (cargo publish --dry-run)
# Jeton : CARGO_REGISTRY_TOKEN (secret) ou jeton OIDC fourni par la CI.
set -euo pipefail
cd "$(dirname "$0")/.."

version="${1:?usage: publish.sh X.Y.Z}"
name="swarm-control"

code=$(curl -s -o /dev/null -w '%{http_code}' -A "swarm-control-release-script" \
  "https://crates.io/api/v1/crates/${name}/${version}" || true)

case "$code" in
  200) echo "${name} ${version} est déjà publié : rien à faire."; exit 0 ;;
  404) echo "${name} ${version} absent de crates.io : publication." ;;
  *)   echo "ERREUR: réponse inattendue de crates.io (HTTP ${code:-aucune}) — abandon." >&2; exit 1 ;;
esac

if [ -n "${DRY_RUN:-}" ]; then
  cargo publish --dry-run --locked --allow-dirty
else
  cargo publish --locked
fi
