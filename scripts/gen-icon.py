#!/usr/bin/env python3
"""Generate the FlowOSS app icon (mic glyph) without external deps."""
import struct, sys, zlib

SIZE = 128
SS = 4  # supersampling factor
BG = (79, 110, 247)      # accent indigo
FG = (255, 255, 255)


def inside_rounded_rect(x, y, x0, y0, x1, y1, r):
    if x < x0 or x > x1 or y < y0 or y > y1:
        return False
    cx = min(max(x, x0 + r), x1 - r)
    cy = min(max(y, y0 + r), y1 - r)
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r


def inside_capsule(x, y, cx, y0, y1, r):
    if y0 + r <= y <= y1 - r:
        return abs(x - cx) <= r
    cy = y0 + r if y < y0 + r else y1 - r
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r


def inside_mic(x, y):
    # coordinates normalized 0..1
    if inside_capsule(x, y, 0.5, 0.20, 0.52, 0.095):
        return True
    # U-shaped arc under the capsule
    dx, dy = x - 0.5, y - 0.44
    d = (dx * dx + dy * dy) ** 0.5
    if y >= 0.44 and 0.155 <= d <= 0.205:
        return True
    # stem
    if abs(x - 0.5) <= 0.025 and 0.64 <= y <= 0.76:
        return True
    # base
    if inside_capsule(y, x, 0.775, 0.36, 0.64, 0.028):  # horizontal capsule
        return True
    return False


def make_pixels():
    big = SIZE * SS
    rows = []
    for py in range(SIZE):
        row = bytearray()
        for px in range(SIZE):
            bg_hits = fg_hits = 0
            for sy in range(SS):
                for sx in range(SS):
                    x = (px * SS + sx + 0.5) / big
                    y = (py * SS + sy + 0.5) / big
                    if inside_rounded_rect(x, y, 0.03, 0.03, 0.97, 0.97, 0.21):
                        bg_hits += 1
                        if inside_mic(x, y):
                            fg_hits += 1
            total = SS * SS
            alpha = round(255 * bg_hits / total)
            if bg_hits == 0:
                row += bytes((0, 0, 0, 0))
                continue
            t = fg_hits / bg_hits
            r = round(BG[0] + (FG[0] - BG[0]) * t)
            g = round(BG[1] + (FG[1] - BG[1]) * t)
            b = round(BG[2] + (FG[2] - BG[2]) * t)
            row += bytes((r, g, b, alpha))
        rows.append(bytes(row))
    return rows


def write_png(path, rows):
    def chunk(tag, data):
        c = tag + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c))

    raw = b"".join(b"\x00" + row for row in rows)
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )
    with open(path, "wb") as f:
        f.write(png)


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else "apps/desktop/icons/icon.png"
    write_png(out, make_pixels())
    print(f"wrote {out}")
