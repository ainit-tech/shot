"""Draws the program's icon into installer/ainitShot.ico for the installer, the Start menu and the
list of apps: white corners of a frame on a blue rounded square, on a grid of 16, as
`draw::app_icon` draws it for the notification area. Run it from the repository's root when the
icon changes: python -I installer/make-icon.py"""
import math
import struct
import zlib

BLUE = (47, 111, 235)
CORNERS = [
    [(4, 7), (4, 4), (7, 4)],
    [(9, 4), (12, 4), (12, 7)],
    [(12, 9), (12, 12), (9, 12)],
    [(7, 12), (4, 12), (4, 9)],
]
SIZES = [16, 20, 24, 32, 40, 48, 64, 256]


def tile_distance(x, y):
    """How far a point is outside the rounded square, negative inside, in units of the grid."""
    qx, qy = abs(x - 8) - 4, abs(y - 8) - 4
    return math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) - 3.5


def stroke_distance(x, y):
    """How far a point is from the nearest corner line, in units of the grid."""
    best = math.inf
    for corner in CORNERS:
        for (ax, ay), (bx, by) in zip(corner, corner[1:]):
            dx, dy = bx - ax, by - ay
            t = max(0.0, min(1.0, ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)))
            best = min(best, math.hypot(x - ax - t * dx, y - ay - t * dy))
    return best


def png(size):
    unit = size / 16
    rows = []
    for row in range(size):
        line = bytearray([0])
        for column in range(size):
            x, y = (column + 0.5) / unit, (row + 0.5) / unit
            tile = max(0.0, min(1.0, 0.5 - tile_distance(x, y) * unit))
            stroke = max(0.0, min(1.0, 0.5 - (stroke_distance(x, y) - 0.75) * unit)) * tile
            alpha = stroke + tile * (1 - stroke)
            if alpha <= 0:
                line += b"\0\0\0\0"
                continue
            color = [(255 * stroke + c * tile * (1 - stroke)) / alpha for c in BLUE]
            line += bytes([round(c) for c in color] + [round(alpha * 255)])
        rows.append(bytes(line))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b"")


images = [png(size) for size in SIZES]
# An icon file: its header, an entry for every size, then the images, here as PNG.
data = struct.pack("<HHH", 0, 1, len(images))
offset = 6 + 16 * len(images)
for size, image in zip(SIZES, images):
    data += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(image), offset)
    offset += len(image)
data += b"".join(images)
with open("installer/ainitShot.ico", "wb") as f:
    f.write(data)
print("installer/ainitShot.ico:", len(data), "bytes")
