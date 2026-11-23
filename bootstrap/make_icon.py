"""Draws aareplay.ico: AA Replay's own mark (a play triangle inside a replay arc), no client art.

    python bootstrap/make_icon.py        writes bootstrap/aareplay.ico (16, 24, 32, 48, 64, 128 and 256 px)
"""
import math
import pathlib

from PIL import Image, ImageDraw

HERE = pathlib.Path(__file__).resolve().parent
BACK = (11, 26, 32, 255)
TEAL = (72, 212, 194, 255)   # the launcher's accent (gui.rs GOLD)
VIOLET = (150, 112, 240, 255)  # its second colour


def draw(n: int) -> Image.Image:
    s = 4  # drawn large, then scaled down: smooth edges
    big = n * s
    im = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    d.rounded_rectangle([0, 0, big - 1, big - 1], radius=big * 0.2, fill=BACK)
    c = big / 2
    r = big * 0.34
    w = max(s * 2, int(big * 0.075))
    # the replay arc, open at the top right, with its arrow head
    d.arc([c - r, c - r, c + r, c + r], start=-40, end=270, fill=VIOLET, width=w)
    a = math.radians(-40)
    tip = (c + r * math.cos(a), c + r * math.sin(a))
    h = big * 0.11
    d.polygon([(tip[0] - h * 0.9, tip[1] - h * 0.9), (tip[0] + h * 0.9, tip[1] - h * 0.2), (tip[0] - h * 0.1, tip[1] + h * 0.8)],
              fill=VIOLET)
    # the play triangle
    t = big * 0.2
    d.polygon([(c - t * 0.75, c - t), (c - t * 0.75, c + t), (c + t * 1.05, c)], fill=TEAL)
    return im.resize((n, n), Image.LANCZOS)


if __name__ == "__main__":
    sizes = [16, 24, 32, 48, 64, 128, 256]
    base = draw(256)
    base.save(HERE / "aareplay.ico", sizes=[(n, n) for n in sizes], append_images=[draw(n) for n in sizes[:-1]])
    print(HERE / "aareplay.ico")
