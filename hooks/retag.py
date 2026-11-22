#!/usr/bin/env python3
"""Mark an arm64 Mach-O built against the macOS SDK as an iOS-simulator binary (LC_BUILD_VERSION platform 7), so
the simulator's dyld loads it. The code only calls libSystem and libobjc, whose interfaces the two share.

    retag.py LIB.dylib [MINOS]   (default 17.0)
"""
import struct
import sys

LC_BUILD_VERSION = 0x32
PLATFORM_IOSSIMULATOR = 7


def version(s):
    major, minor = (s.split(".") + ["0"])[:2]
    return int(major) << 16 | int(minor) << 8


def main():
    path = sys.argv[1]
    minos = version(sys.argv[2] if len(sys.argv) > 2 else "17.0")
    data = bytearray(open(path, "rb").read())
    magic, _, _, _, ncmds, _, _ = struct.unpack_from("<IiiIIII", data, 0)
    if magic != 0xFEEDFACF:
        sys.exit(f"{path}: not a 64-bit little-endian Mach-O")
    off, found = 32, False
    for _ in range(ncmds):
        cmd, size = struct.unpack_from("<II", data, off)
        if cmd == LC_BUILD_VERSION:
            struct.pack_into("<III", data, off + 8, PLATFORM_IOSSIMULATOR, minos, minos)
            found = True
        off += size
    if not found:
        sys.exit(f"{path}: no LC_BUILD_VERSION")
    open(path, "wb").write(data)


if __name__ == "__main__":
    main()
