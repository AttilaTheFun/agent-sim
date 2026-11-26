#!/usr/bin/env python3
"""Extract a platform's Developer directory (XCTest and its agents, the platform's SDK) from an Xcode .xip on Linux,
without unpacking all of Xcode:

    xip-platform.py Xcode.xip OUTDIR [Platform ...]      (default iPhoneSimulator)

writes OUTDIR/<Platform>.platform/Developer/... A .xip is a XAR archive whose "Content" member is a pbzx stream (chunks,
each xz-compressed or stored) of a cpio archive (the portable "odc" format) of Xcode.app. 7-Zip (7z or 7zz) reads the
XAR (told it is one: otherwise it opens "Content" as a nested archive); this reads the pbzx stream and the cpio entries
itself, because Xcode's archive stores identical files once: the entries of a hard-linked file share (dev, ino), the
first holds the data, and each later one holds a placeholder of the same size ("NULLcanary", then zeros). A file whose
first copy lies outside the platforms (in another platform's SDK, say) is kept aside until its later links have been
read, and each later link becomes a hard link to the first copy. About a minute for Xcode 27.2's 2 GB .xip."""
import lzma
import os
import shutil
import stat
import struct
import subprocess
import sys

XZ_MAGIC = b"\xfd7zXZ\x00"
CANARY = b"NULLcanary"
ODC_HEADER = 76


def die(msg):
    sys.exit("xip-platform: " + msg)


def pbzx_chunks(src):
    if src.read(4) != b"pbzx":
        die("Content is not a pbzx stream")
    flags = struct.unpack(">Q", src.read(8))[0]
    while flags & 0x01000000:
        head = src.read(16)
        if len(head) < 16:
            return
        flags, length = struct.unpack(">QQ", head)
        chunk = src.read(length)
        if len(chunk) != length:
            die("truncated pbzx chunk")
        yield lzma.decompress(chunk) if chunk.startswith(XZ_MAGIC) else chunk


class Stream:
    """The cpio archive, read from the pbzx chunks as they come."""

    def __init__(self, chunks):
        self.chunks = chunks
        self.buffer = bytearray()

    def fill(self, size):
        while len(self.buffer) < size:
            chunk = next(self.chunks, None)
            if chunk is None:
                die("the cpio archive ends early")
            self.buffer += chunk

    def read(self, size):
        self.fill(size)
        data = bytes(self.buffer[:size])
        del self.buffer[:size]
        return data

    def copy(self, size, out):
        """size bytes to the file out, or dropped (out None)."""
        while size:
            if not self.buffer:
                self.fill(1)
            n = min(size, len(self.buffer))
            if out is not None:
                out.write(memoryview(self.buffer)[:n])
            del self.buffer[:n]
            size -= n


def entries(stream):
    """(path, mode, dev, ino, nlink, size) of each cpio entry; its data is read or skipped before the next."""
    while True:
        h = stream.read(ODC_HEADER)
        if h[:6] != b"070707":
            die("bad cpio header")
        dev, ino, mode = int(h[6:12], 8), int(h[12:18], 8), int(h[18:24], 8)
        nlink, name_size, size = int(h[36:42], 8), int(h[59:65], 8), int(h[65:76], 8)
        name = stream.read(name_size)
        path = name.rstrip(b"\0").decode("utf-8")
        if path == "TRAILER!!!":
            return
        yield path, mode, dev, ino, nlink, size


def write_first(stream, size, out, path):
    """A file's first copy: its data, never a placeholder."""
    head = stream.read(min(size, len(CANARY)))
    if size and head == CANARY[:size]:
        die("the first copy of %s is a hard link's placeholder" % path)
    with open(out, "wb") as w:
        w.write(head)
        stream.copy(size - len(head), w)


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    xip, out = sys.argv[1], os.path.abspath(sys.argv[2])
    platforms = sys.argv[3:] or ["iPhoneSimulator"]
    seven = shutil.which("7zz") or shutil.which("7z")
    if not seven:
        die("needs 7-Zip (7zz or 7z)")
    staging = out + ".staging"
    aside = out + ".hardlinks"
    for d in (staging, aside):
        shutil.rmtree(d, ignore_errors=True)
        os.makedirs(d)
    # each platform's Developer directory, wherever the archive's root puts Xcode.app
    wanted = ["Contents/Developer/Platforms/%s.platform/Developer/" % p for p in platforms]

    def selected(path):
        return any(w in path + "/" for w in wanted)

    xar = subprocess.Popen([seven, "x", "-txar", "-so", xip, "Content"], stdout=subprocess.PIPE,
                           stderr=subprocess.DEVNULL)
    stream = Stream(pbzx_chunks(xar.stdout))
    links = {}   # (dev, ino) -> the path holding the data
    for path, mode, dev, ino, nlink, size in entries(stream):
        kind = stat.S_IFMT(mode)
        key = (dev, ino)
        later = kind == stat.S_IFREG and key in links
        if not selected(path):
            if kind == stat.S_IFREG and not later and nlink > 1:
                links[key] = os.path.join(aside, "%d-%d" % key)
                write_first(stream, size, links[key], path)
            else:
                stream.copy(size, None)
            continue
        rel = os.path.normpath(path[2:] if path.startswith("./") else path.lstrip("/"))
        if rel.startswith(".."):
            die("archive path leaves the archive: " + path)
        dest = os.path.join(staging, rel)
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        if kind == stat.S_IFDIR:
            stream.copy(size, None)
            os.makedirs(dest, exist_ok=True)
        elif kind == stat.S_IFLNK:
            os.symlink(stream.read(size).decode("utf-8"), dest)
        elif kind == stat.S_IFREG:
            if later:
                stream.copy(size, None)   # the placeholder
                os.link(links[key], dest)
            else:
                write_first(stream, size, dest, path)
                links[key] = dest
            os.chmod(dest, 0o755 if mode & 0o111 else 0o644)
        else:
            stream.copy(size, None)
    xar.stdout.close()
    xar.wait()
    os.makedirs(out, exist_ok=True)
    for p in platforms:
        found = None
        for dp, dns, _ in os.walk(staging):
            if os.path.basename(dp) == "Platforms" and p + ".platform" in dns:
                found = os.path.join(dp, p + ".platform")
                break
        if not found:
            die("%s.platform/Developer is not in %s" % (p, xip))
        dest = os.path.join(out, p + ".platform")
        if os.path.exists(dest):
            shutil.rmtree(dest)
        shutil.move(found, dest)
        print(dest)
    shutil.rmtree(staging)
    shutil.rmtree(aside)


if __name__ == "__main__":
    main()
