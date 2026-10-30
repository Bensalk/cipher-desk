#!/usr/bin/env python3
"""Builds docs/hero.png from two real app screenshots in docs/screenshots/.

  python3 tools/screenshots/make_hero.py

Needs Pillow. A desktop screenshot sits on the left and a phone screenshot on the
right, each in a simple rounded frame, on a dark teal background in the app's colours.
"""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parents[2]
SHOTS = ROOT / "docs" / "screenshots"
OUT = ROOT / "docs" / "hero.png"

W, H = 1600, 760
TEAL = (0, 121, 107)
AMBER = (255, 193, 7)


def background() -> Image.Image:
    img = Image.new("RGB", (W, H))
    top, bottom = (4, 24, 22), (0, 52, 46)
    px = img.load()
    for y in range(H):
        t = y / (H - 1)
        row = tuple(int(top[i] + (bottom[i] - top[i]) * t) for i in range(3))
        for x in range(W):
            px[x, y] = row
    grid = ImageDraw.Draw(img)
    for x in range(0, W, 40):
        grid.line([(x, 0), (x, H)], fill=(10, 44, 40))
    for y in range(0, H, 40):
        grid.line([(0, y), (W, y)], fill=(10, 44, 40))
    return img


def framed(shot: Image.Image, width: int, radius: int, border: int, colour) -> Image.Image:
    h = round(shot.height * width / shot.width)
    shot = shot.convert("RGB").resize((width, h), Image.LANCZOS)
    fw, fh = width + 2 * border, h + 2 * border
    frame = Image.new("RGBA", (fw, fh), (0, 0, 0, 0))
    d = ImageDraw.Draw(frame)
    d.rounded_rectangle([0, 0, fw - 1, fh - 1], radius=radius, fill=(12, 16, 18, 255),
                        outline=colour + (255,), width=3)
    mask = Image.new("L", (width, h), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, width - 1, h - 1],
                                           radius=max(radius - border, 4), fill=255)
    frame.paste(shot, (border, border), mask)
    return frame


def paste_with_glow(base: Image.Image, item: Image.Image, pos, colour) -> None:
    glow = Image.new("RGBA", base.size, (0, 0, 0, 0))
    shape = Image.new("RGBA", item.size, colour + (150,))
    glow.paste(shape, pos, item.split()[3])
    glow = glow.filter(ImageFilter.GaussianBlur(22))
    base.alpha_composite(glow)
    base.alpha_composite(item, pos)


def main() -> None:
    desktop = Image.open(SHOTS / "desktop-session.png")
    phone = Image.open(SHOTS / "phone-connections.png")
    base = background().convert("RGBA")

    d = framed(desktop, 960, 22, 10, TEAL)
    p = framed(phone, 290, 34, 12, AMBER)
    gap = 90
    left = (W - d.width - gap - p.width) // 2
    paste_with_glow(base, d, (left, (H - d.height) // 2), TEAL)
    paste_with_glow(base, p, (left + d.width + gap, (H - p.height) // 2), AMBER)
    base.convert("RGB").save(OUT, optimize=True)
    print(f"wrote {OUT.relative_to(ROOT)} ({W}x{H})")


if __name__ == "__main__":
    main()
