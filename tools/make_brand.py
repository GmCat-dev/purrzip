#!/usr/bin/env python3
"""Purrzip brand art (picked 2026-11-24): app icon = zip-folder GmCat on the orange rounded square,
.purr file icon, Windows installer pictures (orange side picture, header, installer icon), Mac disk-image background.

The zip-folder GmCat is new art: the smiling GmCat read cell by cell from her cut-out (27 x 29 cells,
original cells 13.36 x 12.74 px, kept slightly wide) with the mug replaced by a zipped folder (Pixel's layout,
Claude's grid drawing, Pixel reviewed). Tiny sizes (16 and 32 px) are hand-drawn, not shrunk.
Drawings and grids live in art/zipcat/. Writes art/icon-1024.png (feed it to `npx tauri icon`), art/purr-file/*,
art/installer/*. usage: python3 tools/make_brand.py
"""
import io, json, os, struct
from PIL import Image, ImageDraw, ImageFont

PZ = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ART = os.path.join(PZ, "art")
GRID = json.load(open(os.path.join(ART, "zipcat", "zipcat-grid.json")))
PAL = {"#": (58, 44, 72), "W": (255, 255, 255), "s": (194, 196, 226), "p": (250, 190, 170),
       "Y": (255, 200, 61), "y": (232, 165, 58), "Z": (58, 44, 72), "Q": (232, 234, 244),
       "O": (240, 134, 56), "o": (226, 112, 44), "F": (255, 252, 246), "f": (234, 223, 202), "R": (232, 110, 42)}
ORANGE_T, ORANGE_B = (246, 154, 74), (232, 110, 42)
CREAM, INK, MUTED, OUTLINE = (255, 247, 232), (36, 33, 42), (107, 100, 117), (58, 44, 72)
FB = "/usr/share/fonts/truetype/ubuntu/Ubuntu-B.ttf"
FR = "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf"

# Hand-drawn tiny versions ('.' = transparent)
APP16 = [
    "..oooooooooooo..",
    ".oOOOOOOOOOOOOo.",
    "oOO#OOOOO#OOOOOo",
    "oO#W#####W#OOOOo",
    "oO#WWWWWWW#OOOOo",
    "oO#W#WWW#W#OOOOo",
    "oO#WW###WW#OOOOo",
    "oOO#WWWWW#OOOOOo",
    "oO#YY#Z#YY#O#OOo",
    "o#W#YYZYY#W##OOo",
    "oO#YYYQYYY##OOOo",
    "oO#yyyZyyy#OOOOo",
    "oO###########OOo",
    "oOOOOOOOOOOOOOOo",
    ".oOOOOOOOOOOOOo.",
    "..oooooooooooo..",
]
FILE16 = [
    "..#########.....",
    "..#FFFFFFF#f#...",
    "..#FFFFFFF#ff#..",
    "..#FFFFFFF#fff#.",
    "..#FFFFFFF#####.",
    "..#FF###FFFFFF#.",
    "..#F#YYY#####F#.",
    "..#F#YYYZYYY#F#.",
    "..#F#YYYQYYY#F#.",
    "..#F#YYZZZYY#F#.",
    "..#F#yyZQZyy#F#.",
    "..#F#########F#.",
    "..#FFFFFFFFFFF#.",
    "..#RRRRRRRRRRR#.",
    "..#RRRRRRRRRRR#.",
    "..#############.",
]


def draw_map(rows, cell=1):
    h, w = len(rows), len(rows[0])
    im = Image.new("RGBA", (w * cell, h * cell), (0, 0, 0, 0)); d = ImageDraw.Draw(im)
    for r, row in enumerate(rows):
        for c, k in enumerate(row):
            if k in PAL:
                d.rectangle([c * cell, r * cell, (c + 1) * cell - 1, (r + 1) * cell - 1], fill=PAL[k] + (255,))
    return im


def cat(height):
    """the zip-folder GmCat drawn straight from her grid, whole-pixel cells, original wide cell shape"""
    ch = max(1, height // len(GRID)); cw = max(1, round(ch * 13.36 / 12.74))
    h, w = len(GRID), len(GRID[0])
    im = Image.new("RGBA", (w * cw, h * ch), (0, 0, 0, 0)); d = ImageDraw.Draw(im)
    for r, row in enumerate(GRID):
        for c, k in enumerate(row):
            if k in PAL:
                d.rectangle([c * cw, r * ch, (c + 1) * cw - 1, (r + 1) * ch - 1], fill=PAL[k] + (255,))
    return im


def gradient(w, h, top, bottom):
    im = Image.new("RGBA", (w, h)); d = ImageDraw.Draw(im)
    for y in range(h):
        t = y / max(1, h - 1)
        d.line([(0, y), (w, y)], fill=tuple(int(top[i] * (1 - t) + bottom[i] * t) for i in range(3)) + (255,))
    return im


def rounded_mask(S, inset, radius, M=4):
    big = Image.new("L", (S * M, S * M), 0); d = ImageDraw.Draw(big)
    x0, y0, x1, y1, r = inset * M, inset * M, (S - inset) * M, (S - inset) * M, radius * M
    d.rectangle([x0 + r, y0, x1 - r, y1], fill=255); d.rectangle([x0, y0 + r, x1, y1 - r], fill=255)
    for cx, cy in ((x0, y0), (x1 - 2 * r, y0), (x0, y1 - 2 * r), (x1 - 2 * r, y1 - 2 * r)):
        d.ellipse([cx, cy, cx + 2 * r, cy + 2 * r], fill=255)
    return big.resize((S, S), Image.LANCZOS)


def app_icon(S=1024):
    icon = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    icon.paste(gradient(S, S, ORANGE_T, ORANGE_B), (0, 0), rounded_mask(S, 40, 200))
    c = cat(720)
    icon.alpha_composite(c, ((S - c.width) // 2, (S - c.height) // 2 + 20))
    return icon


def outlined_polygon(d, pts, fill, outline, width):
    d.polygon(pts, fill=fill); d.line(pts + [pts[0]], fill=outline, width=width)
    for x, y in pts:
        r = width // 2; d.ellipse([x - r, y - r, x + r, y + r], fill=outline)


def rounded_rect(d, box, radius, fill):
    x0, y0, x1, y1 = box; r = radius
    d.rectangle([x0 + r, y0, x1 - r, y1], fill=fill); d.rectangle([x0, y0 + r, x1, y1 - r], fill=fill)
    for cx, cy in ((x0, y0), (x1 - 2 * r, y0), (x0, y1 - 2 * r), (x1 - 2 * r, y1 - 2 * r)):
        d.ellipse([cx, cy, cx + 2 * r, cy + 2 * r], fill=fill)


def ctext(d, xy, t, font, fill, middle=False):
    w, h = font.getsize(t); x, y = xy
    d.text((x - w / 2, y - (h / 2 if middle else 0)), t, font=font, fill=fill)


def file_icon(S=1024):
    im = Image.new("RGBA", (S, S), (0, 0, 0, 0)); d = ImageDraw.Draw(im)
    x0, y0, x1, y1, fold = 170, 40, 854, 984, 190
    outlined_polygon(d, [(x0, y0), (x1 - fold, y0), (x1, y0 + fold), (x1, y1), (x0, y1)], PAL["F"] + (255,), OUTLINE + (255,), 26)
    outlined_polygon(d, [(x1 - fold, y0), (x1 - fold, y0 + fold), (x1, y0 + fold)], PAL["f"] + (255,), OUTLINE + (255,), 26)
    c = cat(580); im.alpha_composite(c, ((S - c.width) // 2 - 10, 160))
    rounded_rect(d, [x0 + 60, 770, x1 - 60, 920], 40, ORANGE_B + (255,))
    ctext(d, (S // 2, 845), "PURR", ImageFont.truetype(FB, 120), (255, 255, 255), middle=True)
    return im


def sizes(big, tiny16, out_sizes=(16, 24, 32, 48, 64, 128, 256)):
    """one image per size: the hand-drawn 16 for 16 px, 16 doubled for 32, smooth shrinks above"""
    res = {}
    for s in out_sizes:
        if s == 16:
            res[s] = tiny16
        elif s == 32:
            res[s] = tiny16.resize((32, 32), Image.NEAREST)
        else:
            res[s] = big.convert("RGBa").resize((s, s), Image.LANCZOS).convert("RGBA")
    return res


def write_ico(images, path):
    """ICO with PNG entries (Windows Vista+), each size from its own picture"""
    blobs = []
    for s, im in sorted(images.items()):
        b = io.BytesIO(); im.save(b, "PNG"); blobs.append((s, b.getvalue()))
    head = struct.pack("<HHH", 0, 1, len(blobs)); off = 6 + 16 * len(blobs); dirs = b""; data = b""
    for s, png in blobs:
        dirs += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(png), off + len(data)); data += png
    open(path, "wb").write(head + dirs + data)


def wordmark(d, xy, size, centre=True, on_orange=True):
    f = ImageFont.truetype(FB, size); x, y = xy
    wa = f.getsize("Purr")[0]; wb = f.getsize("zip")[0]
    if centre:
        x -= (wa + wb) / 2
    d.text((x, y), "Purr", font=f, fill=(255, 255, 255) if on_orange else INK)
    d.text((x + wa, y), "zip", font=f, fill=OUTLINE if on_orange else ORANGE_B)


def sidebar():  # NSIS welcome/finish pages, 164x314, orange
    im = gradient(164, 314, ORANGE_T, ORANGE_B); d = ImageDraw.Draw(im)
    for y in range(6, 314, 12):
        for x in range(6, 164, 12):
            d.point((x, y), fill=(255, 255, 255, 40))
    c = cat(145); im.alpha_composite(c, ((164 - c.width) // 2, 50))
    d = ImageDraw.Draw(im)
    wordmark(d, (82, 214), 30)
    ctext(d, (82, 250), "by GmCat", ImageFont.truetype(FB, 14), (255, 247, 232))
    ctext(d, (82, 282), "Open and make archives", ImageFont.truetype(FR, 11), (255, 238, 214))
    return im


def header():  # NSIS other pages, 150x57
    im = Image.new("RGBA", (150, 57), (255, 255, 255, 255))
    c = cat(58)  # 2 px cells: 58 px tall, the last (empty-ish) row is trimmed to fit 57
    c = c.crop((0, 0, c.width, 57))
    im.alpha_composite(c, (150 - c.width - 6, 0))
    d = ImageDraw.Draw(im)
    wordmark(d, (6, 12), 22, centre=False, on_orange=False)
    d.text((7, 37), "by GmCat", font=ImageFont.truetype(FB, 10), fill=MUTED)
    return im


def installer_icon(base):
    im = base.copy(); d = ImageDraw.Draw(im)
    cx, cy, r = 790, 790, 190
    d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=OUTLINE + (255,))
    d.ellipse([cx - r + 22, cy - r + 22, cx + r - 22, cy + r - 22], fill=(255, 255, 255, 255))
    u = 26
    cells = [(gx, gy) for gy in (-4, -3, -2, -1) for gx in (-1, 0, 1)]
    for gy, half in ((0, 3), (1, 2), (2, 1), (3, 0)):
        cells += [(gx, gy) for gx in range(-half, half + 1)]
    for gx, gy in cells:
        x0 = cx + gx * u - u // 2; y0 = cy + gy * u - u // 2 + 6
        d.rectangle([x0, y0, x0 + u - 1, y0 + u - 1], fill=ORANGE_B + (255,))
    return im


def installer16():
    rows = [list(r) for r in APP16]
    for r in range(9, 16):  # small white arrow badge bottom right
        for c in range(9, 16):
            rows[r][c] = rows[r][c]
    badge = ["..###..", ".#WWW#.", "#WWRWW#", "#WRRRW#", "#WWRWW#", ".#WWW#.", "..###.."]
    for dr, line in enumerate(badge):
        for dc, k in enumerate(line):
            if k != ".":
                rows[9 + dr][9 + dc] = k
    return draw_map(["".join(r) for r in rows])


def dmg_background(scale):
    W, H = 660 * scale, 400 * scale
    im = Image.new("RGBA", (W, H), CREAM + (255,)); d = ImageDraw.Draw(im)
    for y in range(7 * scale, H, 14 * scale):
        for x in range(7 * scale, W, 14 * scale):
            d.rectangle([x, y, x + scale - 1, y + scale - 1], fill=(234, 223, 202, 255))
    wordmark(d, (W // 2, 26 * scale), 34 * scale, on_orange=False)
    ctext(d, (W // 2, 70 * scale), "Drag Purrzip into Applications", ImageFont.truetype(FR, 16 * scale), MUTED)
    u = 12 * scale; y = 210 * scale
    for i in range(8):
        d.rectangle([(270 + i * 15) * scale, y - u // 2, (270 + i * 15) * scale + u - 1, y + u // 2 - 1], fill=ORANGE_B + (255,))
    tip = 270 * scale + 8 * 15 * scale
    for k in range(4):
        d.rectangle([tip + k * u, y - u // 2 - (3 - k) * u, tip + k * u + u - 1, y + u // 2 - 1 + (3 - k) * u], fill=ORANGE_B + (255,))
    c = cat(116 * scale); im.alpha_composite(c, (W - c.width - 18 * scale, H - c.height - 10 * scale))
    d.text((24 * scale, H - 30 * scale), "Free · no ads · no account · by GmCat", font=ImageFont.truetype(FR, 12 * scale), fill=MUTED)
    return im


if __name__ == "__main__":
    os.makedirs(os.path.join(ART, "purr-file"), exist_ok=True); os.makedirs(os.path.join(ART, "installer"), exist_ok=True)
    app = app_icon(); app.save(os.path.join(ART, "icon-1024.png"))
    a16 = draw_map(APP16); a16.save(os.path.join(ART, "icon-16.png"))
    write_ico(sizes(app, a16), os.path.join(ART, "icon.ico"))
    f = file_icon(); f.save(os.path.join(ART, "purr-file", "purr-1024.png"))
    f16 = draw_map(FILE16); f16.save(os.path.join(ART, "purr-file", "purr-16.png"))
    write_ico(sizes(f, f16), os.path.join(ART, "purr-file", "purr.ico"))
    for s, im in sizes(f, f16, (16, 32, 48, 64, 128, 256, 512)).items():
        im.save(os.path.join(ART, "purr-file", f"purr-{s}.png"))
    side = sidebar(); side.convert("RGB").save(os.path.join(ART, "installer", "sidebar.bmp")); side.save(os.path.join(ART, "installer", "sidebar.png"))
    hd = header(); hd.convert("RGB").save(os.path.join(ART, "installer", "header.bmp")); hd.save(os.path.join(ART, "installer", "header.png"))
    ii = installer_icon(app); ii.save(os.path.join(ART, "installer", "installer-1024.png"))
    write_ico(sizes(ii, installer16()), os.path.join(ART, "installer", "installer.ico"))
    dmg_background(1).save(os.path.join(ART, "installer", "dmg-background.png"))
    dmg_background(2).save(os.path.join(ART, "installer", "dmg-background@2x.png"))
    print("brand art written to", ART)
