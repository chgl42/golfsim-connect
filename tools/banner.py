#!/usr/bin/env python3
"""Renders docs/images/banner.png from the play screenshot (macOS fonts, needs Pillow).
Usage:  python3 tools/banner.py .   (after regenerating the screenshots)"""
import sys
from PIL import Image, ImageDraw, ImageFont, ImageFilter
root = sys.argv[1]
S = 2  # render at 2x, then downscale for smooth edges
W, H = 1600 * S, 560 * S
PAGE = (11, 18, 32); LINE = (34, 49, 79); WHITE = (255, 255, 255); SOFT = (214, 222, 238)
MUTED = (127, 143, 176); ACCENT = (61, 111, 230); GREEN = (61, 220, 106)
cond = lambda px: ImageFont.truetype("/System/Library/Fonts/Supplemental/DIN Condensed Bold.ttf", px * S)
body = lambda px: ImageFont.truetype("/System/Library/Fonts/Supplemental/Arial.ttf", px * S)

img = Image.new("RGB", (W, H), PAGE)
d = ImageDraw.Draw(img)
# faint perspective lines in the lower half, like the old banner
for i in range(-12, 13):
    d.line([(W * 0.55, H * 0.45), (W * 0.55 + i * 260 * S, H)], fill=(16, 26, 44), width=S)

# icon
icon = Image.open(f"{root}/packaging/icon.png").convert("RGBA").resize((104 * S, 104 * S), Image.LANCZOS)
img.paste(icon, (74 * S, 92 * S), icon)

# wordmark
x, y = 200 * S, 112 * S
d.text((x, y), "OPEN", font=cond(66), fill=MUTED)
x += d.textlength("OPEN", font=cond(66)) + 2 * S
d.text((x, y), "LAUNCH", font=cond(66), fill=WHITE)
x += d.textlength("LAUNCH", font=cond(66)) + 12 * S
pw = d.textlength("CONNECT", font=cond(50)) + 28 * S
d.rounded_rectangle([x, y + 2 * S, x + pw, y + 60 * S], radius=10 * S, fill=ACCENT)
d.text((x + 14 * S, y + 12 * S), "CONNECT", font=cond(50), fill=WHITE)

# tagline
d.text((80 * S, 262 * S), "Launch monitor → GSPro on macOS.", font=body(27), fill=SOFT)
d.text((80 * S, 302 * S), "Garmin R10 via direct Bluetooth. No iPhone, no E6.", font=body(27), fill=SOFT)

# pills
x = 80 * S
for label, col in [("APPLE SILICON", ACCENT), ("CROSSOVER", ACCENT), ("OPEN SOURCE", GREEN)]:
    w = d.textlength(label, font=cond(22)) + 28 * S
    d.rounded_rectangle([x, 370 * S, x + w, 404 * S], radius=17 * S, outline=col, width=2 * S)
    d.text((x + 14 * S, 378 * S), label, font=cond(22), fill=col)
    x += w + 12 * S
d.text((80 * S, 470 * S), "Not affiliated with Garmin or GSPro.", font=body(17), fill=MUTED)

# app screenshot with frame and soft shadow
shot = Image.open(f"{root}/docs/images/screenshot-play.png").convert("RGB")
sw = 760 * S
shot = shot.resize((sw, round(shot.height * sw / shot.width)), Image.LANCZOS)
sx, sy = 780 * S, (H - shot.height) // 2
shadow = Image.new("L", (W, H), 0)
ImageDraw.Draw(shadow).rounded_rectangle([sx, sy + 8 * S, sx + sw, sy + shot.height + 8 * S], radius=14 * S, fill=150)
img.paste((4, 8, 16), mask=shadow.filter(ImageFilter.GaussianBlur(18 * S)))
mask = Image.new("L", shot.size, 0)
ImageDraw.Draw(mask).rounded_rectangle([0, 0, sw - 1, shot.height - 1], radius=12 * S, fill=255)
img.paste(shot, (sx, sy), mask)
d.rounded_rectangle([sx, sy, sx + sw, sy + shot.height], radius=12 * S, outline=LINE, width=2 * S)

img.resize((1600, 560), Image.LANCZOS).save(f"{root}/docs/images/banner.png", optimize=True)
print("ok")
