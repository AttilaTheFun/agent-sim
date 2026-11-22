#!/usr/bin/env bash
# Install agent-sim on an x86-64 Linux machine:
#
#   curl -fsSL https://raw.githubusercontent.com/Developing-Gamer/agent-sim/main/install.sh | bash
#
# Puts the agent-sim command in ~/.local/bin and its scripts and hooks in ~/.local/share/agent-sim/src. Then run
# `agent-sim setup` once (x86Sim, Apple's iOS runtime, a first phone; about half an hour), or set
# AGENT_SIM_SETUP=1 to run it now.
#
# Environment: AGENT_SIM_VERSION (a release tag, default the latest), AGENT_SIM_HOME, AGENT_SIM_BIN,
# AGENT_SIM_TARBALL (a local release tarball instead of downloading one).
set -Eeuo pipefail

REPO=Developing-Gamer/agent-sim
HOME_DIR=${AGENT_SIM_HOME:-$HOME/.local/share/agent-sim}
BIN=${AGENT_SIM_BIN:-$HOME/.local/bin}
say() { printf '==> %s\n' "$*"; }
die() { printf 'agent-sim install: %s\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = Linux ] || die "agent-sim runs on Linux (x86-64); on a Mac, use Apple's own simulator"
[ "$(uname -m)" = x86_64 ] || die "agent-sim needs an x86-64 machine (found $(uname -m))"
command -v curl >/dev/null || die "curl is needed"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
if [ -n "${AGENT_SIM_TARBALL:-}" ]; then
	cp "$AGENT_SIM_TARBALL" "$tmp/agent-sim.tar.gz"
else
	if [ -z "${AGENT_SIM_VERSION:-}" ] || [ "$AGENT_SIM_VERSION" = latest ]; then
		url="https://github.com/$REPO/releases/latest/download/agent-sim-x86_64-linux.tar.gz"
	else
		url="https://github.com/$REPO/releases/download/$AGENT_SIM_VERSION/agent-sim-x86_64-linux.tar.gz"
	fi
	say "Downloading $url"
	curl -fL --progress-bar -o "$tmp/agent-sim.tar.gz" "$url" || die "download failed"
	if curl -fsL -o "$tmp/sum" "$url.sha256"; then
		(cd "$tmp" && echo "$(cut -d' ' -f1 sum)  agent-sim.tar.gz" | sha256sum -c --quiet) || die "checksum mismatch"
	fi
fi

say "Installing into $HOME_DIR/src"
mkdir -p "$HOME_DIR" "$BIN"
rm -rf "$HOME_DIR/src.new"
mkdir -p "$HOME_DIR/src.new"
tar -xzf "$tmp/agent-sim.tar.gz" -C "$HOME_DIR/src.new" --strip-components=1
rm -rf "$HOME_DIR/src"
mv "$HOME_DIR/src.new" "$HOME_DIR/src"
ln -sf "$HOME_DIR/src/bin/agent-sim" "$BIN/agent-sim"
"$BIN/agent-sim" --version

case ":$PATH:" in *":$BIN:"*) ;; *) say "Add $BIN to your PATH: export PATH=\"$BIN:\$PATH\"" ;; esac
if [ "${AGENT_SIM_SETUP:-0}" = 1 ]; then
	"$BIN/agent-sim" setup
else
	say "Next: agent-sim setup     (once: x86Sim, Apple's iOS runtime and a first phone)"
	say "Then: agent-sim up && agent-sim snapshot"
fi
