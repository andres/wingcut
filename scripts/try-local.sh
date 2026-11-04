#!/usr/bin/env bash
# Build, test and try wingcut on a local Linux desktop.
# Usage (from the repo folder):  bash scripts/try-local.sh
# Everything printed is also saved to ./try-local.log
set -euo pipefail
cd "$(dirname "$0")/.."
LOG="$PWD/try-local.log"
exec > >(tee "$LOG") 2>&1
step() { printf '\n== %s\n' "$*"; }

step "System"
uname -srm
echo "desktop session: ${XDG_SESSION_TYPE:-unknown} (DISPLAY=${DISPLAY:-none}, WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-none})"

step "Rust toolchain"
if ! command -v cargo >/dev/null && [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi
if ! command -v cargo >/dev/null; then
  echo "Rust is not installed. rustup installs it for your user only (in ~/.cargo, no sudo)."
  read -r -p "Install Rust now? [y/N] " answer
  if [[ $answer == [yY]* ]]; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
  else
    echo "Stopped: Rust is needed to build wingcut."
    exit 1
  fi
fi
cargo --version
if ! command -v cc >/dev/null; then
  echo "Stopped: no C linker found. Install one, then run this script again:"
  echo "  Debian/Ubuntu/Mint: sudo apt install build-essential"
  echo "  Fedora:             sudo dnf install gcc"
  echo "  Arch:               sudo pacman -S base-devel"
  exit 1
fi

step "Update to the latest version"
git pull --ff-only || echo "(skipped: you have local changes or are not on a branch)"

step "Run the tests (1-3 minutes the first time)"
cargo test --workspace --quiet

step "Build the release binaries"
cargo build --release --workspace --quiet
ls -lh target/release/wingcut target/release/wingcut-gui

step "CLI: generate the example 4 m wing into ./out"
./target/release/wingcut gen --machine examples/machine.toml --wing examples/wing.toml -o out
ls out

step "GUI: opening the example wing (close the window to finish)"
echo "Tip: use File > Save wing as... for experiments, so examples/wing.toml stays clean."
./target/release/wingcut-gui examples/wing.toml || echo "The GUI exited with an error (see above)."

step "Done. Log saved to $LOG"
