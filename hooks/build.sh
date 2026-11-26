#!/usr/bin/env bash
# Build the hooks agent-sim loads into the phone's processes: compiled for arm64 macOS, retagged as iOS simulator.
# Output: hooks/prebuilt/.
#
#   hooks/build.sh
#
# On Linux (an `ios` lease): Swift's clang (a recent one, for the SDK's headers) against the MacOSX SDK from Xcode's .xip
# (scripts/xip-platform.py Xcode.xip OUT MacOSX; MACOS_SDK=OUT/MacOSX.platform/Developer/SDKs/MacOSX.sdk), linked by
# LLVM 18's ld64.lld with the Objective-C runtime's symbols left for dyld to find (lld links neither for the iOS
# simulator nor against the SDK's arm64e.x1 stubs). Not signed: the simulator runtime under Darling doesn't check.
# On a Mac: Xcode's command line tools, ad-hoc signed.
set -Eeuo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
mkdir -p prebuilt
if [ "$(uname -s)" = Darwin ]; then
	SDK=${SDKROOT:-$(xcrun --sdk macosx --show-sdk-path)}
	CC=(clang -arch arm64 -mmacosx-version-min=14.0)
	LINK=(-lobjc)
else
	SDK=${MACOS_SDK:?set MACOS_SDK to a MacOSX.sdk (see the comment above)}
	CC=("${CLANG:-/opt/swift/usr/bin/clang}" -target arm64-apple-macos14.0)
	LINK=(-nostdlib -Wl,-undefined,dynamic_lookup -fuse-ld="${LD64_LLD:-/usr/lib/llvm-18/bin/ld64.lld}" -mlinker-version=705)
fi
for src in springboard.c ax.c; do
	out=prebuilt/libagentsim-${src%.c}.dylib
	"${CC[@]}" -isysroot "$SDK" -O2 -Wall -fblocks \
		-dynamiclib -install_name "@rpath/${out##*/}" "${LINK[@]}" -o "$out" "$src"
	python3 retag.py "$out"
	[ "$(uname -s)" != Darwin ] || codesign -f -s - "$out" 2>/dev/null
	echo "built $out"
done
