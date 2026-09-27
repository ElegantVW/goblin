#!/usr/bin/env bash
# build.sh — produce release goblin; install into faeOS engine paths
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"
cargo build --release
BIN="$ROOT/target/release/goblin"
echo "built: $BIN"
ls -la "$BIN"

if [[ "${1:-}" == "install" ]]; then
  LIB="$HOME/.local/lib/faeos"
  WRAP_SRC="$ROOT/scripts/goblin"
  mkdir -p "$LIB" "$HOME/bin"
  cp -f "$BIN" "$LIB/goblin"
  chmod +x "$LIB/goblin"

  install_launcher() {
    local dest="$1"
    mkdir -p "$(dirname "$dest")"
    cp -f "$WRAP_SRC" "$dest"
    chmod +x "$dest"
  }

  install_launcher "$HOME/bin/goblin"
  # The kit directory is `~/faeOS` with a capital S. Checking only the
  # lowercase `~/faeos` meant the launcher never reached the kit on this
  # machine, silently. bulwark/build.sh already checks both; do the same.
  for kit in "$HOME/faeOS/bin" "$HOME/faeos/bin"; do
    if [[ -d "$kit" ]]; then
      install_launcher "$kit/goblin"
    fi
  done

  echo "installed engine → $LIB/goblin"
  echo "launcher        → $HOME/bin/goblin"
fi
