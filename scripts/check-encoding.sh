#!/usr/bin/env bash
# Anti double-encodage UTF-8 sur tous les *.md suivis par git.
#
# Un texte UTF-8 ré-encodé par erreur via CP1252/Latin-1 laisse des artefacts
# caractéristiques :
#   Ã   (C3 83)      : tout « é/è/à/ç… » doublement encodé (é → C3 A9 → C3 83 C2 A9)
#   â€  (C3 A2 E2…)  : guillemets/apostrophes détruits (“ ” ’ → â€œ â€<9d> â€<99>)
#   Â   (C3 82)      : espace insécable doublement encodée (C2 A0 → C3 82 C2 A0)
# Le motif « â€ » couvre aussi « €œ » (â€œ). Appariement octet à octet (LC_ALL=C).
#
# Sortie : fichier:ligne + extrait. Code 1 dès qu'un fichier est sale.
set -euo pipefail
cd "$(dirname "$0")/.."

export LC_ALL=C

status=0
count=0
while IFS= read -r -d '' f; do
  count=$((count + 1))
  hits="$(grep -aFn -e 'Ã' -e 'â€' -e 'Â' -- "$f" || true)"
  if [ -n "$hits" ]; then
    status=1
    printf 'DOUBLE-ENCODAGE %s\n' "$f"
    printf '%s\n' "$hits"
  fi
done < <(git ls-files -z -- '*.md')

if [ "$status" -eq 0 ]; then
  printf 'encodage OK : %d fichier(s) *.md contrôlé(s), aucun artefact de double encodage\n' "$count"
fi
exit "$status"
