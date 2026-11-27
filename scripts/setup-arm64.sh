#!/usr/bin/env bash
# Turn a fresh Ubuntu 24.04 or 26.04 arm64 machine (or container) into one that boots Apple's iOS simulator runtime natively,
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
#   DARLING_CHECK   0: do not start Darling once to check it (building an image)
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
# fetch URL SHA256 FILE: a download, kept only if it is the one pinned here (nimbus-base's darwin layer builds from this:
# every download a fixed version, checked)
fetch() {
	curl -fsSL -o "$3" "$1"
	echo "$2  $3" | sha256sum -c --quiet - || { rm -f "$3"; die "$1 is not the file pinned here"; }
}

# Pinned downloads (Linux arm64)
RUST_VERSION=1.100.0
RUSTUP_INIT=https://static.rust-lang.org/rustup/archive/1.29.3/aarch64-unknown-linux-gnu/rustup-init
RUSTUP_INIT_SHA256=9f608c142e0e38a78957d0661ba1dfaf0b6fb37184cfaa4b9a0aa57af6184282
SEVENZIP=https://www.7-zip.org/a/7z2501-linux-arm64.tar.xz
SEVENZIP_SHA256=39c5140f02ce4436599303c59a149f654cb1bbc47cdc105a942120d747ae040d
IPSW=https://github.com/blacktop/ipsw/releases/download/v3.1.752/ipsw_3.1.752_linux_arm64.tar.gz
IPSW_SHA256=306c20b31d7478b7154cd27209603fbd4fb0bab28025226fa946f66ac1a3376d
BAZELISK=https://github.com/bazelbuild/bazelisk/releases/download/v1.30.0/bazelisk-linux-arm64
BAZELISK_SHA256=b2bcc040c213b44586b121bda6d983c35df121b917c3a7a66fb16f2fa0df1a4f

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
	mesa-vulkan-drivers python3-pil cpio unzip zstd
# x86Sim's test runner thins Xcode's universal XCTest frameworks with an unversioned llvm-lipo, which Ubuntu's llvm
# packages leave out
command -v llvm-lipo >/dev/null || ln -sf "$(ls /usr/bin/llvm-lipo-* | sort -V | tail -1)" /usr/local/bin/llvm-lipo

log "Darling $(base darling | cut -c1-12) with patches/darling-arm64"
D=$WORK/darling
patched=$(cat "$P"/*.patch "$P/BASE.txt" | sha256sum | cut -c1-64)
# Darling installed with these patches (its prefix records them) needs nothing more, not even its sources: the image
# of Nimbus' ios leases (images/arm64) has no sources
INSTALLED=/usr/local/libexec/darling/.agent-sim-patched
if [ -x /usr/local/bin/darling ] && [ "$(cat "$INSTALLED" 2>/dev/null)" = "$patched" ]; then
	echo "installed, with these patches"
else
	if [ ! -d "$D/.git" ]; then
		git clone -q https://github.com/darlinghq/darling.git "$D"
		git -C "$D" checkout -q "$(base darling)"
		git -C "$D" submodule update -q --init --recursive --jobs 16
	fi
	# ICU's data (and other large files) are Git LFS objects: without git-lfs the checkout holds their pointers
	git lfs install > /dev/null
	git -C "$D" submodule foreach -q --recursive 'git lfs pull > /dev/null 2>&1 || true'
	# The patches applied are recorded by their hash: a checkout with other patches (a lease of an image made from an
	# older commit) puts the tree back to BASE.txt's commits, applies its own and builds again
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
		# twice: Ruby's bundles link against CFNetwork, which the build does not wait for (the second run finishes them).
		# With no swiftc on PATH: Darling's CMake builds its Swift CryptoKit when it finds one, which Swift 6.4 fails
		# to compile (nimbus-base's toolchain layer puts Swift on PATH)
		mkdir -p "$D/build"
		NO_SWIFT_PATH=$(printf '%s\n' "$PATH" | tr ':' '\n' | while read -r d; do [ -x "$d/swiftc" ] || printf '%s:' "$d"; done)
		(export PATH="${NO_SWIFT_PATH%:}" && cd "$D/build" && cmake .. -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ \
			-DTARGET_ARM64=ON > /dev/null &&
			{ ninja -j "$JOBS" -k 0 || ninja -j "$JOBS"; } && ninja install > /dev/null)
	fi
	echo "$patched" > "$INSTALLED"
fi
# (not while building an image: its builder may not allow the namespaces Darling needs; the phones' boot checks it)
[ "${DARLING_CHECK:-1}" = 0 ] || darling shell uname -m > /dev/null || die "darling shell does not start"

log "x86Sim $(base x86sim/x86sim | cut -c1-12): device setup, in-simulator helpers, Metal stand-in"
X=$WORK/x86sim
if [ ! -d "$X/.git" ]; then
	git clone -q https://github.com/x86sim/x86sim.git "$X"
	git -C "$X" checkout -q "$(base x86sim/x86sim)"
fi
# agent-sim's fixes to x86Sim's own scripts for the arm64 boot (patches/x86sim-arm64), each applied once
for p in "$HERE/../patches/x86sim-arm64"/*.patch; do
	[ -e "$p" ] || continue
	git -C "$X" apply --reverse --check "$p" 2>/dev/null || git -C "$X" apply "$p" || die "$(basename "$p") does not apply to x86Sim"
done
if [ ! -d "$X/external/metal2vulkan/crates" ]; then
	git clone -q -b x86sim https://github.com/x86sim/metal2vulkan.git "$WORK/metal2vulkan"
	git -C "$WORK/metal2vulkan" checkout -q "$(x86base x86sim/metal2vulkan)"
	rmdir "$X/external/metal2vulkan" 2>/dev/null || true
	ln -sfn "$WORK/metal2vulkan" "$X/external/metal2vulkan"
fi
if [ ! -x "$X/linux/mtlvk/target/release/mtlvk" ]; then
	command -v cargo > /dev/null || [ -x "$HOME/.cargo/bin/cargo" ] || {
		fetch "$RUSTUP_INIT" "$RUSTUP_INIT_SHA256" "$WORK/rustup-init"
		chmod 755 "$WORK/rustup-init" && "$WORK/rustup-init" -y -q --profile minimal --default-toolchain "$RUST_VERSION"
		rm -f "$WORK/rustup-init"
	}
	(. "$HOME/.cargo/env" && cd "$X/linux/mtlvk" && cargo build -q --release)
fi
[ -x "$X/host/simhost/simhost-arm64" ] ||
	DARLING_SRC=$D "$HERE/build-simhost-arm64.sh" "$X/host/simhost/simhost.c" "$X/host/simhost/simhost-arm64"

log "iOS runtime $RUNTIME_BUILD from Apple"
if [ "$RUNTIME_BUILD" = none ]; then
	echo "skipped (RUNTIME_BUILD=none)"
elif [ ! -d "$(readlink -f "$X/runtimes/"*"-$RUNTIME_BUILD-arm64/RuntimeRoot" 2>/dev/null || echo /nonexistent)" ]; then
	command -v 7z > /dev/null || {
		fetch "$SEVENZIP" "$SEVENZIP_SHA256" "$WORK/7z.tar.xz"
		mkdir -p "$WORK/7z" && tar xf "$WORK/7z.tar.xz" -C "$WORK/7z" && ln -sf "$WORK/7z/7zz" /usr/local/bin/7z
	}
	command -v ipsw > /dev/null || {
		fetch "$IPSW" "$IPSW_SHA256" "$WORK/ipsw.tar.gz"
		mkdir -p "$WORK/ipsw" && tar xzf "$WORK/ipsw.tar.gz" -C "$WORK/ipsw" && install -m755 "$WORK/ipsw/ipsw" /usr/local/bin/ipsw
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
# xcrun simctl for scripts written for a Mac (agent-sim simctl)
ln -sf /usr/local/bin/agent-sim /usr/local/bin/xcrun
# launchctl procinfo, whether iOS suspended a phone's process (Isomer's iOS launcher asks it to wake an app)
ln -sf /usr/local/bin/agent-sim /usr/local/bin/launchctl
# Bazel (bazelisk, which fetches the version a workspace's .bazelversion names), for `bazel test` of iOS test targets,
# unless the machine has one (nimbus-base's build layer does, the same pin)
command -v bazel > /dev/null || {
	fetch "$BAZELISK" "$BAZELISK_SHA256" /usr/local/bin/bazel
	chmod 755 /usr/local/bin/bazel
}

log "Done: boot a phone with X86SIM_ROOT=$X $HERE/boot-arm64.sh"
