#!/usr/bin/env bash
# Turn a fresh Ubuntu 24.04 arm64 machine (or container) into one that boots Apple's iOS simulator runtime natively,
# with no translator: Darling built from darlinghq/darling plus patches/darling-arm64 (x86Sim's Darling changes merged
# with Darling's arm64 host support, and the fixes arm64 needed), x86Sim's device setup and in-simulator helpers, and
# x86Sim's Metal stand-in (mtlvk) built for arm64. Re-runnable: each step skips what is already done. Run as root
# (Darling's prefix and its namespaces), then boot with scripts/boot-arm64.sh. See docs/ARM64.md.
#
#   scripts/setup-arm64.sh
#
# Environment:
#   WORK            where Darling, x86Sim and metal2vulkan are checked out and built (default ~)
#   RUNTIME_BUILD   iOS runtime build from Apple's catalogue (default 24B8117, iOS 27.2); none: no runtime (an image
#                   for CI, whose jobs fetch the runtime themselves by running this again)
#   JOBS            parallel build jobs (default: the CPU count)
#   XCODE_XIP       an Xcode .xip (the one the test bundles were built against: Xcode 27.2, 27B52, for
#                   rules_applecross): its iPhoneSimulator XCTest frameworks go to $WORK/x86sim/sdks, for
#                   x86Sim's test runner (cli/libexec/xctest.py)
#
# Apple's iOS runtime is downloaded from Apple onto this machine (about 8 GB, 20 GB unpacked) and never leaves it.
set -Eeuo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
P=$HERE/../patches/darling-arm64
WORK=${WORK:-$HOME}
RUNTIME_BUILD=${RUNTIME_BUILD:-24B8117}
JOBS=${JOBS:-$(nproc)}
base() { awk -v k="$1" '$1 == k { print $2 }' "$P/BASE.txt"; }        # a commit from BASE.txt
x86base() { awk -v k="$1" '$1 == k { print $3 }' "$P/BASE.txt"; }     # the x86sim/... line's commit

log() { printf '\n==> %s\n' "$*"; }
die() { printf 'setup-arm64.sh: %s\n' "$*" >&2; exit 1; }

log "Checking the machine"
[ "$(uname -m)" = aarch64 ] || die "this is the arm64 setup; on x86-64 use scripts/setup-vm.sh"
[ "$(id -u)" = 0 ] || die "run as root (Darling's prefix and its namespaces)"

log "Packages"
export DEBIAN_FRONTEND=noninteractive
apt-get update -q
apt-get install -y -q --no-install-recommends ca-certificates git git-lfs curl xz-utils file python3 iproute2 \
	cmake ninja-build automake autoconf bison flex pkg-config clang lld llvm-dev libc++-dev \
	libfuse-dev libudev-dev libcairo2-dev libgl1-mesa-dev libglu1-mesa-dev libtiff5-dev libfreetype6-dev \
	libelf-dev libxml2-dev libegl1-mesa-dev libfontconfig1-dev libbsd-dev libxrandr-dev libxcursor-dev \
	libgif-dev libavutil-dev libpulse-dev libavformat-dev libavcodec-dev libswresample-dev libdbus-1-dev \
	libxkbfile-dev libssl-dev libstdc++-12-dev libcap2-bin libvulkan-dev libxkbcommon-dev libcap-dev \
	libbz2-dev zlib1g-dev libxi-dev libxext-dev libxfixes-dev libxrender-dev libxtst-dev \
	mesa-vulkan-drivers python3-pil cpio unzip
# x86Sim's test runner thins Xcode's universal XCTest frameworks with an unversioned llvm-lipo, which Ubuntu's llvm
# packages leave out
command -v llvm-lipo >/dev/null || ln -sf "$(ls /usr/bin/llvm-lipo-* | sort -V | tail -1)" /usr/local/bin/llvm-lipo

log "Darling $(base darling | cut -c1-12) with patches/darling-arm64"
D=$WORK/darling
if [ ! -d "$D/.git" ]; then
	git clone -q https://github.com/darlinghq/darling.git "$D"
	git -C "$D" checkout -q "$(base darling)"
	git -C "$D" submodule update -q --init --recursive --jobs 16
fi
# ICU's data (and other large files) are Git LFS objects: without git-lfs the checkout holds their pointers
git lfs install > /dev/null
git -C "$D" submodule foreach -q --recursive 'git lfs pull > /dev/null 2>&1 || true'
# The patches applied are recorded by their hash: a checkout with other patches (a CI job on an image made from an
# older commit) puts the tree back to BASE.txt's commits, applies its own and builds again
patched=$(cat "$P"/*.patch "$P/BASE.txt" | sha256sum | cut -c1-64)
if [ "$(cat "$D/.agent-sim-patched" 2>/dev/null)" != "$patched" ]; then
	git -C "$D" checkout -q -f "$(base darling)"
	git -C "$D" clean -q -fd -e /build -e /.agent-sim-patched
	git -C "$D" apply "$P/darling.patch"
	for sub in xnu darlingserver libkqueue iokitd libplatform libpthread dyld libunwind objc4 sqlite libnetwork; do
		[ -s "$P/$sub.patch" ] || continue
		git -C "$D/src/external/$sub" checkout -q -f "$(base "$sub")"
		git -C "$D/src/external/$sub" clean -q -fd
		git -C "$D/src/external/$sub" apply "$P/$sub.patch"
	done
	echo "$patched" > "$D/.agent-sim-patched"
fi
if [ ! -x /usr/local/bin/darling ] || [ "$D/.agent-sim-patched" -nt /usr/local/bin/darling ]; then
	# twice: Ruby's bundles link against CFNetwork, which the build does not wait for (the second run finishes them)
	mkdir -p "$D/build"
	(cd "$D/build" && cmake .. -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ \
		-DTARGET_ARM64=ON > /dev/null &&
		{ ninja -j "$JOBS" -k 0 || ninja -j "$JOBS"; } && ninja install > /dev/null)
fi
darling shell uname -m > /dev/null || die "darling shell does not start"

log "x86Sim $(base x86sim/x86sim | cut -c1-12): device setup, in-simulator helpers, Metal stand-in"
X=$WORK/x86sim
if [ ! -d "$X/.git" ]; then
	git clone -q https://github.com/x86sim/x86sim.git "$X"
	git -C "$X" checkout -q "$(base x86sim/x86sim)"
fi
if [ ! -d "$X/external/metal2vulkan/crates" ]; then
	git clone -q -b x86sim https://github.com/x86sim/metal2vulkan.git "$WORK/metal2vulkan"
	git -C "$WORK/metal2vulkan" checkout -q "$(x86base x86sim/metal2vulkan)"
	rmdir "$X/external/metal2vulkan" 2>/dev/null || true
	ln -sfn "$WORK/metal2vulkan" "$X/external/metal2vulkan"
fi
if [ ! -x "$X/linux/mtlvk/target/release/mtlvk" ]; then
	command -v cargo > /dev/null || [ -x "$HOME/.cargo/bin/cargo" ] || curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
	(. "$HOME/.cargo/env" && cd "$X/linux/mtlvk" && cargo build -q --release)
fi
[ -x "$X/host/simhost/simhost-arm64" ] ||
	DARLING_SRC=$D "$HERE/build-simhost-arm64.sh" "$X/host/simhost/simhost.c" "$X/host/simhost/simhost-arm64"

log "iOS runtime $RUNTIME_BUILD from Apple"
if [ "$RUNTIME_BUILD" = none ]; then
	echo "skipped (RUNTIME_BUILD=none)"
elif [ ! -d "$(readlink -f "$X/runtimes/"*"-$RUNTIME_BUILD-arm64/RuntimeRoot" 2>/dev/null || echo /nonexistent)" ]; then
	command -v 7z > /dev/null || {
		curl -fsSL -o "$WORK/7z.tar.xz" https://www.7-zip.org/a/7z2501-linux-arm64.tar.xz
		mkdir -p "$WORK/7z" && tar xf "$WORK/7z.tar.xz" -C "$WORK/7z" && ln -sf "$WORK/7z/7zz" /usr/local/bin/7z
	}
	command -v ipsw > /dev/null || {
		url=$(curl -fsSL https://api.github.com/repos/blacktop/ipsw/releases/latest | python3 -c 'import json, sys
print(next(a["browser_download_url"] for a in json.load(sys.stdin)["assets"]
           if a["name"].startswith("ipsw_") and "linux_arm64" in a["name"] and a["name"].endswith(".tar.gz")))')
		mkdir -p "$WORK/ipsw" && curl -fsSL "$url" | tar xz -C "$WORK/ipsw" && install -m755 "$WORK/ipsw/ipsw" /usr/local/bin/ipsw
	}
	(cd "$X" && python3 -I cli/libexec/arm64/runtime-fetch.py fetch "$RUNTIME_BUILD")
fi

if [ -n "${XCODE_XIP:-}" ]; then
	log "XCTest for the simulator from $(basename "$XCODE_XIP")"
	[ -d "$X/sdks/iPhoneSimulator.platform/Developer/Library/Frameworks/XCTest.framework" ] ||
		python3 -I "$HERE/xip-platform.py" "$XCODE_XIP" "$X/sdks" iPhoneSimulator
fi

log "agent-sim's command (its test runner for \`bazel test\`: bazel/) and Bazel"
(. "$HOME/.cargo/env" && cd "$HERE/.." && cargo build -q --release -p agent-sim)
# a link, so the command finds this checkout's scripts (agent-sim phones up boots with scripts/boot-arm64.sh)
ln -sf "$(cd "$HERE/.." && pwd)/target/release/agent-sim" /usr/local/bin/agent-sim
# Bazel (bazelisk, which fetches the version a workspace's .bazelversion names), for `bazel test` of iOS test targets
command -v bazel > /dev/null || {
	curl -fsSL -o /usr/local/bin/bazel https://github.com/bazelbuild/bazelisk/releases/latest/download/bazelisk-linux-arm64
	chmod 755 /usr/local/bin/bazel
}

log "Done: boot a phone with X86SIM_ROOT=$X $HERE/boot-arm64.sh"
