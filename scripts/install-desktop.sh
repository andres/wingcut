#!/usr/bin/env bash
# Add wingcut to your desktop's application menu (just for your user, no sudo).
# Usage (from the repo folder):
#   bash scripts/install-desktop.sh            # add the menu entry
#   bash scripts/install-desktop.sh --remove   # take it out again
set -euo pipefail
cd "$(dirname "$0")/.."
REPO="$PWD"
DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
FILE="$DIR/wingcut.desktop"

if [ "${1:-}" = "--remove" ]; then
  rm -f "$FILE"
  echo "Removed $FILE"
  exit 0
fi

# Build once if needed, so the menu entry starts instantly.
if [ ! -x target/release/wingcut-gui ]; then
  bash scripts/start.sh --help >/dev/null
fi

mkdir -p "$DIR"
cat > "$FILE" <<DESKTOP
[Desktop Entry]
Type=Application
Name=wingcut
Comment=Hot-wire foam wing cutter: design, preview and cut
Exec="$REPO/target/release/wingcut-gui" %f
Path=$REPO
Terminal=false
Categories=Graphics;Engineering;
MimeType=application/toml;
DESKTOP
chmod +x "$FILE"
if command -v update-desktop-database >/dev/null; then
  update-desktop-database "$DIR" >/dev/null 2>&1 || true
fi
echo "Added wingcut to your application menu ($FILE)."
echo "It starts the program built in $REPO; after 'bash scripts/try-local.sh' the menu entry uses the new version."
