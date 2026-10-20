#!/usr/bin/env python3
"""Renders the golf ball in packaging/icon.png as a shaded 3D sphere with dimples
(replaces the ball and its shadow; the background and the signal waves stay).
Pure Python + Pillow.  Usage:  python3 tools/icon_ball.py packaging/icon.png"""
import math
import sys

from PIL import Image, ImageDraw, ImageFilter

PATH = sys.argv[1]
CX, CY, R = 512, 642, 86          # ball centre and radius in the 1024 px icon
SS = 3                            # supersampling
CLEAR = (380, 548, 650, 800)      # old ball + shadow, refilled with the background gradient

LIGHT = (-0.42, -0.62, 0.66)      # from the upper left, towards the viewer
DIMPLES = 332                     # typical count on a real ball
DIMPLE_RADIUS = 0.098             # angular radius (rad); leaves small lands between dimples


def norm(v):
    l = math.sqrt(sum(c * c for c in v))
    return tuple(c / l for c in v)


def rotate(p, ax, ay):
    """Tilt the dimple pattern so it doesn't look aligned to the view."""
    x, y, z = p
    y, z = y * math.cos(ax) - z * math.sin(ax), y * math.sin(ax) + z * math.cos(ax)
    x, z = x * math.cos(ay) + z * math.sin(ay), -x * math.sin(ay) + z * math.cos(ay)
    return x, y, z


def fibonacci_sphere(n):
    golden = math.pi * (3 - math.sqrt(5))
    for i in range(n):
        y = 1 - 2 * (i + 0.5) / n
        r = math.sqrt(1 - y * y)
        yield rotate((math.cos(golden * i) * r, y, math.sin(golden * i) * r), 0.5, 0.3)


L = norm(LIGHT)
H = norm((L[0], L[1], L[2] + 1))  # Blinn half vector, viewer at +z
dimples = [d for d in fibonacci_sphere(DIMPLES) if d[2] > -DIMPLE_RADIUS]
CELL = 0.12
grid = {}
for d in dimples:
    grid.setdefault((int(math.floor(d[0] / CELL)), int(math.floor(d[1] / CELL))), []).append(d)
cos_a = math.cos(DIMPLE_RADIUS)


def shade(nx, ny, nz):
    """Colour of the ball surface with normal n (screen y down)."""
    n = (nx, ny, nz)
    gx, gy = int(math.floor(nx / CELL)), int(math.floor(ny / CELL))
    best, best_dot = None, cos_a
    for ix in (gx - 1, gx, gx + 1):
        for iy in (gy - 1, gy, gy + 1):
            for d in grid.get((ix, iy), ()):
                dot = nx * d[0] + ny * d[1] + nz * d[2]
                if dot > best_dot:
                    best, best_dot = d, dot
    ao = 1.0
    if best is not None:
        rho = math.acos(min(1.0, best_dot)) / DIMPLE_RADIUS  # 0 at the dimple centre, 1 at its rim
        t = (nx - best[0] * best_dot, ny - best[1] * best_dot, nz - best[2] * best_dot)
        tl = math.sqrt(sum(c * c for c in t)) or 1.0
        # Concave dimple: the normal tilts towards the dimple centre, most near the rim.
        k = 0.42 * math.sin(rho * math.pi / 2)
        n = norm((nx - k * t[0] / tl, ny - k * t[1] / tl, nz - k * t[2] / tl))
        ao = 1 - 0.07 * (1 - rho * rho)
    diffuse = max(0.0, n[0] * L[0] + n[1] * L[1] + n[2] * L[2])
    spec = max(0.0, n[0] * H[0] + n[1] * H[1] + n[2] * H[2]) ** 48
    light = (0.50 + 0.52 * diffuse) * ao
    # Slightly cool shadows, warm-white lit side.
    base = (248 * light + 10 * (1 - light), 249 * light + 12 * (1 - light), 252 * light + 22 * (1 - light))
    return tuple(min(255, int(c + 120 * spec)) for c in base)


def render_ball():
    size = 2 * R * SS + 2 * SS
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    px = img.load()
    c = size / 2
    rr = R * SS
    for y in range(size):
        for x in range(size):
            dx, dy = (x + 0.5 - c) / rr, (y + 0.5 - c) / rr
            d2 = dx * dx + dy * dy
            if d2 > 1:
                continue
            px[x, y] = shade(dx, dy, math.sqrt(1 - d2)) + (255,)
    # Thin rim, as in the rest of the icon.
    ImageDraw.Draw(img).ellipse([c - rr, c - rr, c + rr, c + rr], outline=(176, 180, 188, 255), width=SS)
    return img.resize((size // SS, size // SS), Image.LANCZOS)


icon = Image.open(PATH).convert("RGBA")
px = icon.load()
x0, y0, x1, y1 = CLEAR
for y in range(y0, y1):
    bg = px[200, y]  # background is a vertical gradient
    for x in range(x0, x1):
        px[x, y] = bg

# Soft drop shadow (light from the upper left) plus a tighter contact shadow.
shadow = Image.new("L", icon.size, 0)
d = ImageDraw.Draw(shadow)
d.ellipse([CX - R + 8, CY - R + 16, CX + R + 8, CY + R + 16], fill=95)
shadow = shadow.filter(ImageFilter.GaussianBlur(16))
contact = Image.new("L", icon.size, 0)
ImageDraw.Draw(contact).ellipse([CX - R * 0.8, CY + R * 0.72, CX + R * 0.9, CY + R * 1.08], fill=70)
contact = contact.filter(ImageFilter.GaussianBlur(7))
for m in (shadow, contact):
    icon = Image.composite(Image.new("RGBA", icon.size, (60, 64, 72, 255)), icon, m.point(lambda v: v * 0.9))

ball = render_ball()
icon.alpha_composite(ball, (CX - ball.width // 2, CY - ball.height // 2))
icon.save(PATH)
print("ok")
