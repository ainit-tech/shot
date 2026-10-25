"""Draws the pictures of the installer, each for the scales of Windows from 100 % to 250 %:
- the large one beside the first and the last page: the program's icon in a selection with its
  white round handles, as the frozen screen draws one, in the program's blue light on its dark;
- the small one at the top of the pages between them: the icon alone.
Run it from the repository's root when the icon or the colours change:
python -I installer/make-images.py"""
import math
import os
import struct
import zlib

BLUE = (47, 111, 235)
DARK = (22, 24, 29)
DEEP = (16, 31, 60)
WHITE = (255, 255, 255)
CORNERS = [
    [(4, 7), (4, 4), (7, 4)],
    [(9, 4), (12, 4), (12, 7)],
    [(12, 9), (12, 12), (9, 12)],
    [(7, 12), (4, 12), (4, 9)],
]
# The sizes Inno Setup recommends for the modern style, at 100, 125, 150, 175, 200, 225 and 250 %.
LARGE = [(202, 386), (269, 515), (336, 643), (403, 772), (430, 824), (498, 953), (534, 1022)]
SMALL = [58, 77, 97, 116, 124, 143, 159]


def tile_distance(x, y):
    """How far a point is outside the icon's rounded square, negative inside, on its grid of 16."""
    qx, qy = abs(x - 8) - 4, abs(y - 8) - 4
    return math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) - 3.5


def stroke_distance(x, y):
    """How far a point is from the nearest corner line of the icon, on its grid of 16."""
    best = math.inf
    for corner in CORNERS:
        for (ax, ay), (bx, by) in zip(corner, corner[1:]):
            dx, dy = bx - ax, by - ay
            t = max(0.0, min(1.0, ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)))
            best = min(best, math.hypot(x - ax - t * dx, y - ay - t * dy))
    return best


def coverage(distance):
    """How much of a pixel a shape covers, from the distance of its centre to the shape's edge in
    pixels, negative inside."""
    return max(0.0, min(1.0, 0.5 - distance))


def over(below, color, alpha):
    """A colour with its opacity laid over another one, both as (r, g, b, a) with a from 0 to 1."""
    r, g, b, a = below
    out = alpha + a * (1 - alpha)
    if out <= 0:
        return (0.0, 0.0, 0.0, 0.0)
    mix = [(c * alpha + d * a * (1 - alpha)) / out for c, d in zip(color, (r, g, b))]
    return (*mix, out)


def icon(pixel, x, y, left, top, size):
    """Lays the icon, `size` pixels wide with its top left corner at (left, top), over a pixel."""
    unit = size / 16
    gx, gy = (x - left) / unit, (y - top) / unit
    tile = coverage(tile_distance(gx, gy) * unit)
    if tile <= 0:
        return pixel
    stroke = coverage((stroke_distance(gx, gy) - 0.75) * unit) * tile
    pixel = over(pixel, BLUE, tile)
    return over(pixel, WHITE, stroke)


def large(width, height):
    """The picture beside the first and the last page."""
    scale = width / 202
    cx, cy = width / 2, height * 0.42
    size = 76 * scale
    margin = 30 * scale
    frame = (cx - size / 2 - margin, cy - size / 2 - margin, cx + size / 2 + margin, cy + size / 2 + margin)
    handle = 3.5 * scale
    line = max(1.0, scale)
    rows = []
    for row in range(height):
        out = bytearray([0])
        for column in range(width):
            x, y = column + 0.5, row + 0.5
            # The dark goes deep blue towards the top, and the program's blue glows behind the icon.
            k = y / height
            pixel = tuple(d + (e - d) * (1 - k) ** 2 for d, e in zip(DARK, DEEP)) + (1.0,)
            glow = max(0.0, 1 - math.hypot(x - cx, (y - cy) * 0.9) / (width * 0.62))
            pixel = over(pixel, BLUE, 0.42 * glow ** 2)
            # The selection: a thin white frame with round handles at its corners and sides.
            left, top, right, bottom = frame
            edge = max(min(abs(x - left), abs(x - right)) if top <= y <= bottom else math.inf,
                       -math.inf)
            edge = min(edge, min(abs(y - top), abs(y - bottom)) if left <= x <= right else math.inf)
            pixel = over(pixel, WHITE, 0.82 * coverage(edge - line / 2))
            for hx, hy in ((left, top), (cx, top), (right, top), (right, cy), (right, bottom), (cx, bottom),
                           (left, bottom), (left, cy)):
                distance = math.hypot(x - hx, y - hy)
                if distance < handle + 2 * line + 1:
                    pixel = over(pixel, (0, 0, 0), 0.3 * coverage(distance - handle - line))
                    pixel = over(pixel, WHITE, coverage(distance - handle))
            pixel = icon(pixel, x, y, cx - size / 2, cy - size / 2, size)
            out += bytes(round(c) for c in pixel[:3]) + bytes([255])
        rows.append(bytes(out))
    return rows


def small(size):
    """The icon alone on nothing, with a little room around it."""
    side = size * 0.84
    start = (size - side) / 2
    rows = []
    for row in range(size):
        out = bytearray([0])
        for column in range(size):
            r, g, b, a = icon((0.0, 0.0, 0.0, 0.0), column + 0.5, row + 0.5, start, start, side)
            out += bytes([round(r), round(g), round(b), round(a * 255)])
        rows.append(bytes(out))
    return rows


def png(width, height, rows):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b"")


os.makedirs("installer/images", exist_ok=True)
for percent, (width, height), side in zip((100, 125, 150, 175, 200, 225, 250), LARGE, SMALL):
    for name, data in ((f"large-{percent}.png", png(width, height, large(width, height))),
                       (f"small-{percent}.png", png(side, side, small(side)))):
        with open(f"installer/images/{name}", "wb") as f:
            f.write(data)
        print(f"installer/images/{name}: {len(data)} bytes")
