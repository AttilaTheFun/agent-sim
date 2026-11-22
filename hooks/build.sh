#!/usr/bin/env bash
# Build the hooks agent-sim loads into the phone's processes, on a Mac with Xcode's command line tools (no iOS SDK
# needed): compiled for arm64 macOS, retagged as iOS simulator, ad-hoc signed. Output: hooks/prebuilt/.
#
#   hooks/build.sh
set -Eeuo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
[ "$(uname -s)" = Darwin ] || { echo "hooks/build.sh: needs macOS (clang with the macOS SDK)" >&2; exit 1; }
SDK=${SDKROOT:-$(xcrun --sdk macosx --show-sdk-path)}
mkdir -p prebuilt
for src in springboard.c ax.c; do
	out=prebuilt/libagentsim-${src%.c}.dylib
	clang -arch arm64 -isysroot "$SDK" -mmacosx-version-min=14.0 -O2 -Wall -fblocks \
		-dynamiclib -install_name "@rpath/${out##*/}" -lobjc -o "$out" "$src"
	python3 retag.py "$out"
	codesign -f -s - "$out" 2>/dev/null
	echo "built $out"
done
