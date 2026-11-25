#!/usr/bin/env python3
"""Extract a platform's Developer directory (XCTest and its agents) from an Xcode .xip on Linux, without unpacking all
of Xcode:

    xip-platform.py Xcode.xip OUTDIR [Platform ...]      (default iPhoneSimulator)

writes OUTDIR/<Platform>.platform/Developer/... A .xip is a XAR archive whose "Content" member is a pbzx stream
(chunks, each xz-compressed or stored) of a cpio archive of Xcode.app. 7-Zip (7z or 7zz) reads the XAR (told it is one:
otherwise it opens "Content" as a nested archive); this decodes the pbzx stream into cpio, which extracts only the
platforms' Developer directories. About a minute for Xcode 27.2's 2 GB .xip."""
import lzma
import os
import shutil
import struct
import subprocess
import sys

XZ_MAGIC = b"\xfd7zXZ\x00"


def pbzx_to(src, dst):
    if src.read(4) != b"pbzx":
        sys.exit("xip-platform: Content is not a pbzx stream")
    flags = struct.unpack(">Q", src.read(8))[0]
    while flags & 0x01000000:
        head = src.read(16)
        if len(head) < 16:
            break
        flags, length = struct.unpack(">QQ", head)
        chunk = src.read(length)
        if len(chunk) != length:
            sys.exit("xip-platform: truncated pbzx chunk")
        dst.write(lzma.decompress(chunk) if chunk.startswith(XZ_MAGIC) else chunk)


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    xip, out = sys.argv[1], os.path.abspath(sys.argv[2])
    platforms = sys.argv[3:] or ["iPhoneSimulator"]
    seven = shutil.which("7zz") or shutil.which("7z")
    if not seven:
        sys.exit("xip-platform: needs 7-Zip (7zz or 7z)")
    staging = out + ".staging"
    os.makedirs(staging, exist_ok=True)
    patterns = ["*Contents/Developer/Platforms/%s.platform/Developer/*" % p for p in platforms]
    xar = subprocess.Popen([seven, "x", "-txar", "-so", xip, "Content"], stdout=subprocess.PIPE,
                           stderr=subprocess.DEVNULL)
    cpio = subprocess.Popen(["cpio", "-idm", "--quiet", "--no-absolute-filenames"] + patterns, stdin=subprocess.PIPE,
                            cwd=staging)
    pbzx_to(xar.stdout, cpio.stdin)
    cpio.stdin.close()
    if cpio.wait() != 0 or xar.wait() != 0:
        sys.exit("xip-platform: extraction failed (7z %s, cpio %s)" % (xar.returncode, cpio.returncode))
    os.makedirs(out, exist_ok=True)
    for p in platforms:
        found = None
        for dp, dns, _ in os.walk(staging):
            if os.path.basename(dp) == "Platforms" and p + ".platform" in dns:
                found = os.path.join(dp, p + ".platform")
                break
        if not found:
            sys.exit("xip-platform: %s.platform/Developer is not in %s" % (p, xip))
        dest = os.path.join(out, p + ".platform")
        if os.path.exists(dest):
            shutil.rmtree(dest)
        shutil.move(found, dest)
        print(dest)
    shutil.rmtree(staging)


if __name__ == "__main__":
    main()
