#!/usr/bin/env bash
# Install `just` (the task runner the README's commands assume) without root.
set -euo pipefail
PREFIX="$HOME/.local/bin"; mkdir -p "$PREFIX"
if command -v just >/dev/null 2>&1; then echo "just already installed: $(just --version)"; exit 0; fi
V="$(curl -fsSL https://api.github.com/repos/casey/just/releases/latest | python3 -c 'import sys,json;print(json.load(sys.stdin)["tag_name"])')"
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
curl -fsSL "https://github.com/casey/just/releases/download/$V/just-$V-x86_64-unknown-linux-musl.tar.gz" | tar -xz -C "$tmp" just
install -m755 "$tmp/just" "$PREFIX/just"
echo "installed $("$PREFIX/just" --version) -> $PREFIX (make sure ~/.local/bin is on PATH; scripts/env.sh does this)"
