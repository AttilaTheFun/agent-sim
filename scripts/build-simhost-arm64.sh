#!/bin/bash
# Builds x86Sim's host/simhost/simhost.c as an arm64 macOS program for Darling, with the Darling build's own SDK
# and ld64 (DARLING_SRC: the Darling checkout and its build/ directory, default ~/darling).
set -e
SRC=${1:?usage: build-simhost-arm64.sh <simhost.c> <output>}
OUT=${2:?usage: build-simhost-arm64.sh <simhost.c> <output>}
SDK=${DARLING_SRC:-$HOME/darling}/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk
SDKB=${DARLING_SRC:-$HOME/darling}/build/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk
LD=${DARLING_SRC:-$HOME/darling}/build/src/external/cctools-port/cctools/ld64/src/aarch64-apple-darwin20-ld
clang -target arm64-apple-macos11 -nostdinc -isystem "$(clang -print-resource-dir)/include" -isystem $SDKB/usr/include -isystem $SDK/usr/include \
  -O1 -Wall -Wno-deprecated-declarations --ld-path=$LD -nostdlib -o "$OUT" "$SRC" \
  -L/usr/local/libexec/darling/usr/lib -lSystem -Wl,-syslibroot,/usr/local/libexec/darling
file "$OUT"
