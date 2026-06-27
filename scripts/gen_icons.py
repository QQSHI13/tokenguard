#!/usr/bin/env python3
"""Generate Token Guard tray/bundle icons. Shield silhouette, green state."""
from PIL import Image, ImageDraw
import os

OUT = os.path.join(os.path.dirname(__file__), "..", "src-tauri", "icons")
os.makedirs(OUT, exist_ok=True)

def shield(size: int, fill=(37, 150, 90), stroke=(255, 255, 255)):
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    pad = size * 0.06
    pts = [
        (size * 0.5, size * 0.08),   # top center
        (size - pad, size * 0.20),  # top right
        (size - pad, size * 0.55),  # right
        (size * 0.5, size * 0.92),  # bottom point
        (pad, size * 0.55),         # left
        (pad, size * 0.20),         # top left
    ]
    sw = max(2, int(size * 0.12))
    d.polygon(pts, fill=fill, outline=stroke, width=sw)
    # bold white "T" — proportions chosen to survive 16px downscale
    tw = size * 0.46
    cx, cy = size * 0.5, size * 0.40
    th = max(3, int(size * 0.16))
    d.line((cx - tw / 2, cy, cx + tw / 2, cy), fill=stroke, width=th)
    d.line((cx, cy, cx, cy + size * 0.32), fill=stroke, width=th)
    return img

GREEN = (37, 150, 90)
YELLOW = (210, 160, 40)
RED = (200, 60, 60)

for s in (32, 128, 256, 512):
    shield(s, fill=GREEN).save(os.path.join(OUT, f"{s}x{s}.png"))

# aliases tauri expects
shield(128, fill=GREEN).save(os.path.join(OUT, "128x128.png"))
shield(256, fill=GREEN).save(os.path.join(OUT, "128x128@2x.png"))
shield(512, fill=GREEN).save(os.path.join(OUT, "icon.png"))

# tray color-state variants (512px)
for name, color in (("green", GREEN), ("yellow", YELLOW), ("red", RED)):
    shield(512, fill=color).save(os.path.join(OUT, f"icon_{name}.png"))

# windows .ico (multi-size)
shield(256, fill=GREEN).save(
    os.path.join(OUT, "icon.ico"),
    format="ICO",
    sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
)

print("icons written to", OUT)
for f in sorted(os.listdir(OUT)):
    print(" ", f)
