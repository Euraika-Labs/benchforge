#!/usr/bin/env python3
from __future__ import annotations

import math
import struct
import zlib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "app-scaffold" / "src-tauri" / "icons" / "icon.png"
SIZE = 512
SAMPLES = 3


Color = tuple[float, float, float, float]


def chunk(kind: bytes, data: bytes) -> bytes:
    return (
        struct.pack(">I", len(data))
        + kind
        + data
        + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
    )


def clamp(value: float, low: float = 0.0, high: float = 1.0) -> float:
    return max(low, min(high, value))


def mix(a: Color, b: Color, t: float) -> Color:
    t = clamp(t)
    return tuple(a[i] * (1 - t) + b[i] * t for i in range(4))  # type: ignore[return-value]


def over(bottom: Color, top: Color) -> Color:
    alpha = top[3] + bottom[3] * (1 - top[3])
    if alpha <= 0:
        return (0, 0, 0, 0)
    return (
        (top[0] * top[3] + bottom[0] * bottom[3] * (1 - top[3])) / alpha,
        (top[1] * top[3] + bottom[1] * bottom[3] * (1 - top[3])) / alpha,
        (top[2] * top[3] + bottom[2] * bottom[3] * (1 - top[3])) / alpha,
        alpha,
    )


def inside_round_rect(x: float, y: float, left: float, top: float, right: float, bottom: float, radius: float) -> bool:
    cx = clamp(x, left + radius, right - radius)
    cy = clamp(y, top + radius, bottom - radius)
    return (x - cx) * (x - cx) + (y - cy) * (y - cy) <= radius * radius


def distance_to_segment(px: float, py: float, ax: float, ay: float, bx: float, by: float) -> float:
    dx = bx - ax
    dy = by - ay
    if dx == 0 and dy == 0:
        return math.hypot(px - ax, py - ay)
    t = clamp(((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy))
    return math.hypot(px - (ax + t * dx), py - (ay + t * dy))


def rect_layer(
    color: Color,
    x: float,
    y: float,
    left: float,
    top: float,
    right: float,
    bottom: float,
    radius: float = 0.0,
) -> Color | None:
    inside = (
        inside_round_rect(x, y, left, top, right, bottom, radius)
        if radius
        else left <= x <= right and top <= y <= bottom
    )
    return color if inside else None


def background(x: float, y: float) -> Color:
    base = mix(
        (12 / 255, 22 / 255, 27 / 255, 1),
        (18 / 255, 66 / 255, 70 / 255, 1),
        0.46 * x + 0.38 * (1 - y),
    )
    glow = clamp(1 - math.hypot(x - 0.78, y - 0.22) / 0.78)
    base = mix(base, (77 / 255, 171 / 255, 151 / 255, 1), glow * 0.28)
    shadow = clamp(math.hypot(x - 0.5, y - 0.5) / 0.72)
    return mix(base, (6 / 255, 10 / 255, 13 / 255, 1), shadow * 0.25)


def render_sample(x: float, y: float) -> Color:
    if not inside_round_rect(x, y, 0.035, 0.035, 0.965, 0.965, 0.205):
        return (0, 0, 0, 0)

    color = background(x, y)

    grid_color = (202 / 255, 242 / 255, 229 / 255, 0.10)
    if abs((x - 0.115) % 0.145) < 0.003 or abs((y - 0.12) % 0.145) < 0.003:
        color = over(color, grid_color)

    if inside_round_rect(x, y, 0.10, 0.10, 0.90, 0.90, 0.16):
        color = over(color, (255 / 255, 255 / 255, 255 / 255, 0.035))

    light = (236 / 255, 247 / 255, 245 / 255, 1)
    mint = (105 / 255, 230 / 255, 186 / 255, 1)
    gold = (247 / 255, 190 / 255, 91 / 255, 1)
    coral = (245 / 255, 113 / 255, 104 / 255, 1)

    for layer in (
        rect_layer(light, x, y, 0.205, 0.215, 0.305, 0.785, 0.027),
        rect_layer(light, x, y, 0.285, 0.215, 0.565, 0.315, 0.032),
        rect_layer(light, x, y, 0.285, 0.455, 0.545, 0.555, 0.032),
        rect_layer(light, x, y, 0.285, 0.685, 0.595, 0.785, 0.032),
        rect_layer(light, x, y, 0.535, 0.295, 0.635, 0.475, 0.03),
        rect_layer(light, x, y, 0.565, 0.535, 0.665, 0.705, 0.03),
    ):
        if layer:
            color = over(color, layer)

    for left, top, right, hue in (
        (0.690, 0.585, 0.750, gold),
        (0.780, 0.465, 0.840, coral),
        (0.870, 0.335, 0.930, mint),
    ):
        layer = rect_layer(hue, x, y, left, top, right, 0.785, 0.026)
        if layer:
            color = over(color, layer)

    if distance_to_segment(x, y, 0.705, 0.565, 0.900, 0.335) <= 0.015:
        color = over(color, (236 / 255, 247 / 255, 245 / 255, 0.88))
    for cx, cy, hue in ((0.705, 0.565, gold), (0.795, 0.460, coral), (0.900, 0.335, mint)):
        if math.hypot(x - cx, y - cy) <= 0.035:
            color = over(color, hue)

    if inside_round_rect(x, y, 0.055, 0.055, 0.945, 0.945, 0.19) and not inside_round_rect(
        x, y, 0.070, 0.070, 0.930, 0.930, 0.175
    ):
        color = over(color, (255 / 255, 255 / 255, 255 / 255, 0.14))

    return color


def pixel(x: int, y: int) -> tuple[int, int, int, int]:
    acc = [0.0, 0.0, 0.0, 0.0]
    for sy in range(SAMPLES):
        for sx in range(SAMPLES):
            sample = render_sample((x + (sx + 0.5) / SAMPLES) / SIZE, (y + (sy + 0.5) / SAMPLES) / SIZE)
            for index, value in enumerate(sample):
                acc[index] += value
    count = SAMPLES * SAMPLES
    return tuple(round(clamp(channel / count) * 255) for channel in acc)  # type: ignore[return-value]


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
