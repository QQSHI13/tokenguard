#!/usr/bin/env python3
"""Generate high-DPI colored tray icons for Token Guard."""
from pathlib import Path
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"
SIZE = 1024

# Shield outline as a list of normalized (x, y) points
SHIELD_OUTLINE = [
    (0.5, 0.05),
    (0.95, 0.20),
    (0.88, 0.72),
    (0.5, 0.95),
    (0.12, 0.72),
    (0.05, 0.20),
]


def draw_shield(draw: ImageDraw.ImageDraw, size: int, color: tuple) -> None:
    pts = [(x * size, y * size) for x, y in SHIELD_OUTLINE]
    draw.polygon(pts, fill=color)


def draw_t(draw: ImageDraw.ImageDraw, size: int) -> None:
    # White bold T centered in the shield
    bar_w = int(size * 0.48)
    bar_h = int(size * 0.18)
    stem_w = int(size * 0.20)
    stem_h = int(size * 0.48)
    cx = size // 2
    cy = size // 2 + int(size * 0.02)
    top_y = cy - stem_h // 2
    # top bar
    draw.rectangle(
        [cx - bar_w // 2, top_y, cx + bar_w // 2, top_y + bar_h],
        fill=(255, 255, 255, 255),
    )
    # stem
    draw.rectangle(
        [cx - stem_w // 2, top_y, cx + stem_w // 2, cy + stem_h // 2],
        fill=(255, 255, 255, 255),
    )


def make_icon(color: tuple, path: Path) -> None:
    img = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    draw_shield(draw, SIZE, color)
    draw_t(draw, SIZE)
    img.save(path)
    print(f"saved {path}")


def main() -> None:
    ROOT.mkdir(parents=True, exist_ok=True)
    make_icon((34, 153, 84, 255), ROOT / "icon_green.png")     # emerald-600
    make_icon((234, 179, 8, 255), ROOT / "icon_yellow.png")    # amber-500
    make_icon((249, 115, 22, 255), ROOT / "icon_orange.png")   # orange-500
    make_icon((239, 68, 68, 255), ROOT / "icon_red.png")       # red-500


if __name__ == "__main__":
    main()
