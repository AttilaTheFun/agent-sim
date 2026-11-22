#!/usr/bin/env bash
# Rebuild x86Sim's ARM translator (libarm64jit.so) with the fixes in patches/x86sim and install it into the unpacked
# x86Sim. Re-runnable; the original library is kept next to it as libarm64jit.so.orig.
#
#   scripts/patch-x86sim.sh           build and install
#   scripts/patch-x86sim.sh restore   put x86Sim's own library back
#
# The patches change x86Sim source and are under x86Sim's license (GPL-3.0-or-later); they are meant for upstream.
# The build fetches x86Sim's tagged source and dynarmic from GitHub, needs about 2 GB and takes 5-10 minutes.
set -Eeuo pipefail

X86SIM_VERSION=${X86SIM_VERSION:-0.5.2}
ROOT=${AGENT_SIM_HOME:-$HOME/.local/share/agent-sim}
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
PATCHES=$HERE/../patches/x86sim
SRC=$ROOT/src/x86sim-$X86SIM_VERSION
BUILD=$ROOT/build/arm64jit-$X86SIM_VERSION
DATA=${XDG_DATA_HOME:-$HOME/.local/share}/x86sim
# the unpacked AppImage, and the copy x86Sim installs under its data directory on first run (the one phones load)
mapfile -t LIBS < <(find "$ROOT/x86sim-$X86SIM_VERSION" "$DATA"/app/"$X86SIM_VERSION"* -name libarm64jit.so \
	-path '*libexec/darling*' 2>/dev/null)
[ ${#LIBS[@]} -gt 0 ] || { echo "patch-x86sim.sh: x86Sim $X86SIM_VERSION is not installed; run scripts/setup-vm.sh" >&2; exit 1; }

if [ "${1:-}" = restore ]; then
	for lib in "${LIBS[@]}"; do [ -e "$lib.orig" ] && cp "$lib.orig" "$lib.new" && mv -f "$lib.new" "$lib"; done
	exit 0
fi

sudo DEBIAN_FRONTEND=noninteractive apt-get install -yqq clang cmake ninja-build git libboost-dev >/dev/null
if [ ! -d "$SRC/.git" ]; then
	git clone -q --depth 1 --branch "v$X86SIM_VERSION" https://github.com/x86sim/x86sim.git "$SRC"
	# the submodule URLs are SSH; the same repositories over HTTPS need no GitHub key
	git -C "$SRC" config submodule.external/dynarmic.url https://github.com/x86sim/dynarmic.git
	git -C "$SRC" submodule -q update --init --depth 1 external/dynarmic
	git -C "$SRC/external/dynarmic" submodule -q update --init --recursive --depth 1
fi
git -C "$SRC" checkout -q -- .
for p in "$PATCHES"/*.patch; do git -C "$SRC" apply "$p"; done
[ -f "$BUILD/build.ninja" ] || cmake -S "$SRC/linux/arm64jit" -B "$BUILD" -G Ninja -DCMAKE_BUILD_TYPE=Release \
	-DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ >/dev/null
nice ninja -C "$BUILD" -j"$(nproc)" arm64jit >/dev/null
# replaced by rename: running phones keep the library they loaded until they restart
for lib in "${LIBS[@]}"; do
	[ -e "$lib.orig" ] || cp "$lib" "$lib.orig"
	cp "$BUILD/libarm64jit.so" "$lib.new" && mv -f "$lib.new" "$lib"
	echo "patch-x86sim.sh: installed the patched translator in $lib"
done
