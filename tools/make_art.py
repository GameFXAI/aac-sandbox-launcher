"""Cuts the launcher's pictures (assets/art/*.jpg) from the owner's AA REPLAY paintings.

    python -I tools/make_art.py <kit generated folder>
    python -I tools/make_art.py D:/research/aaemu35/Art/aa-replay-kit/generated

The paintings are the owner's own (made for the AA REPLAY art kit: Art/aa-replay-kit in the server repo); nothing of
AA Classic's art is used. Each picture is a crop of one painting, scaled to what the window draws (the window is a
fixed 1120x680, drawn at up to 1.5x), as a JPEG the exe embeds (gui/art.rs). The fades into the window's background
are painted by the launcher, not baked in, so a picture can sit anywhere.

  hero_play     the battlefield banner          the Play screen behind the pitch
  hero_replays  the arena duel                  the Replays screen
  hero_more     the knight at the gate          Settings, the log and the transfer
  thumb_won / thumb_lost / thumb_fight          a recording's picture on its card, by how the fight ended
  *_grey                                        the same, without colour: a recording past the plan's cap
"""
import os
import sys

from PIL import Image, ImageEnhance, ImageOps

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "art")

# name: (painting, crop box in the 1920x1200 painting, output size)
PICTURES = {
    "hero_play": ("loading_battlefield", (0, 40, 1500, 747), (1680, 792)),
    "hero_replays": ("loading_arena", (420, 250, 1920, 957), (1680, 792)),
    "hero_more": ("rating_screen_bg", (0, 80, 1500, 787), (1680, 792)),
    "thumb_won": ("loading_arena", (960, 650, 1600, 1050), (384, 240)),
    "thumb_lost": ("loading_default", (380, 420, 1500, 1120), (384, 240)),
    "thumb_fight": ("loading_battlefield", (620, 220, 1300, 645), (384, 240)),
}
GREY = ["thumb_won", "thumb_lost", "thumb_fight"]


def main(kit):
    os.makedirs(OUT, exist_ok=True)
    for name, (painting, box, size) in PICTURES.items():
        img = Image.open(os.path.join(kit, painting + ".png")).convert("RGB").crop(box).resize(size, Image.LANCZOS)
        img.save(os.path.join(OUT, name + ".jpg"), quality=84, optimize=True, progressive=False)
        if name in GREY:
            grey = ImageEnhance.Brightness(ImageOps.grayscale(img).convert("RGB")).enhance(0.8)
            grey.save(os.path.join(OUT, name + "_grey.jpg"), quality=84, optimize=True)
    for f in sorted(os.listdir(OUT)):
        print(f, os.path.getsize(os.path.join(OUT, f)))


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "D:/research/aaemu35/Art/aa-replay-kit/generated")
