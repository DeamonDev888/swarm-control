#!/usr/bin/env bash
# Mesures à consigner dans le CHANGELOG à chaque release (PLAN §5 « Optimisations ») :
#   1. taille du binaire release (octets et Mio) ;
#   2. mémoire au repos d'un agent headless (serveur MCP démarré, aucune demande en cours) :
#      WorkingSet64 sous Windows, RSS (`ps -o rss`) sous Linux et macOS. L'agent est lancé
#      puis arrêté proprement (TERM, puis KILL) dans un workspace temporaire.
#
# Usage : scripts/measure.sh [--json]
#   (sans option)  deux lignes prêtes à coller dans le CHANGELOG, sur stdout.
#   --json         les mêmes mesures en un seul objet JSON sur stdout (version, binaire :
#                  chemin, octets, Mio ; mémoire au repos : octets, Mio ; date UTC), pour
#                  `jq` ou PowerShell `ConvertFrom-Json`. Rien d'autre sur stdout.
#   -h, --help     cette aide.
# La progression (« ==> … ») va toujours sur stderr : stdout ne porte que le résultat.
#
# Ne construit pas : le binaire release doit déjà exister
# (target/release/swarm-control[.exe]) ; sinon le script s'arrête avec un message (code 1).
# Une mesure impossible (binaire absent, agent sans bannière, mémoire illisible) est un
# échec explicite, jamais un 0 silencieux.
#
# Sous Windows, à lancer depuis Git Bash (la mémoire se lit via PowerShell) ; sous Linux
# et macOS, `ps` suffit.
set -euo pipefail
cd "$(dirname "$0")/.."

usage() {
  cat <<'FIN'
Usage : scripts/measure.sh [--json]
  (sans option)  taille du binaire release + mémoire au repos d'un agent headless,
                 deux lignes prêtes à coller dans le CHANGELOG (stdout)
  --json         les mêmes mesures en un objet JSON (stdout), pour jq / ConvertFrom-Json
  -h, --help     cette aide
Ne construit pas : lance `cargo build --release` d'abord.
FIN
}

json=0
for arg in "$@"; do
  case "$arg" in
    --json) json=1 ;;
    -h | --help) usage; exit 0 ;;
    *) echo "option inconnue : $arg" >&2; usage >&2; exit 2 ;;
  esac
done

step() { printf '\n==> %s\n' "$*" >&2; }

exe="target/release/swarm-control"
[ -f "$exe.exe" ] && exe="$exe.exe"
if [ ! -f "$exe" ]; then
  echo "pas de build release ($exe) : le script ne construit pas, lance \`cargo build --release\` d'abord." >&2
  exit 1
fi
version="$("$exe" --version)"

# Chemin Windows pour l'exécutable natif sous Git Bash (les variables d'environnement
# ne sont pas converties automatiquement, contrairement aux arguments).
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) W() { cygpath -w "$1"; } ;;
  *) W() { printf '%s' "$1"; } ;;
esac

step "taille du binaire release ($exe)"
bytes="$(wc -c < "$exe" | tr -d '[:space:]')"
mib="$(awk -v b="$bytes" 'BEGIN { printf "%.2f", b / 1048576 }')"

step "mémoire au repos d'un agent headless (workspace temporaire)"
root="$(mktemp -d "${TMPDIR:-/tmp}/swarm-measure.XXXXXX")"
ws="$(W "$root")/ws"
mkdir -p "$root/ws" "$root/hermes/profiles/mesure"
# Un profil Hermes factice suffit pour que l'agent résolve au démarrage (aucun tour lancé).
printf 'model:\n  default: mesure\n  provider: hermes\n' \
  > "$root/hermes/profiles/mesure/config.yaml"

# Environnement isolé, comme les tests e2e : registre LiteMCP et HOME sous $root, ports
# éphémères (0) pour ne pas toucher à la tranche 23080..=23099 ni aux instances réelles.
export HERMES_HOME="$(W "$root/hermes")"
export TEMP="$(W "$root")" TMP="$(W "$root")" TMPDIR="$(W "$root")"
export HOME="$(W "$root")" USERPROFILE="$(W "$root")"
export SWARM_AGENT_PORT=0
unset SWARM_WORKSPACE OVERMIND_WORKSPACE 2>/dev/null || true

pid=""
arrete() {
  [ -n "$pid" ] || return 0
  kill -TERM "$pid" 2>/dev/null || true
  for _ in $(seq 1 30); do
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.1
  done
  kill -KILL "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  rm -rf "$root"
}
trap arrete EXIT

"$exe" --agent mesure --headless --workspace "$ws" --port 0 \
  >/dev/null 2>"$root/banniere.log" &
pid=$!

# La bannière « … MCP sur http://127.0.0.1:PORT/mcp … » annonce le démarrage effectif.
for _ in $(seq 1 100); do
  grep -q 'MCP sur http://127.0.0.1:' "$root/banniere.log" 2>/dev/null && break
  kill -0 "$pid" 2>/dev/null || { cat "$root/banniere.log" >&2; exit 1; }
  sleep 0.1
done
grep -q 'MCP sur http://127.0.0.1:' "$root/banniere.log" || {
  cat "$root/banniere.log" >&2
  exit 1
}
sleep 3 # laisser le démarrage retomber au repos

octets=""
wpid=""
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*)
    # PID Windows retrouvé par la ligne de commande (le workspace jetable est unique),
    # mémoire = WorkingSet64 lu par PowerShell (pas d'outil Unix fiable sous Git Bash).
    wpid="$(powershell.exe -NoProfile -Command \
      "(Get-CimInstance Win32_Process -Filter \"Name='swarm-control.exe'\" | \
        Where-Object { \$_.CommandLine -like '*$ws*' } | Select-Object -First 1).ProcessId" \
      | tr -d '\r')"
    [[ "$wpid" =~ ^[0-9]+$ ]] && \
      octets="$(powershell.exe -NoProfile -Command "(Get-Process -Id $wpid).WorkingSet64" | tr -d '\r')"
    ;;
  *)
    # `ps -o rss` rend des kioctets.
    octets="$(ps -o rss= -p "$pid" | tr -d '[:space:]' | awk '{ printf "%d", $1 * 1024 }')"
    ;;
esac
if ! [[ "$octets" =~ ^[0-9]+$ ]] || [ "$octets" = "0" ]; then
  echo "mémoire au repos illisible (pid « $wpid », valeur « $octets ») : mesure annulée." >&2
  exit 1
fi
mem_mib="$(awk -v o="$octets" 'BEGIN { printf "%.2f", o / 1048576 }')"

step "arrêt de l'agent (TERM, puis KILL, puis nettoyage du workspace temporaire)"
if [ "$json" = 1 ]; then
  printf '{"version":"%s","binary":"%s","bytes":%s,"mib":%s,"memory_idle_bytes":%s,"memory_idle_mib":%s,"measured_at":"%s"}\n' \
    "$version" "$exe" "$bytes" "$mib" "$octets" "$mem_mib" "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
else
  printf '\nÀ coller dans le CHANGELOG :\n- Binaire release (%s) : %s Mio (%s octets)\n- Mémoire au repos d\x27un agent headless (%s) : %s Mio\n' \
    "$version" "$mib" "$bytes" "$version" "$mem_mib"
fi
