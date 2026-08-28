#!/usr/bin/env bash
# Install TrueForge's *local* sandbox dependencies, without root.
#
# Why this exists (Phase 0, item 3):
#   trueforge.dev documents Daytona as "the only sandbox provider supported
#   today" — a cloud sandbox, which almost certainly cannot reach a Docker
#   Compose network running on this host. That would have cost Spyglass its
#   sandbox causal-verification step (ADR-010) and forced the bisection
#   fallback.
#
#   But TrueForge bundles @anthropic-ai/sandbox-runtime and logs at startup:
#     "Local sandbox fallback is unavailable {reason: SRT host dependencies
#      missing (linux: bwrap, socat, rg)}"
#   Supply those three and the sandbox runs locally — same host, same network
#   namespace reach as any other local process. That is strictly better than
#   Daytona for our purposes and needs no cloud account.
#
#   bwrap ships with Ubuntu. socat and rg do not, and apt needs root — so we
#   fetch a prebuilt static rg and build socat from source into ~/.local.
set -euo pipefail

PREFIX="$HOME/.local"
mkdir -p "$PREFIX/bin"

need() { ! command -v "$1" >/dev/null 2>&1; }

if ! command -v bwrap >/dev/null 2>&1; then
  echo "error: bwrap (bubblewrap) not found and cannot be installed without root." >&2
  echo "       install it with: sudo apt install bubblewrap" >&2
  exit 1
fi
echo "bwrap: $(command -v bwrap)"

if need rg && [ ! -x "$PREFIX/bin/rg" ]; then
  echo "installing ripgrep ..."
  RG_VER="14.1.1"
  RG="ripgrep-${RG_VER}-x86_64-unknown-linux-musl"
  tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
  curl -fsSL "https://github.com/BurntSushi/ripgrep/releases/download/${RG_VER}/${RG}.tar.gz" \
    | tar -xz -C "$tmp"
  install -m755 "$tmp/$RG/rg" "$PREFIX/bin/rg"
fi
echo "rg: $("$PREFIX/bin/rg" --version 2>/dev/null | head -1 || rg --version | head -1)"

if need socat && [ ! -x "$PREFIX/bin/socat" ]; then
  echo "building socat from source ..."
  SOCAT_VER="1.8.0.3"
  tmp2="$(mktemp -d)"; trap 'rm -rf "$tmp2"' EXIT
  curl -fsSL "http://www.dest-unreach.org/socat/download/socat-${SOCAT_VER}.tar.gz" \
    | tar -xz -C "$tmp2"
  ( cd "$tmp2/socat-${SOCAT_VER}" \
      && ./configure --prefix="$PREFIX" >/dev/null \
      && make -j"$(nproc)" >/dev/null \
      && install -m755 socat "$PREFIX/bin/socat" )
fi
echo "socat: $("$PREFIX/bin/socat" -V 2>/dev/null | sed -n 2p || socat -V | sed -n 2p)"

echo
echo "sandbox deps ready. restart trueforge; the startup warning should be gone."
