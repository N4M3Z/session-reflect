#!/usr/bin/env bash
# Shared build-on-demand logic. Source this, don't execute directly.

PLUGIN_ROOT="${CLAUDE_PLUGIN_ROOT:-$(builtin cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
BIN_DIR="$PLUGIN_ROOT/target/release"

ensure_built() {
  local binary="$1"
  if [ -x "$BIN_DIR/$binary" ]; then return 0; fi

  local CARGO=""
  if command -v cargo >/dev/null 2>&1; then
    CARGO=cargo
  else
    for candidate in "$HOME/.cargo/bin/cargo" /opt/homebrew/bin/cargo /usr/local/bin/cargo; do
      if [ -x "$candidate" ]; then
        CARGO="$candidate"
        break
      fi
    done
  fi

  if [ -z "$CARGO" ]; then
    echo "session-reflect: cargo not found — install Rust: https://rustup.rs" >&2
    return 1
  fi

  "$CARGO" build --release --manifest-path "$PLUGIN_ROOT/Cargo.toml" >&2
}
