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
  if [[ -d "$HOME/faeos/bin" ]]; then
    install_launcher "$HOME/faeos/bin/goblin"
  fi

  echo "installed engine → $LIB/goblin"
  echo "launcher        → $HOME/bin/goblin"
fi
