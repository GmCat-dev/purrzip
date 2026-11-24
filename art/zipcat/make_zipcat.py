#!/usr/bin/env python3
"""Claude's take: GmCat holding a zip folder, on GmCat's own pixel grid.

The cat is the smiling GmCat, read cell by cell from the cat-only cut-out (27 x 29 cells; the original
cells are 13.36 x 12.74 px, so the picture is drawn back with the same slightly wide cells). Only the mug and
paws are replaced by a folder with a zipper. Originals are never touched.
usage: python3 make_zipcat.py <out folder>
"""
import json, sys, os
from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1] if len(sys.argv) > 1 else HERE
ROWS = json.load(open(os.path.join(HERE, "cat-rows.json")))  # the cat, one character per cell

PAL = {
    "#": (58, 44, 72), "W": (255, 255, 255), "s": (194, 196, 226), "p": (250, 190, 170),
    # folder colours
    "Y": (255, 200, 61), "y": (232, 165, 58), "o": (196, 112, 40),
    "Z": (58, 44, 72), "z": (214, 216, 232), "P": (150, 154, 180), "Q": (232, 234, 244),
}

# Folders are drawn over the chest; paws ('W'/'#') are drawn again on top at the sides.
FOLDERS = {
    # A: manila folder, zipper down the middle with a pull tab
    "A": (7, 15, [
        ".###.....",
        "#yyy#####",
        "#YYYZYYY#",
        "#YYYQYYY#",
        "#YYYZYYY#",
        "#YYYQYYY#",
        "#YY#Z#YY#",
        "#YY#Q#YY#",
        "#yyy#yyy#",
        "#########",
    ]),
    # B: zipper across the top, pull tab hanging at the right
    "B": (7, 15, [
        ".####....",
        "#yyyy####",
        "#YYYYYYY#",
        "#ZzZzZzP#",
        "#YYYYYPQ#",
        "#YYYYYYP#",
        "#YYYYYYY#",
        "#yyyyyyy#",
        "#########",
    ]),
}
# paw styles drawn over the folder: the original GmCat paw strokes (dark lines reaching in from the arms)
PAWS = {
    "strokes": [(18, 6, "#"), (18, 8, "#"), (18, 16, "#"), (18, 14, "#"),
                (21, 5, "#"), (21, 6, "#"), (21, 8, "#"), (21, 17, "#"), (21, 16, "#"), (21, 14, "#")],
    # white paws holding the folder's sides
    "mitts": [(17, 7, "#"), (17, 8, "#"), (18, 6, "W"), (18, 7, "W"), (18, 8, "W"), (18, 9, "#"), (19, 7, "#"), (19, 8, "#"),
              (17, 15, "#"), (17, 14, "#"), (18, 16, "W"), (18, 15, "W"), (18, 14, "W"), (18, 13, "#"), (19, 15, "#"), (19, 14, "#")],
}
FOLDERS["blend"] = (6, 15, [  # Pixel's layout (big folder, long zipper, chunky pull), on GmCat's grid
    ".###.......",
    "#YYY#######",
    "#YYYYQYYYY#",
    "#YYYYZYYYY#",
    "#YYYYQYYYY#",
    "#YYYYZYYYY#",
    "#YYYYQYYYY#",
    "#YYYZZZYYY#",
    "#YYYZQZYYY#",
    "#yyyZZZyyy#",
    "###########",
])
PAWS["side-mitts"] = [(17, 5, "#"), (17, 6, "#"), (18, 5, "W"), (18, 6, "W"), (18, 7, "#"), (19, 5, "W"), (19, 6, "s"), (19, 7, "#"), (20, 5, "#"), (20, 6, "#"),
                      (17, 17, "#"), (17, 16, "#"), (18, 17, "W"), (18, 16, "W"), (18, 15, "#"), (19, 17, "W"), (19, 16, "s"), (19, 15, "#"), (20, 17, "#"), (20, 16, "#")]
VARIANTS = {"A-strokes": ("A", "strokes"), "A-mitts": ("A", "mitts"), "blend": ("blend", "side-mitts")}


def grid_for(key):
    folder, paws = VARIANTS[key]
    g = [list(r) for r in ROWS]
    # the mug area becomes plain body (white above, shading below) before the folder goes on
    if folder == "blend":
        for r in range(15, 26):
            for c in range(5, 18):
                g[r][c] = "W"
        for r in range(17, 21):  # mitts reach past the body line
            g[r][4] = "#" if r in (17, 20) else g[r][4]
    else:
        for r in range(15, 25):
            for c in range(6, 17):
                g[r][c] = "s" if r in (15, 22) or 19 <= r <= 22 and c in (6, 16) else "W"
    c0, r0, sprite = FOLDERS[folder]
    for dr, line in enumerate(sprite):
        for dc, ch in enumerate(line):
            if ch != ".":
                g[r0 + dr][c0 + dc] = ch
    for r, c, ch in PAWS[paws]:
        g[r][c] = ch
    return g


def render(g, cw=21, ch=20):
    h, w = len(g), len(g[0])
    im = Image.new("RGBA", (w * cw, h * ch), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    for r, row in enumerate(g):
        for c, k in enumerate(row):
            if k in PAL:
                d.rectangle([c * cw, r * ch, (c + 1) * cw - 1, (r + 1) * ch - 1], fill=PAL[k] + (255,))
    return im


if __name__ == "__main__":
    for key in VARIANTS:
        g = grid_for(key)
        render(g).save(os.path.join(OUT, f"zipcat-{key}.png"))
        json.dump(["".join(r) for r in g], open(os.path.join(OUT, f"zipcat-{key}.json"), "w"), indent=0)
    render([list(r) for r in ROWS]).save(os.path.join(OUT, "cat-from-grid.png"))
    print("done")
