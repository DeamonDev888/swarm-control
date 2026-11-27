#!/usr/bin/env bash
# Affiche la version du paquet (section [package] de Cargo.toml).
set -euo pipefail
cd "$(dirname "$0")/.."
sed -n '/^\[package\]/,/^\[/{s/^version *= *"\([^"]*\)".*/\1/p}' Cargo.toml | head -n1
