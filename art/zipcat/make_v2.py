#!/usr/bin/env python3
"""Purrzip art v2 (2026-11-25): pictures built on GmCat's REFERENCE sprite (dot eyes, small nose),
read cell for cell from GmCat Friends/Final - grid redraws/gmcat.png (approved grid; original untouched).
Floor shadow dropped (icons: cat only). Variants:
  zip    - holding the zipped folder (new app icon)
  wave   - waving, mug in the other paw          (replaces 'hello', whose mouth read like a mustache)
  look   - peering through a magnifying glass     (replaces 'look', top hat + monocle read as a gentleman)
  busy   - working hard, sweat drops              (replaces 'work', the teeth read like a beard)
usage: python3 make_v2.py  -> PNGs next to this file
"""
import json, os
from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
REF = json.load(open(os.path.join(HERE, "ref-grid.json")))
PAL = {k: tuple(v) for k, v in REF["pal"].items()}
PAL.update({"Y": (255, 200, 61), "y": (232, 165, 58), "Z": (56, 40, 72), "Q": (232, 234, 244),
            "h": (240, 104, 138), "L": (196, 202, 220), "B": (120, 190, 236), "b": (200, 232, 250), "G": (255, 255, 255), "H": (150, 104, 72)})
PAD = 4  # empty columns added on the left (room for a raised arm) and rows on top (sweat drops)


def base():
    g = [list("." * PAD + r) for r in REF["grid"][:29]]
    g[28] = [("." if ch == "K" else ch) for ch in g[28]]  # no floor shadow
    width = max(len(r) for r in g) + 2
    g = [r + ["."] * (width - len(r)) for r in g]
    return [["."] * width for _ in range(PAD)] + g


def put(g, r, c, ch):
    if 0 <= r + PAD < len(g) and 0 <= c + PAD < len(g[0]):
        g[r + PAD][c + PAD] = ch


def sprite(g, r0, c0, rows):
    for dr, line in enumerate(rows):
        for dc, ch in enumerate(line):
            if ch != ".":
                put(g, r0 + dr, c0 + dc, ch)


def clear_chest(g, rows=range(15, 26), cols=range(2, 15)):
    for r in rows:
        for c in cols:
            put(g, r, c, "W")


def zipcat():
    g = base()
    clear_chest(g)
    sprite(g, 15, 3, [
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
    for c0 in (1, 13):  # paws holding the sides
        for (r, dc, ch) in [(17, 1, "#"), (17, 2, "#"), (18, 0, "#"), (18, 1, "W"), (18, 2, "W"), (18, 3, "#"),
                            (19, 0, "#"), (19, 1, "W"), (19, 2, "s"), (19, 3, "#"), (20, 1, "#"), (20, 2, "#")]:
            put(g, r, c0 + dc, ch)
    return g


def P(g, r, c, ch):
    """set a cell in printed coordinates (row 0 = the top of the ears, col 0 = the padded left edge)"""
    g[r + PAD][c] = ch


def paint(g, cells):
    for (r, c, ch) in cells:
        P(g, r, c, ch)


def wave():
    g = base()
    # left paw lets go of the mug: plain fur there, mug keeps its outline
    for r in range(17, 22):
        for c in range(6, 9):
            P(g, r, c, "s" if r >= 19 and c == 6 else "W")
    P(g, 18, 9, "#"); P(g, 21, 9, "#")
    # raised arm from the shoulder up beside the head, paw at the top
    paint(g, [(6, 1, "#"), (6, 2, "#"), (6, 3, "#"),
              (7, 0, "#"), (7, 1, "W"), (7, 2, "W"), (7, 3, "W"), (7, 4, "#"),
              (8, 0, "#"), (8, 1, "W"), (8, 2, "s"), (8, 3, "W"), (8, 4, "#"),
              (9, 1, "#"), (9, 2, "W"), (9, 3, "W"), (9, 4, "#"),
              (10, 1, "#"), (10, 2, "W"), (10, 3, "W"), (10, 4, "#"),
              (11, 1, "#"), (11, 2, "W"), (11, 3, "W"), (11, 4, "#"),
              (12, 1, "#"), (12, 2, "W"), (12, 3, "W"), (12, 4, "#"),
              (13, 2, "#"), (13, 3, "W"), (13, 4, "W"), (13, 5, "#"),
              (14, 2, "#"), (14, 3, "W"), (14, 4, "W"), (14, 5, "W"), (14, 6, "#"),
              (15, 3, "#"), (15, 4, "W"), (15, 5, "W"), (15, 6, "W"),
              (16, 4, "#"), (16, 5, "W"), (16, 6, "W")])
    return g


def look():
    g = base()
    for r in range(15, 22):
        for c in range(7, 18):
            P(g, r, c, "s" if r == 15 and (c < 10 or c > 14) else "W")
    lens = [(15, 11, "#"), (15, 12, "#"), (15, 13, "#"),
            (16, 10, "#"), (16, 11, "b"), (16, 12, "b"), (16, 13, "b"), (16, 14, "#"),
            (17, 9, "#"), (17, 10, "b"), (17, 11, "G"), (17, 12, "b"), (17, 13, "b"), (17, 14, "b"), (17, 15, "#"),
            (18, 9, "#"), (18, 10, "b"), (18, 11, "b"), (18, 12, "b"), (18, 13, "b"), (18, 14, "b"), (18, 15, "#"),
            (19, 9, "#"), (19, 10, "b"), (19, 11, "b"), (19, 12, "b"), (19, 13, "b"), (19, 14, "b"), (19, 15, "#"),
            (20, 10, "#"), (20, 11, "b"), (20, 12, "b"), (20, 13, "b"), (20, 14, "#"),
            (21, 11, "#"), (21, 12, "#"), (21, 13, "#")]
    handle = [(21, 10, "H"), (22, 9, "H"), (20, 9, "#"), (21, 9, "#"), (22, 10, "#"), (23, 9, "#")]
    paw_l = [(21, 7, "#"), (21, 8, "#"), (22, 6, "#"), (22, 7, "W"), (22, 8, "W"), (23, 6, "#"), (23, 7, "W"),
             (23, 8, "s"), (24, 7, "#"), (24, 8, "#")]
    paw_r = [(17, 16, "#"), (17, 17, "#"), (18, 16, "W"), (18, 17, "W"), (18, 18, "#"), (19, 16, "W"),
             (19, 17, "s"), (19, 18, "#"), (20, 16, "#"), (20, 17, "#")]
    paint(g, lens + handle + paw_l + paw_r)
    return g


def hearts(g=None):
    """happy GmCat with two little hearts (from her 'hearts' emoji): closed happy eyes, pink cheeks, centred nose,
    no mouth (the emoji's mouth read like a mustache), crisp pixel hearts. Pass a grid to give it this face."""
    g = base() if g is None else g
    # eyes: the reference eye cells (rows 8-9, cols 4 and 12 inside the head) become small closed "^" arcs
    for col in (4, 12):
        for r in (8, 9):
            P(g, r, col + PAD, "W")
        P(g, 8, col + PAD, "#"); P(g, 9, col - 1 + PAD, "#"); P(g, 9, col + 1 + PAD, "#")
    # pink cheeks, level with the nose, mirrored around the nose's centre
    P(g, 10, 2 + PAD, "S"); P(g, 10, 14 + PAD, "S")
    heart = ["hh.hh", "hhhhh", ".hhh.", "..h.."]
    for dr, line in enumerate(heart):
        for dc, ch in enumerate(line):
            if ch == "h":
                P(g, 3 + dr, 17 + PAD + dc, "h")  # right heart, by the ear
    g = [["."] * 2 + r for r in g]                # room on the left for the other heart, clear of her outline
    for dr, line in enumerate(heart):
        for dc, ch in enumerate(line):
            if ch == "h":
                P(g, 9 + dr, dc, "h")            # left heart, beside the cheek
    return g


def hearts_zip():
    """website hero: the hearts GmCat holding the zipped folder instead of her mug"""
    return hearts(zipcat())


def padlock():
    """website 'Real locks': GmCat holding a padlock (steel shackle, golden body, keyhole)"""
    g = base()
    clear_chest(g, rows=range(15, 26), cols=range(3, 14))  # her outline and chin shading stay
    sprite(g, 14, 3, [
        "...#####...",
        "..#LLLLL#..",
        "..#L###L#..",
        "..#L#.#L#..",
        "###########",
        "#YYYYYYYYY#",
        "#YYY###YYY#",
        "#YYY###YYY#",
        "#YYYY#YYYY#",
        "#YYYY#YYYY#",
        "#yyyyyyyyy#",
        "###########",
    ])
    for c0 in (1, 13):  # paws holding the sides, like the zip folder
        for (r, dc, ch) in [(19, 1, "#"), (19, 2, "#"), (20, 0, "#"), (20, 1, "W"), (20, 2, "W"), (20, 3, "#"),
                            (21, 0, "#"), (21, 1, "W"), (21, 2, "s"), (21, 3, "#"), (22, 1, "#"), (22, 2, "#")]:
            put(g, r, c0 + dc, ch)
    return g


def busy():
    g = base()
    # sweat drops around the head (each drop: light top, blue bottom)
    for (r, c) in [(1, 17), (5, 19), (9, 18), (-2, 3), (2, -1)]:
        put(g, r, c, "b"); put(g, r + 1, c, "B")
    return g


# Official colours (art.gmcat.io/gmcat/): outline #402E4D, fur #FFFFFF, shade #BCC2E0, ear #F8C0A8
PAL.update({"#": (0x40, 0x2E, 0x4D), "Z": (0x40, 0x2E, 0x4D), "W": (255, 255, 255), "s": (0xBC, 0xC2, 0xE0),
            "S": (0xF8, 0xC0, 0xA8)})
NOSE_ROW = 10  # in printed rows: the 2-cell nose, drawn half a cell to the right so it is exactly centred


def nose_cells(g):
    row = "".join(g[NOSE_ROW + PAD])
    first = row.index("#")                      # left outline of the head
    last = row.rindex("#")                      # right outline
    inner = [c for c in range(first + 1, last) if row[c] == "#"]
    return inner if len(inner) == 2 and inner[1] == inner[0] + 1 else []


def render(g, cell=24, crop=True):
    h, w = len(g), len(g[0])
    im = Image.new("RGBA", (w * cell, h * cell), (0, 0, 0, 0)); d = ImageDraw.Draw(im)
    nose = nose_cells(g)
    for r, row in enumerate(g):
        for c, k in enumerate(row):
            if r == NOSE_ROW + PAD and c in nose:
                k = "W"
            if k in PAL:
                d.rectangle([c * cell, r * cell, (c + 1) * cell - 1, (r + 1) * cell - 1], fill=PAL[k] + (255,))
    if nose:
        r = NOSE_ROW + PAD; x0 = nose[0] * cell + cell // 2
        d.rectangle([x0, r * cell, x0 + 2 * cell - 1, (r + 1) * cell - 1], fill=PAL["#"] + (255,))
    return im.crop(im.getbbox()) if crop else im


if __name__ == "__main__":
    out = {}
    for name, fn in (("zip", zipcat), ("wave", wave), ("look", look), ("busy", busy), ("hearts", hearts), ("hearts-zip", hearts_zip), ("padlock", padlock)):
        g = fn()
        render(g).save(os.path.join(HERE, f"v2-{name}.png"))
        json.dump(["".join(r) for r in g], open(os.path.join(HERE, f"v2-{name}.json"), "w"), indent=0)
    render(base()).save(os.path.join(HERE, "v2-reference.png"))
    print("ok")
