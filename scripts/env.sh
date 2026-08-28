#!/usr/bin/env bash
# Spyglass development environment.
#
# TrueForge requires Node >= 22.14 (package.json engines: node >=22). Ubuntu's
# packaged Node is 20.x, so Phase 0 installs an official Node 22 tarball into
# ~/.local/node-v22 rather than touching system packages or shell profiles.
#
# Usage:  source scripts/env.sh
#
# Everything here is reversible: `rm -rf ~/.local/node-v22` undoes the install.

SPYGLASS_NODE_HOME="${SPYGLASS_NODE_HOME:-$HOME/.local/node-v22}"

if [ ! -x "$SPYGLASS_NODE_HOME/bin/node" ]; then
  echo "error: node 22 not found at $SPYGLASS_NODE_HOME" >&2
  echo "       run scripts/install-node22.sh first" >&2
  return 1 2>/dev/null || exit 1
fi

case ":$PATH:" in
  *":$SPYGLASS_NODE_HOME/bin:"*) ;;
  *) PATH="$SPYGLASS_NODE_HOME/bin:$PATH" ;;
esac

# TrueForge's local sandbox (Anthropic sandbox-runtime) shells out to bwrap,
# socat and rg. Without all three it silently falls back to "no sandbox", which
# would cost us the causal-replay step entirely. socat/rg live in ~/.local/bin.
case ":$PATH:" in
  *":$HOME/.local/bin:"*) ;;
  *) PATH="$HOME/.local/bin:$PATH" ;;
esac
export PATH

# Repo root, resolved from this script's location so `source` works from anywhere.
SPYGLASS_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd)"
export SPYGLASS_ROOT

# Keep all TrueForge state inside the repo (gitignored) so a run is disposable
# and reproducible: `rm -rf .local` returns the harness to first-run state.
export SQLITE_PATH="${SQLITE_PATH:-$SPYGLASS_ROOT/.local/trueforge/db.sqlite}"
export TRUEFORGE_PORT="${TRUEFORGE_PORT:-8790}"
export TRUEFORGE_URL="${TRUEFORGE_URL:-http://localhost:$TRUEFORGE_PORT}"
mkdir -p "$(dirname "$SQLITE_PATH")"

echo "spyglass env ready: node $(node --version), trueforge -> $TRUEFORGE_URL"
echo "  SQLITE_PATH=$SQLITE_PATH"
