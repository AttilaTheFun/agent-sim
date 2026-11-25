#!/bin/bash
# Builds hello (iOS simulator, arm64) with Darling's own ld64. No Apple SDK or binaries.
set -ex
cd /root/hello
LD=/root/darling/build/src/external/cctools-port/cctools/ld64/src/
T=arm64-apple-ios17.0-simulator
CF="-target $T --ld-path=${LD}aarch64-apple-darwin20-ld -nostdlib -O1"
clang $CF -dynamiclib -install_name /usr/lib/libSystem.B.dylib -o libSystem.B.dylib libsystem_stub.c
clang $CF -o hello hello.c libSystem.B.dylib
file hello
/root/darling/build/src/external/cctools-port/cctools/otool/otool -l hello 2>/dev/null | grep -A4 -E "LC_BUILD_VERSION|LC_LOAD_DYLIB|LC_MAIN" || true
