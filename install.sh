#!/bin/sh
# Installe swarm-control sous Linux ou macOS (dernière version publiée sur GitHub).
#
#   curl -fsSL https://raw.githubusercontent.com/DeamonDev888/swarm-control/main/install.sh | sh
#
# - Télécharge l'exécutable dans ~/.local/bin (ou dans SWARM_INSTALL_DIR).
# - Aucun sudo requis. Relancer le script met à jour l'exécutable.
set -eu

REPO="DeamonDev888/swarm-control"
DIR="${SWARM_INSTALL_DIR:-$HOME/.local/bin}"

os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Linux) os=linux ;;
  Darwin) os=macos ;;
  *) echo "Système non pris en charge : $os (Windows : utilisez install.ps1)" >&2; exit 1 ;;
esac
case "$arch" in
  x86_64 | amd64) arch=x86_64 ;;
  aarch64 | arm64) arch=aarch64 ;;
  *) echo "Architecture non prise en charge : $arch" >&2; exit 1 ;;
esac

ASSET="swarm-control-$os-$arch"
URL="https://github.com/$REPO/releases/latest/download/$ASSET"
DEST="$DIR/swarm-control"

echo "-> Téléchargement de $ASSET"
mkdir -p "$DIR"
TMP="$DEST.download"
if command -v curl >/dev/null 2>&1; then
  ok=0; curl -fSL --progress-bar "$URL" -o "$TMP" || ok=$?
elif command -v wget >/dev/null 2>&1; then
  ok=0; wget -q "$URL" -O "$TMP" || ok=$?
else
  echo "Il faut curl ou wget." >&2; exit 1
fi
if [ "$ok" -ne 0 ]; then
  rm -f "$TMP"
  echo "Échec du téléchargement depuis $URL" >&2
  echo "Pas de binaire publié pour $os/$arch ? Alternative : cargo install swarm-control" >&2
  echo "Ou téléchargez à la main : https://github.com/$REPO/releases/latest" >&2
  exit 1
fi
chmod +x "$TMP"
mv -f "$TMP" "$DEST"

echo ""
echo "OK  $("$DEST" --version) installé dans $DEST"

case ":$PATH:" in
  *":$DIR:"*) ;;
  *)
    echo ""
    echo "!  $DIR n'est pas dans votre PATH. Ajoutez cette ligne à ~/.bashrc ou ~/.zshrc :"
    echo "     export PATH=\"$DIR:\$PATH\""
    ;;
esac

echo ""
echo "Ensuite :"
echo "  swarm-control doctor     # vérifie que tout est prêt"
echo "  swarm-control            # démarre le serveur"
