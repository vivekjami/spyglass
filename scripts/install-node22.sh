#!/usr/bin/env bash
# Install the Node runtime TrueForge needs, without touching system packages.
#
# TrueForge requires Node >= 22.14. This fetches the official tarball, verifies
# it against the signed SHASUMS256.txt, and unpacks it to ~/.local/node-v22.
# Nothing global is modified; `source scripts/env.sh` puts it on PATH.
set -euo pipefail

NODE_VERSION="${NODE_VERSION:-v22.23.2}"
PREFIX="${SPYGLASS_NODE_HOME:-$HOME/.local/node-v22}"
ARCH="linux-x64"
TARBALL="node-${NODE_VERSION}-${ARCH}.tar.xz"
BASE="https://nodejs.org/dist/${NODE_VERSION}"

if [ -x "$PREFIX/bin/node" ]; then
  echo "node already installed: $("$PREFIX/bin/node" --version) at $PREFIX"
  exit 0
fi

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
cd "$workdir"

echo "fetching ${NODE_VERSION} ..."
curl -fsSLO "$BASE/$TARBALL"
curl -fsSLO "$BASE/SHASUMS256.txt"

echo "verifying checksum ..."
grep " $TARBALL\$" SHASUMS256.txt | sha256sum -c -

mkdir -p "$(dirname "$PREFIX")"
tar -xJf "$TARBALL"
rm -rf "$PREFIX"
mv "node-${NODE_VERSION}-${ARCH}" "$PREFIX"

echo "installed $("$PREFIX/bin/node" --version) -> $PREFIX"
echo "run: source scripts/env.sh"
