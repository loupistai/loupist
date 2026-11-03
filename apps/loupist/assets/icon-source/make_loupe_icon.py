"""Loupist app icon: a loupe over a small knowledge map. Paper tile, ink lens, one amber node.

Drawn at 4x and downsampled for clean edges. Usage: make_loupe_icon.py OUT.png [dark]
"""
import math
import sys
from PIL import Image, ImageDraw, ImageFilter

OUT = sys.argv[1]
DARK = len(sys.argv) > 2 and sys.argv[2] == "dark"
S = 4                      # supersampling
C = 1024 * S
TILE, ORIGIN, RADIUS = 824 * S, 100 * S, 186 * S

PAPER = (243, 238, 227)
INK = (27, 26, 24)
AMBER = (196, 106, 28)     # burnt amber, one accent
MUTED = (150, 143, 130)
if DARK:
    PAPER, INK, MUTED = (19, 19, 18), (238, 232, 219), (120, 116, 108)
    AMBER = (232, 150, 62)

img = Image.new("RGBA", (C, C), (0, 0, 0, 0))

# Tile with a soft shadow.
shadow = Image.new("L", (C, C), 0)
ImageDraw.Draw(shadow).rounded_rectangle((ORIGIN, ORIGIN + 14 * S, ORIGIN + TILE, ORIGIN + TILE + 14 * S), radius=RADIUS, fill=95)
shadow = shadow.filter(ImageFilter.GaussianBlur(20 * S))
img.paste(Image.new("RGBA", (C, C), (0, 0, 0, 255)), (0, 0), shadow)
tile = Image.new("RGBA", (C, C), (0, 0, 0, 0))
ImageDraw.Draw(tile).rounded_rectangle((ORIGIN, ORIGIN, ORIGIN + TILE, ORIGIN + TILE), radius=RADIUS, fill=PAPER + (255,))
img.alpha_composite(tile)

d = ImageDraw.Draw(img)
p = lambda v: v * S  # 1024-space to canvas

# Lens geometry (1024 space).
cx, cy, r, ring = 432, 432, 206, 42

# The map seen through the lens: five names, one in focus (amber).
nodes = [(-118, -40, 22), (-30, -120, 18), (96, -70, 20), (70, 78, 18), (-76, 98, 16)]
focal = (6, 4, 40)
edges = [(focal, n) for n in nodes] + [(nodes[0], nodes[1]), (nodes[2], nodes[3]), (nodes[4], nodes[0])]
for a, b in edges:
    d.line([(p(cx + a[0]), p(cy + a[1])), (p(cx + b[0]), p(cy + b[1]))], fill=MUTED, width=p(9))
for x, y, rad in nodes:
    d.ellipse((p(cx + x - rad), p(cy + y - rad), p(cx + x + rad), p(cy + y + rad)), fill=INK)
fx, fy, fr = focal
d.ellipse((p(cx + fx - fr), p(cy + fy - fr), p(cx + fx + fr), p(cy + fy + fr)), fill=AMBER)

# Handle: a rounded bar at 45° from the ring, ink, with an amber-free collar for weight.
angle = math.radians(45)
hx0 = cx + (r + ring / 2 - 6) * math.cos(angle)
hy0 = cy + (r + ring / 2 - 6) * math.sin(angle)
length, width = 236, 72
hx1, hy1 = hx0 + length * math.cos(angle), hy0 + length * math.sin(angle)
d.line([(p(hx0), p(hy0)), (p(hx1), p(hy1))], fill=INK, width=p(width))
for x, y in ((hx1, hy1),):
    d.ellipse((p(x - width / 2), p(y - width / 2), p(x + width / 2), p(y + width / 2)), fill=INK)
# A short neck, slightly wider than the handle, joins it to the ring.
nx1, ny1 = hx0 + 40 * math.cos(angle), hy0 + 40 * math.sin(angle)
d.line([(p(hx0 - 10 * math.cos(angle)), p(hy0 - 10 * math.sin(angle))), (p(nx1), p(ny1))], fill=INK, width=p(width + 18))
for x, y in ((nx1, ny1),):
    d.ellipse((p(x - (width + 18) / 2), p(y - (width + 18) / 2), p(x + (width + 18) / 2), p(y + (width + 18) / 2)), fill=INK)

# Ring on top of the map's edges.
d.ellipse((p(cx - r), p(cy - r), p(cx + r), p(cy + r)), outline=INK, width=p(ring))

img = img.resize((1024, 1024), Image.LANCZOS)
img.save(OUT)
print("wrote", OUT)
