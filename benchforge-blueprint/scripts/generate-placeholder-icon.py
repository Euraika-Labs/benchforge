#!/usr/bin/env python3
from __future__ import annotations

import struct
import zlib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "app-scaffold" / "src-tauri" / "icons" / "icon.png"
SIZE = 128


def chunk(kind: bytes, data: bytes) -> bytes:
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)


def pixel(x: int, y: int) -> tuple[int, int, int, int]:
    r = 18 + (x * 35 // SIZE)
    g = 35 + (y * 70 // SIZE)
    b = 42 + ((x + y) * 45 // (SIZE * 2))
    if 28 <= x <= 54 and 24 <= y <= 104:
        return (236, 247, 245, 255)
    if 54 < x <= 82 and (24 <= y <= 44 or 64 <= y <= 84 or 88 <= y <= 104):
        return (236, 247, 245, 255)
    if 90 <= x <= 104 and 24 <= y <= 104:
        return (110, 231, 183, 255)
    if 104 < x <= 116 and (24 <= y <= 44 or 64 <= y <= 80):
        return (110, 231, 183, 255)
    return (r, g, b, 255)


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    rows = []
    for y in range(SIZE):
        row = bytearray([0])
        for x in range(SIZE):
            row.extend(pixel(x, y))
        rows.append(bytes(row))

    png = b"".join(
        [
            b"\x89PNG\r\n\x1a\n",
            chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)),
            chunk(b"IDAT", zlib.compress(b"".join(rows), 9)),
            chunk(b"IEND", b""),
        ]
    )
    OUT.write_bytes(png)
    print(OUT.relative_to(ROOT))


if __name__ == "__main__":
    main()
