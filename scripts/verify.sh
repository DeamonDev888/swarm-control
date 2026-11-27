#!/usr/bin/env bash
# Gate qualité : le même que la CI.
#   fmt → clippy (-D warnings) → tests → doc stricte → paquet crates.io → cargo-deny
# Utilisé en local avant un commit et par le job `verify` de release.yml.
#
# ALLOW_DIRTY=1 : accepte un arbre de travail non commité pour `cargo package`.
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n==> %s\n' "$*"; }

step "encodage markdown (anti double-encodage UTF-8)"
scripts/check-encoding.sh

step "format"
cargo fmt --all -- --check

step "clippy (avertissements = erreurs)"
cargo clippy --all-targets --locked -- -D warnings

step "tests"
cargo test --locked

step "documentation (rustdoc strict)"
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked

step "paquet crates.io (contenu + compilation depuis l'archive)"
# Dossier de build PROPRE à cette étape. Sans cela, `cargo package` compile la copie
# `target/package/…` dans `target/debug` et y laisse ses artefacts (exécutable, `.d`) :
# le `cargo build` suivant se croit alors à jour et ne voit plus les modifications.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target}/package-check"
if [ -n "${ALLOW_DIRTY:-}" ]; then
  cargo package --locked --allow-dirty
else
  cargo package --locked
fi
unset CARGO_TARGET_DIR

step "cargo-deny (advisories, licences, sources)"
if command -v cargo-deny >/dev/null 2>&1; then
  cargo deny check
else
  echo "cargo-deny absent : ignoré en local (la CI l'exécute)"
fi

printf '\nverify OK\n'
