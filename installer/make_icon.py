"""Generate installer/snow.ico — a soft-edged snow icon.

Run with:  uv run python installer/make_icon.py
"""
from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter


ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "installer" / "snow.ico"

MASTER = 256
BACKDROP = (12, 30, 56, 255)  # deep navy blue
ROUNDED_RADIUS = 48
MARGIN = 8

# (cx, cy, radius)  positions on the 256×256 master canvas
FLAKES = [
    (58, 42, 9),
    (130, 32, 7),
    (200, 52, 11),
    (90, 82, 8),
    (170, 92, 10),
    (50, 128, 11),
    (130, 128, 13),
    (212, 146, 8),
    (78, 172, 9),
    (158, 192, 11),
    (60, 218, 7),
    (208, 212, 9),
]


def render_master(size: int = MASTER) -> Image.Image:
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    draw.rounded_rectangle(
        (MARGIN, MARGIN, size - MARGIN, size - MARGIN),
        radius=ROUNDED_RADIUS, fill=BACKDROP,
    )

    flakes = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    fdraw = ImageDraw.Draw(flakes)
    scale = size / MASTER
    for cx, cy, r in FLAKES:
        cx, cy, r = cx * scale, cy * scale, r * scale
        fdraw.ellipse((cx - r, cy - r, cx + r, cy + r), fill=(255, 255, 255, 255))
    flakes = flakes.filter(ImageFilter.GaussianBlur(2.5 * scale))

    return Image.alpha_composite(img, flakes)


def main() -> None:
    master = render_master(MASTER)
    sizes = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
    master.save(OUT, format="ICO", sizes=sizes)
    print(f"Wrote {OUT}  ({OUT.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
