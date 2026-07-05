#!/usr/bin/env python3
"""Generate a 1024x1024 solid dark-slate RGBA PNG placeholder icon.
Dependency-free (stdlib zlib/struct). Usage: python3 scripts/gen_icon.py <out.png>
"""
import struct
import sys
import zlib

W = H = 1024
PIXEL = bytes((11, 15, 20, 255))  # #0b0f14 opaque, matches app dark bg


def chunk(tag: bytes, data: bytes) -> bytes:
    body = tag + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)


def main() -> None:
    out = sys.argv[1]
    row = b"\x00" + PIXEL * W  # filter byte 0 (None) + one row of pixels
    raw = row * H
    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 6, 0, 0, 0))  # RGBA, 8-bit
    png += chunk(b"IDAT", zlib.compress(raw, 9))
    png += chunk(b"IEND", b"")
    with open(out, "wb") as f:
        f.write(png)
    print(f"wrote {out} ({W}x{H})")


if __name__ == "__main__":
    main()
