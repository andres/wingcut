#!/usr/bin/env bash
# Start the wingcut GUI. Builds it first only if it hasn't been built yet.
# Usage (from the repo folder):
#   bash scripts/start.sh                          # empty window
#   bash scripts/start.sh examples/wing-5m.toml    # open a wing
set -euo pipefail
cd "$(dirname "$0")/.."
APP=target/release/wingcut-gui
if [ ! -x "$APP" ]; then
  if ! command -v cargo >/dev/null && [ -f "$HOME/.cargo/env" ]; then
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
  fi
  command -v cargo >/dev/null || { echo "Rust is not installed: run 'bash scripts/try-local.sh' once first."; exit 1; }
  echo "First start: building wingcut (a few minutes)..."
  cargo build --release -p wingcut-gui --quiet
fi
exec "$APP" "$@"
