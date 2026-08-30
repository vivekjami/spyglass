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

# One scratch dir for every download; one EXIT trap removes it.
work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT

if need rg && [ ! -x "$PREFIX/bin/rg" ]; then
  echo "installing ripgrep ..."
  RG_VER="14.1.1"
  RG="ripgrep-${RG_VER}-x86_64-unknown-linux-musl"
  tmp="$work/rg"; mkdir -p "$tmp"
  curl -fsSL "https://github.com/BurntSushi/ripgrep/releases/download/${RG_VER}/${RG}.tar.gz" \
    | tar -xz -C "$tmp"
  install -m755 "$tmp/$RG/rg" "$PREFIX/bin/rg"
fi
echo "rg: $("$PREFIX/bin/rg" --version 2>/dev/null | head -1 || rg --version | head -1)"

if need socat && [ ! -x "$PREFIX/bin/socat" ]; then
  echo "building socat from source ..."
  # dest-unreach.org serves the release over plain HTTP (its HTTPS certificate is
  # self-signed), so the tarball is verified against a pinned sha256 -- the one
  # Homebrew and Alpine publish for this release, checked against both mirrors.
  SOCAT_VER="1.8.1.3"
  SOCAT_SHA256="06602ffd591e98c75b3dc1d66f0f19136cc666b0b2d95caad987d6ab2cb28097"
  tmp2="$work/socat"; mkdir -p "$tmp2"
  curl -fsSL -o "$tmp2/socat.tar.gz" "http://www.dest-unreach.org/socat/download/socat-${SOCAT_VER}.tar.gz"
  echo "${SOCAT_SHA256}  $tmp2/socat.tar.gz" | sha256sum -c - >/dev/null \
    || { echo "error: socat-${SOCAT_VER}.tar.gz does not match the pinned sha256" >&2; exit 1; }
  tar -xz -C "$tmp2" -f "$tmp2/socat.tar.gz"
  ( cd "$tmp2/socat-${SOCAT_VER}" \
      && ./configure --prefix="$PREFIX" >/dev/null \
      && make -j"$(nproc)" >/dev/null \
      && install -m755 socat "$PREFIX/bin/socat" )
fi
echo "socat: $("$PREFIX/bin/socat" -V 2>/dev/null | sed -n 2p || socat -V | sed -n 2p)"

echo
# TrueForge runs the sandbox's proxy bridge (`socat TCP-LISTEN:3128 ... &`) INSIDE
# the sandbox, whose read policy allows only /usr, /bin, /lib, /etc, ... -- not
# $HOME. A socat that lives in ~/.local/bin satisfies the harness's start-up
# dependency check and then cannot be executed by the bridge, and every sandbox
# command fails at bootstrap with "pip install pydantic ... Cannot connect to
# proxy" (Phase 11 F1). The one step that needs root:
if [ -x /usr/local/bin/socat ] || [ -x /usr/bin/socat ]; then
  echo "socat is on a sandbox-readable path ($(command -v socat)) -- the bridge can run."
else
  echo "ONE ROOT STEP REMAINS -- TrueForge's sandbox bridge must find socat under /usr/local/bin or /usr/bin:"
  echo "    sudo install -m 0755 $PREFIX/bin/socat /usr/local/bin/socat"
  echo "  (or: sudo apt-get install -y socat). Without it the sandbox fails at bootstrap."
fi
echo "sandbox deps ready. restart trueforge; the startup warning should be gone."
