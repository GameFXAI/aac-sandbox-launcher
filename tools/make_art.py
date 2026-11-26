"""Cuts the app's moment pictures (assets/art/*.jpg) from the owner's design images.

    python -I tools/make_art.py <design folder>
    python -I tools/make_art.py C:/Users/<you>/.codex/visualizations/2026/11/25/<id>/aa-replay-desktop/obsidian-ember-v2

The owner picked the "Obsidian Ember V2" designs (docs/companion-app.md). Their sample stills are the owner's own
generated art; each is cut from the design image that shows it largest, inside its rounded corners and away from its
badges. They are stand-ins: a full-size picture of the same name in `assets/art/src/` (any size, 16:9 or wider) is used
instead when it is there. The app embeds the results (gui/pictures.rs) and picks one per kind of moment.
"""
import os
import sys

from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "assets", "art")
OWN = os.path.join(OUT, "src")

# name: (design image, crop box)
STILLS = {
    "moment_fog": ("community.png", (262, 322, 788, 546)),
    "moment_duel": ("community.png", (262, 573, 716, 755)),
    "moment_siege": ("community.png", (740, 573, 1193, 755)),
    "moment_naval": ("guild.png", (265, 633, 714, 800)),
    "moment_dawn": ("guild.png", (736, 633, 1185, 800)),
    "moment_port": ("launch-recovery.png", (254, 431, 557, 620)),
    "moment_caravan": ("launch-recovery.png", (254, 695, 557, 879)),
    "moment_harbor": ("launch-recovery.png", (579, 695, 877, 879)),
    "moment_wall": ("launch-recovery.png", (896, 695, 1195, 879)),
    "crest": ("guild.png", (276, 82, 366, 188)),
}


def main(designs):
    os.makedirs(OUT, exist_ok=True)
    for name, (image, box) in STILLS.items():
        own = os.path.join(OWN, name + ".png")
        if os.path.exists(own):
            img = Image.open(own).convert("RGB")
            img.thumbnail((960, 540), Image.LANCZOS)
        else:
            img = Image.open(os.path.join(designs, image)).convert("RGB").crop(box)
        img.save(os.path.join(OUT, name + ".jpg"), quality=92, optimize=True)
        print(name, img.size, os.path.getsize(os.path.join(OUT, name + ".jpg")))


if __name__ == "__main__":
    main(sys.argv[1])
